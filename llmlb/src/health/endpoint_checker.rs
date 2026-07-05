//! エンドポイントヘルスチェッカー
//!
//! プル型ヘルスチェックでエンドポイントの稼働状況を監視
//!
//! ## Phase 1.4: `/api/health`対応
//!
//! - xLLMエンドポイントでは`/api/health`を優先的に呼び出し、GPU情報を取得
//! - `/api/health`が失敗した場合、またはxLLM以外のエンドポイントでは`/v1/models`をフォールバック

use crate::db::endpoints as db;
use crate::registry::endpoints::EndpointRegistry;
use crate::types::endpoint::{Endpoint, EndpointHealthCheck, EndpointStatus, EndpointType};
use chrono::Utc;
use reqwest::Client;

use std::time::{Duration, Instant};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

mod probe;
mod recovery;
mod scheduler;
#[cfg(test)]
use probe::GpuInfo;

/// ヘルスチェックのタイムアウト（秒）
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;

/// デフォルトのチェック間隔（秒）
const DEFAULT_CHECK_INTERVAL_SECS: u64 = 30;

/// オフライン判定までの連続失敗回数
const CONSECUTIVE_FAILURES_FOR_OFFLINE: u32 = 2;

/// エンドポイントヘルスチェッカー
///
/// 定期的にエンドポイントにGET /v1/modelsリクエストを送信し、
/// 稼働状況を監視する。
#[derive(Clone)]
pub struct EndpointHealthChecker {
    /// エンドポイントレジストリ
    registry: EndpointRegistry,
    /// offline/error遷移時のTPSリセット用ロードマネージャー
    load_manager: Option<crate::balancer::LoadManager>,
    /// HTTPクライアント
    client: Client,
    /// チェック間隔（秒）
    check_interval_secs: u64,
    /// 同一エンドポイントに対するモデル自動同期の最短間隔
    auto_sync_models_interval: Duration,
    /// エンドポイントごとの最終モデル同期時刻（スロットリング用）
    last_auto_sync_models: Arc<RwLock<HashMap<Uuid, Instant>>>,
}

impl EndpointHealthChecker {
    /// 新しいヘルスチェッカーを作成
    pub fn new(registry: EndpointRegistry) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            registry,
            load_manager: None,
            client,
            check_interval_secs: DEFAULT_CHECK_INTERVAL_SECS,
            auto_sync_models_interval: crate::config::get_auto_sync_models_interval(),
            last_auto_sync_models: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// TPSリセット連携用のLoadManagerを設定する
    pub fn with_load_manager(mut self, load_manager: crate::balancer::LoadManager) -> Self {
        self.load_manager = Some(load_manager);
        self
    }

    /// チェック間隔を設定
    pub fn with_interval(mut self, interval_secs: u64) -> Self {
        self.check_interval_secs = interval_secs;
        self
    }

    /// 単一エンドポイントのヘルスチェック
    ///
    /// Phase 1.4: xLLMのみ`/api/health`を優先的に呼び出し、GPU情報を取得。
    /// `/api/health`が失敗した場合は`/v1/models`にフォールバック。
    /// 非xLLMでは`/api/health`を呼ばず、`/v1/models`で判定する。
    pub async fn check_endpoint(
        &self,
        endpoint: &Endpoint,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let status_before = endpoint.status;
        let (success, error_message, new_status, gpu_info, latency_ms) =
            self.probe_endpoint(endpoint, status_before).await;

        // ステータス更新
        if success {
            self.registry
                .update_status(endpoint.id, new_status, Some(latency_ms), None)
                .await?;
        } else {
            self.registry
                .update_status(endpoint.id, new_status, None, error_message.as_deref())
                .await?;
        }

        if new_status != EndpointStatus::Online {
            if let Some(load_manager) = &self.load_manager {
                load_manager.clear_tps_for_endpoint(endpoint.id).await;
            }
        }

        // GPU情報更新（/api/healthから取得した場合のみ）
        if let Some(info) = gpu_info {
            self.registry
                .update_gpu_info(
                    endpoint.id,
                    info.gpu_device_count,
                    info.gpu_total_memory_bytes,
                    info.gpu_used_memory_bytes,
                    info.gpu_capability_score,
                    info.active_requests,
                )
                .await;
        }

        // SPEC-e8e9326e: offline→online遷移時にタイプ再検出
        let endpoint_type_for_auto_sync = self
            .redetect_type_on_recovery(endpoint, status_before, success)
            .await;

        if success {
            self.maybe_auto_sync_models(endpoint, new_status, endpoint_type_for_auto_sync)
                .await;
        }

        // ヘルスチェック履歴を記録
        let health_check = EndpointHealthCheck {
            id: 0, // DBで自動採番
            endpoint_id: endpoint.id,
            checked_at: Utc::now(),
            success,
            latency_ms: if success { Some(latency_ms) } else { None },
            error_message: error_message.clone(),
            status_before,
            status_after: new_status,
        };

        db::record_health_check(self.registry.pool(), &health_check).await?;

        // ログ出力
        if success {
            debug!(
                endpoint_id = %endpoint.id,
                endpoint_name = %endpoint.name,
                latency_ms = latency_ms,
                status = %new_status.as_str(),
                "Health check succeeded"
            );
        } else {
            warn!(
                endpoint_id = %endpoint.id,
                endpoint_name = %endpoint.name,
                error = ?error_message,
                status = %new_status.as_str(),
                "Health check failed"
            );
        }

        if success {
            Ok(())
        } else {
            Err(error_message
                .unwrap_or_else(|| "Unknown error".to_string())
                .into())
        }
    }

