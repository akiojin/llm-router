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
mod tests;