    /// 特定エンドポイントの手動チェック（APIから呼び出し用）
    pub async fn check_endpoint_by_id(
        &self,
        endpoint_id: Uuid,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = self
            .registry
            .get(endpoint_id)
            .await
            .ok_or("Endpoint not found")?;

        self.check_endpoint(&endpoint).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::protocol::TpsApiKind;
    use crate::db::test_utils::TEST_LOCK;
    use crate::types::endpoint::{EndpointModel, EndpointType, SupportedAPI};
    use serde_json::json;
    use sqlx::SqlitePool;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_health_checker_creation() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        assert_eq!(checker.check_interval_secs, DEFAULT_CHECK_INTERVAL_SECS);
    }

    #[tokio::test]
    async fn test_health_checker_with_interval() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry).with_interval(60);

        assert_eq!(checker.check_interval_secs, 60);
    }

    #[tokio::test]
    async fn test_determine_failure_status_pending() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );

        // pending → offline（即時）
        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Pending);
        assert_eq!(new_status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_determine_failure_status_online_first_failure() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );

        // online + error_count=0 → error（1回目の失敗）
        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Online);
        assert_eq!(new_status, EndpointStatus::Error);
    }

    #[tokio::test]
    async fn test_determine_failure_status_online_second_failure() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let mut endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        endpoint.error_count = 1; // 既に1回失敗

        // online + error_count=1 → offline（2回目の失敗でoffline）
        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Online);
        assert_eq!(new_status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_health_check_skips_api_health_for_non_xllm_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        let health_call_count = Arc::new(AtomicUsize::new(0));
        let v1_call_count = Arc::new(AtomicUsize::new(0));

        let health_call_count_clone = health_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(move |_req: &wiremock::Request| {
                health_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({"health": "ok"}))
            })
            .mount(&mock)
            .await;

        let v1_call_count_clone = v1_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(move |_req: &wiremock::Request| {
                v1_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({
                    "object": "list",
                    "data": [{"id": "test-model", "object": "model"}]
                }))
            })
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new(
            "Test".to_string(),
            mock.uri(),
            EndpointType::OpenaiCompatible,
        );
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        assert_eq!(health_call_count.load(Ordering::SeqCst), 0);
        assert_eq!(v1_call_count.load(Ordering::SeqCst), 1);

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
    }

    #[tokio::test]
    async fn test_health_check_uses_api_health_for_xllm_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        let health_call_count = Arc::new(AtomicUsize::new(0));
        let v1_call_count = Arc::new(AtomicUsize::new(0));

        let health_call_count_clone = health_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(move |_req: &wiremock::Request| {
                health_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({
                    "gpu": {
                        "device_count": 1,
                        "total_memory_bytes": 16_000_000_000u64,
                        "used_memory_bytes": 4_000_000_000u64
                    },
                    "load": {
                        "active_requests": 2
                    }
                }))
            })
            .mount(&mock)
            .await;

        let v1_call_count_clone = v1_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(move |_req: &wiremock::Request| {
                v1_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({
                    "object": "list",
                    "data": [{"id": "xllm-model", "object": "model"}]
                }))
            })
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        assert_eq!(health_call_count.load(Ordering::SeqCst), 1);
        assert_eq!(v1_call_count.load(Ordering::SeqCst), 0);

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
        assert_eq!(updated.gpu_device_count, Some(1));
        assert_eq!(updated.active_requests, Some(2));
    }

    #[tokio::test]
    async fn test_health_check_falls_back_to_v1_models_when_api_health_fails_for_xllm() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        let health_call_count = Arc::new(AtomicUsize::new(0));
        let v1_call_count = Arc::new(AtomicUsize::new(0));

        let health_call_count_clone = health_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(move |_req: &wiremock::Request| {
                health_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(500).set_body_string("internal error")
            })
            .mount(&mock)
            .await;

        let v1_call_count_clone = v1_call_count.clone();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(move |_req: &wiremock::Request| {
                v1_call_count_clone.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(json!({
                    "object": "list",
                    "data": [{"id": "xllm-model", "object": "model"}]
                }))
            })
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        assert_eq!(health_call_count.load(Ordering::SeqCst), 1);
        assert_eq!(v1_call_count.load(Ordering::SeqCst), 1);

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
        assert_eq!(updated.gpu_device_count, None);
    }

    #[tokio::test]
    async fn test_health_check_triggers_auto_model_sync() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [
                    {"id": "auto-model-1", "object": "model"},
                    {"id": "auto-model-2", "object": "model"}
                ]
            })))
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        // Auto sync is expected to run asynchronously after a successful health check.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let models = registry.list_models(endpoint.id).await.unwrap();
            if models.iter().any(|m| m.model_id == "auto-model-1") {
                break;
            }
            if Instant::now() > deadline {
                panic!("Timed out waiting for auto model sync after health check");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn test_auto_model_sync_uses_redetected_endpoint_type_for_metadata() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [
                    {
                        "type": "llm",
                        "publisher": "lmstudio-community",
                        "key": "lmstudio-model",
                        "display_name": "LM Studio Model",
                        "architecture": "llama",
                        "loaded_instances": [],
                        "max_context_length": 32768,
                        "format": "gguf"
                    }
                ]
            })))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{"id": "lmstudio-model", "object": "model"}]
            })))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models/lmstudio-model"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "lmstudio-model",
                "max_context_length": 32768
            })))
            .mount(&mock)
            .await;

        let mut endpoint = Endpoint::new(
            "Test".to_string(),
            mock.uri(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Offline;
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let models = registry.list_models(endpoint.id).await.unwrap();
            if let Some(model) = models.iter().find(|m| m.model_id == "lmstudio-model") {
                if model.max_tokens == Some(32768) {
                    break;
                }
            }

            if Instant::now() > deadline {
                panic!("Timed out waiting for auto model sync metadata update");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.endpoint_type, EndpointType::LmStudio);
    }

    #[tokio::test]
    async fn test_auto_model_sync_retries_after_failure() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(move |_req: &wiremock::Request| {
                let n = calls_clone.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(500)
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "object": "list",
                        "data": [
                            {"id": "auto-retry-model-1", "object": "model"},
                        ]
                    }))
                }
            })
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        registry.add(endpoint.clone()).await.unwrap();

        let mut checker = EndpointHealthChecker::new(registry.clone());
        // Ensure a failed sync would normally be throttled for a long time.
        checker.auto_sync_models_interval = Duration::from_secs(60 * 60);

        checker.check_endpoint(&endpoint).await.unwrap();

        // Wait for the initial auto sync attempt to hit /v1/models and fail.
        let attempt_deadline = Instant::now() + Duration::from_secs(1);
        while calls.load(Ordering::SeqCst) < 1 {
            if Instant::now() > attempt_deadline {
                panic!("Timed out waiting for initial auto model sync attempt");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;

        // A subsequent successful health check should retry auto sync even if the normal interval is long,
        // because the previous sync attempt failed.
        checker.check_endpoint(&endpoint).await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let models = registry.list_models(endpoint.id).await.unwrap();
            if models
                .iter()
                .any(|model| model.model_id == "auto-retry-model-1")
            {
                break;
            }
            if Instant::now() > deadline {
                panic!("Timed out waiting for auto model sync retry after failure");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // --- additional coverage tests ---

    #[test]
    fn test_gpu_info_default() {
        let info = GpuInfo::default();
        assert!(info.gpu_device_count.is_none());
        assert!(info.gpu_total_memory_bytes.is_none());
        assert!(info.gpu_used_memory_bytes.is_none());
        assert!(info.gpu_capability_score.is_none());
        assert!(info.active_requests.is_none());
    }

    #[test]
    fn test_gpu_info_clone() {
        let info = GpuInfo {
            gpu_device_count: Some(2),
            gpu_total_memory_bytes: Some(16_000_000_000),
            gpu_used_memory_bytes: Some(8_000_000_000),
            gpu_capability_score: Some(0.95),
            active_requests: Some(5),
        };
        let cloned = info.clone();
        assert_eq!(cloned.gpu_device_count, Some(2));
        assert_eq!(cloned.gpu_total_memory_bytes, Some(16_000_000_000));
        assert_eq!(cloned.gpu_used_memory_bytes, Some(8_000_000_000));
        assert_eq!(cloned.gpu_capability_score, Some(0.95));
        assert_eq!(cloned.active_requests, Some(5));
    }

    #[tokio::test]
    async fn test_determine_failure_status_error_first_failure() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );

        // error + error_count=0 → error (not enough failures for offline)
        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Error);
        assert_eq!(new_status, EndpointStatus::Error);
    }

    #[tokio::test]
    async fn test_determine_failure_status_error_reaches_threshold() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let mut endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );
        endpoint.error_count = 1; // already 1 failure

        // error + error_count=1 → offline (reaches threshold)
        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Error);
        assert_eq!(new_status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_determine_failure_status_offline_stays_offline() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let endpoint = Endpoint::new(
            "Test".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::Xllm,
        );

        let new_status = checker.determine_failure_status(&endpoint, EndpointStatus::Offline);
        assert_eq!(new_status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_check_all_endpoints_empty_registry() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        // Should succeed with no endpoints
        let result = checker.check_all_endpoints().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_all_endpoints_parallel_empty_registry() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let result = checker.check_all_endpoints_parallel().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_both_fail_for_xllm_goes_offline() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;

        let mut endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        // Set status to pending so first failure goes directly to offline
        endpoint.status = EndpointStatus::Pending;
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        let result = checker.check_endpoint(&endpoint).await;
        assert!(result.is_err());

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_health_check_non_xllm_failure_from_online() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock)
            .await;

        let mut endpoint = Endpoint::new(
            "Test".to_string(),
            mock.uri(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        endpoint.error_count = 0;
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        let result = checker.check_endpoint(&endpoint).await;
        assert!(result.is_err());

        let updated = registry.get(endpoint.id).await.unwrap();
        // First failure from online goes to Error status
        assert_eq!(updated.status, EndpointStatus::Error);
    }

    #[tokio::test]
    async fn test_failure_clears_endpoint_tps_when_load_manager_is_wired() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let load_manager = crate::balancer::LoadManager::new(Arc::new(registry.clone()));

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&mock)
            .await;

        let mut endpoint = Endpoint::new(
            "TPS Reset Target".to_string(),
            mock.uri(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        registry.add(endpoint.clone()).await.unwrap();
        registry
            .add_model(&EndpointModel {
                endpoint_id: endpoint.id,
                model_id: "shared-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .unwrap();

        load_manager
            .update_tps(
                endpoint.id,
                "shared-model".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                1_000,
            )
            .await;
        assert_eq!(load_manager.get_model_tps(endpoint.id).await.len(), 1);

        let checker =
            EndpointHealthChecker::new(registry.clone()).with_load_manager(load_manager.clone());
        let result = checker.check_endpoint(&endpoint).await;
        assert!(result.is_err());

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Error);
        assert!(
            load_manager.get_model_tps(endpoint.id).await.is_empty(),
            "TPS state should be cleared after offline/error transition"
        );
    }

    #[tokio::test]
    async fn test_with_interval_chaining() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry)
            .with_interval(10)
            .with_interval(120);

        assert_eq!(checker.check_interval_secs, 120);
    }

    #[tokio::test]
    async fn test_check_endpoint_by_id_not_found() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();
        let checker = EndpointHealthChecker::new(registry);

        let result = checker.check_endpoint_by_id(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_xllm_health_with_partial_gpu_info() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;
        let registry = EndpointRegistry::new(pool).await.unwrap();

        let mock = MockServer::start().await;
        // Return only partial GPU info (no load section)
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "gpu": {
                    "device_count": 4,
                    "total_memory_bytes": 80_000_000_000u64
                }
            })))
            .mount(&mock)
            .await;

        let endpoint = Endpoint::new("Test".to_string(), mock.uri(), EndpointType::Xllm);
        registry.add(endpoint.clone()).await.unwrap();

        let checker = EndpointHealthChecker::new(registry.clone());
        checker.check_endpoint(&endpoint).await.unwrap();

        let updated = registry.get(endpoint.id).await.unwrap();
        assert_eq!(updated.status, EndpointStatus::Online);
        assert_eq!(updated.gpu_device_count, Some(4));
        assert_eq!(updated.active_requests, None);
    }
}
