//! ロードバランサーモジュール
//!
//! エンドポイントのメトリクスとリクエスト統計を集約し、
//! TPS優先のロードバランシングを提供する。
//!
//! # EndpointRegistry統合
//!
//! このモジュールはEndpointRegistryを使用してエンドポイント情報を管理します。
//! 負荷分散はTPS優先、同一TPS時はラウンドロビンで行われます。

mod admission;
mod history;
pub mod lease;
mod selection;
mod tps;
pub mod types;

// Re-export all public types for backward compatibility
pub use lease::RequestLease;
pub use types::{
    EndpointLoadSnapshot, EndpointTpsSummary, MetricsUpdate, ModelTpsInfo, ModelTpsState,
    RequestHistoryPoint, RequestOutcome, SystemSummary, WaitResult,
};

use types::{EndpointLoadState, TpsTrackerMap};

use crate::common::error::{LbError, RouterResult};
use crate::common::protocol::TpsApiKind;
use crate::registry::endpoints::EndpointRegistry;
use crate::types::HealthMetrics;
use chrono::{DateTime, Utc};
// 履歴 free 関数と Timelike はテストのみが直接参照するため cfg(test) で取り込む。
#[cfg(test)]
use chrono::Timelike;
#[cfg(test)]
use history::{
    align_to_minute, build_history_window, fill_history, increment_history, new_history_point,
    prune_history,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering},
        Arc,
    },
};
// StdDuration はテストのみが参照するため cfg(test) で取り込む。
#[cfg(test)]
use std::time::Duration as StdDuration;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

/// LoadManagerインスタンスIDの採番カウンタ
static NEXT_LOAD_MANAGER_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::TEST_LOCK;
    use crate::types::endpoint::{
        Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
    };
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;
    use tokio::time::{sleep, Duration};

    async fn setup_test_load_manager() -> (LoadManager, Uuid) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to create test database");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        let registry = EndpointRegistry::new(pool)
            .await
            .expect("Failed to create endpoint registry");
        let endpoint = Endpoint::new(
            "lease-test-endpoint".to_string(),
            "http://localhost:11434".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let endpoint_id = endpoint.id;
        registry
            .add(endpoint)
            .await
            .expect("Failed to add test endpoint");

        let load_manager = LoadManager::new(Arc::new(registry));
        (load_manager, endpoint_id)
    }

    // NOTE: SPEC-e8e9326eによりNodeRegistryは廃止されました。
    // SPEC-f8e3a1b7によりNode型は削除され、Endpoint型に移行しました。
    // compare_average_ms_orders_values テストは関数削除に伴い削除されました。

    #[test]
    fn effective_average_ms_prefers_metrics_value() {
        let timestamp = Utc::now();
        let state = EndpointLoadState {
            success_count: 5,
            total_latency_ms: 500,
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 10.0,
                memory_usage: 20.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                total_requests: 5,
                average_response_time_ms: Some(80.0),
                timestamp,
            }),
            ..Default::default()
        };

        assert_eq!(state.effective_average_ms(), Some(80.0));
    }

    // SPEC-f8e3a1b7: NodeRegistry依存のテストは削除されました
    // - load_manager_prefers_lower_latency_when_active_equal
    // - metrics_history_tracks_recent_points
    // - wait_for_ready_unblocks_when_node_becomes_ready
    // - wait_for_ready_limits_waiters_and_notifies_first
    // 新しいEndpointRegistryベースのテストは tests/integration/ に追加してください

    // T004: WaitResult enum テスト
    #[test]
    fn wait_result_enum_variants_exist() {
        // WaitResultの3つのバリアントが存在することを確認
        let ready = WaitResult::Ready;
        let timeout = WaitResult::Timeout;
        let capacity_exceeded = WaitResult::CapacityExceeded;

        // PartialEq実装の確認
        assert_eq!(ready, WaitResult::Ready);
        assert_eq!(timeout, WaitResult::Timeout);
        assert_eq!(capacity_exceeded, WaitResult::CapacityExceeded);
        assert_ne!(ready, timeout);

        // Debug実装の確認
        assert!(!format!("{:?}", ready).is_empty());
    }

    #[tokio::test]
    async fn load_manager_cache_key_is_stable_and_unique_per_instance() {
        let _lock = TEST_LOCK.lock().await;

        let (load_manager, _) = setup_test_load_manager().await;
        let cloned = load_manager.clone();
        assert_eq!(load_manager.cache_key(), cloned.cache_key());

        let (another, _) = setup_test_load_manager().await;
        assert_ne!(load_manager.cache_key(), another.cache_key());
    }

    // SPEC-f8e3a1b7: wait_for_ready_with_timeout / admission_control テストは削除されました

    #[test]
    fn test_node_load_state_token_accumulation() {
        let mut state = EndpointLoadState::default();

        assert_eq!(state.total_input_tokens, 0);
        assert_eq!(state.total_output_tokens, 0);
        assert_eq!(state.total_tokens, 0);

        state.total_input_tokens += 100;
        state.total_output_tokens += 50;
        state.total_tokens += 150;

        assert_eq!(state.total_input_tokens, 100);
        assert_eq!(state.total_output_tokens, 50);
        assert_eq!(state.total_tokens, 150);

        state.total_input_tokens += 200;
        state.total_output_tokens += 100;
        state.total_tokens += 300;

        assert_eq!(state.total_input_tokens, 300);
        assert_eq!(state.total_output_tokens, 150);
        assert_eq!(state.total_tokens, 450);
    }

    #[test]
    fn test_node_load_state_average_tokens_per_request() {
        let state = EndpointLoadState {
            total_assigned: 10,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_tokens: 1500,
            ..Default::default()
        };

        let avg = if state.total_assigned > 0 {
            state.total_tokens as f32 / state.total_assigned as f32
        } else {
            0.0
        };
        assert_eq!(avg, 150.0);
    }

    #[tokio::test]
    async fn request_lease_complete_releases_active_counter() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        let lease = load_manager
            .begin_request(endpoint_id)
            .await
            .expect("begin_request should succeed");

        let active_before = load_manager
            .snapshot(endpoint_id)
            .await
            .expect("snapshot should succeed")
            .active_requests;
        assert_eq!(active_before, 1);

        lease
            .complete(RequestOutcome::Success, StdDuration::from_millis(3))
            .await
            .expect("complete should succeed");

        let snapshot_after = load_manager
            .snapshot(endpoint_id)
            .await
            .expect("snapshot should succeed");
        assert_eq!(snapshot_after.active_requests, 0);
        assert_eq!(snapshot_after.successful_requests, 1);
    }

    #[tokio::test]
    async fn request_lease_drop_auto_releases_active_counter() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        {
            let _lease = load_manager
                .begin_request(endpoint_id)
                .await
                .expect("begin_request should succeed");
        }

        let mut final_snapshot = None;
        for _ in 0..30 {
            let snapshot = load_manager
                .snapshot(endpoint_id)
                .await
                .expect("snapshot should succeed");
            if snapshot.active_requests == 0 {
                final_snapshot = Some(snapshot);
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        let snapshot = final_snapshot.expect("lease auto-complete should drain active requests");
        assert_eq!(snapshot.active_requests, 0);
        assert_eq!(snapshot.failed_requests, 1);
    }

    #[tokio::test]
    async fn select_endpoint_round_robin_ready_for_model_excludes_initializing_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to create test database");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        let registry = EndpointRegistry::new(pool)
            .await
            .expect("Failed to create endpoint registry");

        let mut ready_endpoint = Endpoint::new(
            "ready-endpoint".to_string(),
            "http://localhost:11080".to_string(),
            EndpointType::OpenaiCompatible,
        );
        ready_endpoint.status = EndpointStatus::Online;
        let ready_endpoint_id = ready_endpoint.id;
        registry
            .add(ready_endpoint)
            .await
            .expect("Failed to add ready endpoint");

        let mut initializing_endpoint = Endpoint::new(
            "initializing-endpoint".to_string(),
            "http://localhost:11081".to_string(),
            EndpointType::OpenaiCompatible,
        );
        initializing_endpoint.status = EndpointStatus::Online;
        let initializing_endpoint_id = initializing_endpoint.id;
        registry
            .add(initializing_endpoint)
            .await
            .expect("Failed to add initializing endpoint");

        let model_id = "gpt-oss:latest".to_string();
        for endpoint_id in [ready_endpoint_id, initializing_endpoint_id] {
            registry
                .add_model(&EndpointModel {
                    endpoint_id,
                    model_id: model_id.clone(),
                    capabilities: None,
                    max_tokens: None,
                    last_checked: None,
                    supported_apis: vec![SupportedAPI::ChatCompletions],
                    canonical_name: None,
                })
                .await
                .expect("Failed to add endpoint model");
        }

        let load_manager = LoadManager::new(Arc::new(registry));
        load_manager
            .upsert_initial_state(ready_endpoint_id, false, Some((1, 1)))
            .await;
        load_manager
            .upsert_initial_state(initializing_endpoint_id, true, Some((0, 1)))
            .await;

        for _ in 0..4 {
            let selected = load_manager
                .select_endpoint_round_robin_ready_for_model(&model_id)
                .await
                .expect("selection should succeed");
            assert_eq!(
                selected.id, ready_endpoint_id,
                "initializing endpoint must not be selected"
            );
        }
    }

    // SPEC-4bb5b55f T002: ModelTpsState EMA計算テスト

    #[test]
    fn test_model_tps_state_initial_none() {
        let state = ModelTpsState::default();
        assert!(state.tps_ema.is_none());
        assert_eq!(state.request_count, 0);
        assert_eq!(state.total_output_tokens, 0);
        assert_eq!(state.total_duration_ms, 0);
    }

    #[test]
    fn test_model_tps_state_first_update() {
        let mut state = ModelTpsState::default();
        state.update_tps(100, 2000);
        assert!(state.tps_ema.is_some());
        let tps = state.tps_ema.unwrap();
        assert!(
            (tps - 50.0).abs() < 0.01,
            "初回TPS: expected 50.0, got {tps}"
        );
        assert_eq!(state.request_count, 1);
        assert_eq!(state.total_output_tokens, 100);
        assert_eq!(state.total_duration_ms, 2000);
    }

    #[test]
    fn test_model_tps_state_ema_smoothing() {
        let mut state = ModelTpsState::default();

        state.update_tps(100, 2000);
        assert!((state.tps_ema.unwrap() - 50.0).abs() < 0.01);

        state.update_tps(200, 2000);
        assert!(
            (state.tps_ema.unwrap() - 60.0).abs() < 0.01,
            "2回目EMA: expected 60.0, got {}",
            state.tps_ema.unwrap()
        );

        state.update_tps(50, 1000);
        assert!(
            (state.tps_ema.unwrap() - 58.0).abs() < 0.01,
            "3回目EMA: expected 58.0, got {}",
            state.tps_ema.unwrap()
        );

        assert_eq!(state.request_count, 3);
        assert_eq!(state.total_output_tokens, 350);
        assert_eq!(state.total_duration_ms, 5000);
    }

    #[test]
    fn test_model_tps_state_zero_duration_skipped() {
        let mut state = ModelTpsState::default();
        state.update_tps(100, 0);
        assert!(state.tps_ema.is_none(), "duration=0ではTPS更新しない");
    }

    #[tokio::test]
    async fn test_get_model_tps_empty_for_unknown_endpoint() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let unknown_id = Uuid::new_v4();
        let result = load_manager.get_model_tps(unknown_id).await;
        assert!(result.is_empty(), "未計測エンドポイントは空Vecを返す");
    }

    #[tokio::test]
    async fn test_get_model_tps_returns_entries_after_update() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                2000,
            )
            .await;

        let result = load_manager.get_model_tps(endpoint_id).await;
        assert_eq!(result.len(), 1, "1モデル分のTPS情報が返る");
        assert_eq!(result[0].model_id, "model-a");
        assert_eq!(result[0].api_kind, TpsApiKind::ChatCompletions);
        assert_eq!(result[0].request_count, 1);
        assert_eq!(result[0].total_output_tokens, 100);
        let tps = result[0].tps.expect("TPS値がSomeであること");
        assert!((tps - 50.0).abs() < 0.01, "TPS = 50.0");
    }

    #[tokio::test]
    async fn test_get_model_tps_multiple_models() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                2000,
            )
            .await;
        load_manager
            .update_tps(
                endpoint_id,
                "model-b".to_string(),
                TpsApiKind::Completions,
                200,
                1000,
            )
            .await;

        let result = load_manager.get_model_tps(endpoint_id).await;
        assert_eq!(result.len(), 2, "2モデル分のTPS情報が返る");

        let model_ids: Vec<&str> = result.iter().map(|e| e.model_id.as_str()).collect();
        assert!(model_ids.contains(&"model-a"));
        assert!(model_ids.contains(&"model-b"));
    }

    #[tokio::test]
    async fn test_get_model_tps_separates_api_kind_for_same_model() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        load_manager
            .update_tps(
                endpoint_id,
                "shared-model".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                2000,
            )
            .await;
        load_manager
            .update_tps(
                endpoint_id,
                "shared-model".to_string(),
                TpsApiKind::Responses,
                80,
                2000,
            )
            .await;

        let result = load_manager.get_model_tps(endpoint_id).await;
        assert_eq!(result.len(), 2, "同一モデルでもAPI種別ごとに分離される");
        assert!(result.iter().any(|entry| entry.model_id == "shared-model"
            && entry.api_kind == TpsApiKind::ChatCompletions));
        assert!(result.iter().any(
            |entry| entry.model_id == "shared-model" && entry.api_kind == TpsApiKind::Responses
        ));
    }

    #[tokio::test]
    async fn test_update_tps_skips_zero_tokens() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                0,
                2000,
            )
            .await;

        let result = load_manager.get_model_tps(endpoint_id).await;
        assert!(result.is_empty(), "output_tokens=0はTPS更新しない");
    }

    #[tokio::test]
    async fn test_get_model_tps_isolates_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let other_endpoint_id = Uuid::new_v4();

        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                2000,
            )
            .await;
        load_manager
            .update_tps(
                other_endpoint_id,
                "model-b".to_string(),
                TpsApiKind::Completions,
                200,
                1000,
            )
            .await;

        let result = load_manager.get_model_tps(endpoint_id).await;
        assert_eq!(result.len(), 1, "他エンドポイントのデータは含まない");
        assert_eq!(result[0].model_id, "model-a");
    }

    #[tokio::test]
    async fn test_get_all_endpoint_tps_empty() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;

        let result = load_manager.get_all_endpoint_tps().await;
        assert!(result.is_empty(), "TPS未計測の場合は空");
    }

    #[tokio::test]
    async fn test_get_all_endpoint_tps_returns_per_endpoint_summary() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let other_endpoint_id = Uuid::new_v4();

        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                2000,
            )
            .await;
        load_manager
            .update_tps(
                endpoint_id,
                "model-b".to_string(),
                TpsApiKind::Completions,
                200,
                1000,
            )
            .await;
        load_manager
            .update_tps(
                other_endpoint_id,
                "model-c".to_string(),
                TpsApiKind::Responses,
                50,
                500,
            )
            .await;

        let result = load_manager.get_all_endpoint_tps().await;
        assert_eq!(result.len(), 2, "2エンドポイント分のサマリ");

        let ep1 = result
            .iter()
            .find(|s| s.endpoint_id == endpoint_id)
            .expect("endpoint_id存在");
        assert_eq!(ep1.model_count, 2);
        assert_eq!(ep1.total_output_tokens, 300);
        assert!(ep1.aggregate_tps.is_some());

        let ep2 = result
            .iter()
            .find(|s| s.endpoint_id == other_endpoint_id)
            .expect("other存在");
        assert_eq!(ep2.model_count, 1);
        assert_eq!(ep2.total_output_tokens, 50);
    }

    #[tokio::test]
    async fn test_forget_endpoint_clears_state_and_tps() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        // 負荷状態（state）を生成
        load_manager
            .upsert_initial_state(endpoint_id, false, Some((1, 1)))
            .await;
        // TPS状態（tps_tracker）を生成
        load_manager
            .update_tps(
                endpoint_id,
                "model-a".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                1_000,
            )
            .await;

        assert!(
            load_manager.state.read().await.contains_key(&endpoint_id),
            "前提: 負荷状態が登録されている"
        );
        assert!(
            !load_manager.get_model_tps(endpoint_id).await.is_empty(),
            "前提: TPS状態が登録されている"
        );

        load_manager.forget_endpoint(endpoint_id).await;

        assert!(
            !load_manager.state.read().await.contains_key(&endpoint_id),
            "forget_endpoint は負荷状態を除去する"
        );
        assert!(
            load_manager.get_model_tps(endpoint_id).await.is_empty(),
            "forget_endpoint はTPS状態を除去する"
        );
    }

    /// T013 [US5]: seed_history_from_db が MinuteHistoryPoint を VecDeque に
    /// 正しく投入できることを検証
    #[tokio::test]
    async fn test_seed_history_from_db() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;

        let now = Utc::now();
        let minute1 = super::align_to_minute(now - chrono::Duration::minutes(5));
        let minute2 = super::align_to_minute(now - chrono::Duration::minutes(3));

        let points = vec![
            crate::db::request_history::MinuteHistoryPoint {
                minute: minute1.to_rfc3339(),
                success_count: 10,
                error_count: 2,
            },
            crate::db::request_history::MinuteHistoryPoint {
                minute: minute2.to_rfc3339(),
                success_count: 5,
                error_count: 1,
            },
        ];

        load_manager.seed_history_from_db(points).await;

        let history = load_manager.request_history().await;
        // VecDeque には seed したデータが含まれる
        let total_success: u64 = history.iter().map(|h| h.success).sum();
        let total_error: u64 = history.iter().map(|h| h.error).sum();
        assert_eq!(total_success, 15, "seeded success count");
        assert_eq!(total_error, 3, "seeded error count");
    }

    /// T018 [US4]: seed_tps_from_db が TpsSeedEntry から TpsTrackerMap に
    /// 正しく TPS EMA を計算して投入できることを検証
    #[tokio::test]
    async fn test_seed_tps_from_db() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        let entries = vec![
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "test-model".to_string(),
                api_kind: "chat_completions".to_string(),
                total_output_tokens: 100,
                total_duration_ms: 2000,
                successful_requests: 5,
            },
            // duration_ms=0 のエントリはスキップされること
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "skip-model".to_string(),
                api_kind: "chat_completions".to_string(),
                total_output_tokens: 100,
                total_duration_ms: 0,
                successful_requests: 1,
            },
            // tokens=0 のエントリもスキップされること
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "skip-model2".to_string(),
                api_kind: "chat_completions".to_string(),
                total_output_tokens: 0,
                total_duration_ms: 1000,
                successful_requests: 1,
            },
        ];

        load_manager.seed_tps_from_db(entries).await;

        let tps_data = load_manager.get_all_endpoint_tps().await;
        // test-model のみ seed されている
        assert!(!tps_data.is_empty(), "should have TPS data");
        let ep_tps = tps_data
            .iter()
            .find(|t| t.endpoint_id == endpoint_id)
            .expect("should have endpoint TPS");
        // TPS = 100 / (2000/1000) = 50.0
        assert_eq!(ep_tps.model_count, 1, "only valid entry should be seeded");
        assert!(ep_tps.aggregate_tps.is_some(), "should have aggregate TPS");
        let tps = ep_tps.aggregate_tps.unwrap();
        assert!((tps - 50.0).abs() < 0.1, "TPS should be ~50.0, got {tps}");
    }

    // ===== 追加テスト: LoadManager 基本機能 =====

    #[test]
    fn endpoint_load_state_default_values() {
        let state = EndpointLoadState::default();
        assert_eq!(state.assigned_active, 0);
        assert_eq!(state.total_assigned, 0);
        assert_eq!(state.success_count, 0);
        assert_eq!(state.error_count, 0);
        assert_eq!(state.total_latency_ms, 0);
        assert!(!state.initializing);
        assert!(state.ready_models.is_none());
        assert!(state.last_metrics.is_none());
        assert!(state.metrics_history.is_empty());
        assert_eq!(state.total_input_tokens, 0);
        assert_eq!(state.total_output_tokens, 0);
        assert_eq!(state.total_tokens, 0);
    }

    #[test]
    fn endpoint_load_state_combined_active_no_metrics() {
        let state = EndpointLoadState {
            assigned_active: 3,
            ..Default::default()
        };
        assert_eq!(state.combined_active(), 3);
    }

    #[test]
    fn endpoint_load_state_combined_active_metrics_higher() {
        let state = EndpointLoadState {
            assigned_active: 2,
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 7,
                total_requests: 0,
                average_response_time_ms: None,
                timestamp: Utc::now(),
            }),
            ..Default::default()
        };
        assert_eq!(state.combined_active(), 7);
    }

    #[test]
    fn endpoint_load_state_combined_active_assigned_higher() {
        let state = EndpointLoadState {
            assigned_active: 10,
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 3,
                total_requests: 0,
                average_response_time_ms: None,
                timestamp: Utc::now(),
            }),
            ..Default::default()
        };
        assert_eq!(state.combined_active(), 10);
    }

    #[test]
    fn endpoint_load_state_average_latency_ms_no_completed() {
        let state = EndpointLoadState::default();
        assert!(state.average_latency_ms().is_none());
    }

    #[test]
    fn endpoint_load_state_average_latency_ms_with_data() {
        let state = EndpointLoadState {
            success_count: 8,
            error_count: 2,
            total_latency_ms: 1000,
            ..Default::default()
        };
        let avg = state.average_latency_ms().unwrap();
        assert!((avg - 100.0).abs() < 0.01, "expected 100.0, got {avg}");
    }

    #[test]
    fn endpoint_load_state_is_stale_no_metrics() {
        let state = EndpointLoadState::default();
        assert!(state.is_stale(Utc::now()));
    }

    #[test]
    fn endpoint_load_state_is_stale_fresh() {
        let now = Utc::now();
        let state = EndpointLoadState {
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                total_requests: 0,
                average_response_time_ms: None,
                timestamp: now,
            }),
            ..Default::default()
        };
        assert!(!state.is_stale(now));
    }

    #[test]
    fn endpoint_load_state_effective_average_ms_no_data() {
        let state = EndpointLoadState::default();
        assert!(state.effective_average_ms().is_none());
    }

    #[test]
    fn endpoint_load_state_effective_average_ms_fallback_to_computed() {
        let now = Utc::now();
        let state = EndpointLoadState {
            success_count: 5,
            error_count: 0,
            total_latency_ms: 500,
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                total_requests: 0,
                average_response_time_ms: None,
                timestamp: now,
            }),
            ..Default::default()
        };
        let avg = state.effective_average_ms().unwrap();
        assert!(
            (avg - 100.0).abs() < 0.01,
            "fallback to computed: expected 100.0, got {avg}"
        );
    }

    #[test]
    fn endpoint_load_state_last_updated_none() {
        let state = EndpointLoadState::default();
        assert!(state.last_updated().is_none());
    }

    #[test]
    fn endpoint_load_state_last_updated_some() {
        let now = Utc::now();
        let state = EndpointLoadState {
            last_metrics: Some(HealthMetrics {
                endpoint_id: Uuid::new_v4(),
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                total_requests: 0,
                average_response_time_ms: None,
                timestamp: now,
            }),
            ..Default::default()
        };
        assert_eq!(state.last_updated(), Some(now));
    }

    // ===== align_to_minute テスト =====

    #[test]
    fn align_to_minute_strips_seconds_and_nanos() {
        let ts = chrono::TimeZone::with_ymd_and_hms(&Utc, 2025, 6, 15, 10, 30, 45).unwrap();
        let aligned = super::align_to_minute(ts);
        assert_eq!(aligned.second(), 0);
        assert_eq!(aligned.nanosecond(), 0);
        assert_eq!(aligned.minute(), 30);
        assert_eq!(aligned.hour(), 10);
    }

    #[test]
    fn align_to_minute_already_aligned() {
        let ts = chrono::TimeZone::with_ymd_and_hms(&Utc, 2025, 1, 1, 0, 0, 0).unwrap();
        let aligned = super::align_to_minute(ts);
        assert_eq!(aligned, ts);
    }

    // ===== prune_history テスト =====

    #[test]
    fn prune_history_removes_old_entries() {
        let now = super::align_to_minute(Utc::now());
        let mut history = std::collections::VecDeque::new();
        // 120分前のエントリ（60分窓より古い）
        let old_minute = now - chrono::Duration::minutes(120);
        history.push_back(RequestHistoryPoint {
            minute: old_minute,
            success: 10,
            error: 1,
        });
        // 30分前のエントリ（窓内）
        let recent_minute = now - chrono::Duration::minutes(30);
        history.push_back(RequestHistoryPoint {
            minute: recent_minute,
            success: 5,
            error: 0,
        });

        super::prune_history(&mut history, now);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].minute, recent_minute);
    }

    #[test]
    fn prune_history_keeps_all_within_window() {
        let now = super::align_to_minute(Utc::now());
        let mut history = std::collections::VecDeque::new();
        for i in 0..5 {
            history.push_back(RequestHistoryPoint {
                minute: now - chrono::Duration::minutes(i),
                success: 1,
                error: 0,
            });
        }
        super::prune_history(&mut history, now);
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn prune_history_empty_is_noop() {
        let now = super::align_to_minute(Utc::now());
        let mut history = std::collections::VecDeque::new();
        super::prune_history(&mut history, now);
        assert!(history.is_empty());
    }

    // ===== new_history_point テスト =====

    #[test]
    fn new_history_point_success() {
        let now = super::align_to_minute(Utc::now());
        let point = super::new_history_point(now, RequestOutcome::Success);
        assert_eq!(point.minute, now);
        assert_eq!(point.success, 1);
        assert_eq!(point.error, 0);
    }

    #[test]
    fn new_history_point_error() {
        let now = super::align_to_minute(Utc::now());
        let point = super::new_history_point(now, RequestOutcome::Error);
        assert_eq!(point.minute, now);
        assert_eq!(point.success, 0);
        assert_eq!(point.error, 1);
    }

    #[test]
    fn new_history_point_queued() {
        let now = super::align_to_minute(Utc::now());
        let point = super::new_history_point(now, RequestOutcome::Queued);
        assert_eq!(point.minute, now);
        assert_eq!(point.success, 0);
        assert_eq!(point.error, 0);
    }

    // ===== increment_history テスト =====

    #[test]
    fn increment_history_success() {
        let mut point = RequestHistoryPoint {
            minute: Utc::now(),
            success: 5,
            error: 2,
        };
        super::increment_history(&mut point, RequestOutcome::Success);
        assert_eq!(point.success, 6);
        assert_eq!(point.error, 2);
    }

    #[test]
    fn increment_history_error() {
        let mut point = RequestHistoryPoint {
            minute: Utc::now(),
            success: 5,
            error: 2,
        };
        super::increment_history(&mut point, RequestOutcome::Error);
        assert_eq!(point.success, 5);
        assert_eq!(point.error, 3);
    }

    #[test]
    fn increment_history_queued_no_change() {
        let mut point = RequestHistoryPoint {
            minute: Utc::now(),
            success: 5,
            error: 2,
        };
        super::increment_history(&mut point, RequestOutcome::Queued);
        assert_eq!(point.success, 5);
        assert_eq!(point.error, 2);
    }

    // ===== compute_round_robin_priority_for_endpoints テスト =====

    #[test]
    fn compute_round_robin_priority_empty() {
        let endpoints: Vec<crate::types::endpoint::Endpoint> = vec![];
        let priority = super::compute_round_robin_priority_for_endpoints(&endpoints, 0);
        assert!(priority.is_empty());
    }

    #[test]
    fn compute_round_robin_priority_single() {
        let ep = Endpoint::new(
            "ep1".to_string(),
            "http://localhost:1".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let id = ep.id;
        let endpoints = vec![ep];
        let priority = super::compute_round_robin_priority_for_endpoints(&endpoints, 0);
        assert_eq!(priority.len(), 1);
        assert_eq!(priority[&id], 0);
    }

    #[test]
    fn compute_round_robin_priority_wraps_around() {
        let ep1 = Endpoint::new(
            "ep1".to_string(),
            "http://localhost:1".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let ep2 = Endpoint::new(
            "ep2".to_string(),
            "http://localhost:2".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let ep3 = Endpoint::new(
            "ep3".to_string(),
            "http://localhost:3".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let id1 = ep1.id;
        let id2 = ep2.id;
        let id3 = ep3.id;
        let endpoints = vec![ep1, ep2, ep3];
        // start_index=1 => priority: ep2=0, ep3=1, ep1=2
        let priority = super::compute_round_robin_priority_for_endpoints(&endpoints, 1);
        assert_eq!(priority[&id2], 0);
        assert_eq!(priority[&id3], 1);
        assert_eq!(priority[&id1], 2);
    }

    // ===== fill_history テスト =====

    #[test]
    fn fill_history_fills_gaps() {
        let now = super::align_to_minute(Utc::now());
        let mut map = std::collections::HashMap::new();
        // 10分前にデータがある
        let ten_ago = now - chrono::Duration::minutes(10);
        map.insert(
            ten_ago,
            RequestHistoryPoint {
                minute: ten_ago,
                success: 42,
                error: 0,
            },
        );

        let result = super::fill_history(now, &mut map);
        // REQUEST_HISTORY_WINDOW_MINUTESは60なので、結果は60エントリ
        assert_eq!(
            result.len(),
            super::types::REQUEST_HISTORY_WINDOW_MINUTES as usize
        );
        // 10分前のエントリには success=42 が含まれる
        let ten_ago_entry = result.iter().find(|p| p.minute == ten_ago);
        assert!(ten_ago_entry.is_some());
        assert_eq!(ten_ago_entry.unwrap().success, 42);
        // 他のエントリは success=0, error=0
        let zero_entries: Vec<_> = result.iter().filter(|p| p.minute != ten_ago).collect();
        for e in zero_entries {
            assert_eq!(e.success, 0);
            assert_eq!(e.error, 0);
        }
    }

    // ===== has_ready_nodes / all_initializing テスト =====

    #[tokio::test]
    async fn has_ready_nodes_empty_state() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        // 初期状態ではstateにエントリなし
        assert!(!load_manager.has_ready_nodes().await);
    }

    #[tokio::test]
    async fn has_ready_nodes_with_ready_endpoint() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .upsert_initial_state(endpoint_id, false, Some((1, 1)))
            .await;
        assert!(load_manager.has_ready_nodes().await);
    }

    #[tokio::test]
    async fn has_ready_nodes_all_initializing() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .upsert_initial_state(endpoint_id, true, Some((0, 1)))
            .await;
        assert!(!load_manager.has_ready_nodes().await);
    }

    #[tokio::test]
    async fn all_initializing_empty() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        // stateが空の場合はfalse
        assert!(!load_manager.all_initializing().await);
    }

    #[tokio::test]
    async fn all_initializing_true_when_all_are() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .upsert_initial_state(endpoint_id, true, Some((0, 1)))
            .await;
        assert!(load_manager.all_initializing().await);
    }

    #[tokio::test]
    async fn all_initializing_false_when_one_ready() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .upsert_initial_state(endpoint_id, false, Some((1, 1)))
            .await;
        assert!(!load_manager.all_initializing().await);
    }

    // ===== queue_waiters テスト =====

    #[tokio::test]
    async fn queue_waiters_starts_at_zero() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        assert_eq!(load_manager.queue_waiters(), 0);
    }

    // ===== endpoint_registry テスト =====

    #[tokio::test]
    async fn endpoint_registry_returns_registry() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let registry = load_manager.endpoint_registry();
        let ep = registry.get(endpoint_id).await;
        assert!(ep.is_some());
    }

    // ===== record_metrics テスト =====

    #[tokio::test]
    async fn record_metrics_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let update = MetricsUpdate {
            endpoint_id: Uuid::new_v4(),
            cpu_usage: 10.0,
            memory_usage: 20.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 0,
            average_response_time_ms: None,
            initializing: false,
            ready_models: None,
        };
        let result = load_manager.record_metrics(update).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn record_metrics_valid_endpoint_succeeds() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let update = MetricsUpdate {
            endpoint_id,
            cpu_usage: 50.0,
            memory_usage: 60.0,
            gpu_usage: Some(70.0),
            gpu_memory_usage: Some(80.0),
            gpu_memory_total_mb: Some(8192),
            gpu_memory_used_mb: Some(4096),
            gpu_temperature: Some(65.0),
            gpu_model_name: Some("NVIDIA RTX 4090".to_string()),
            gpu_compute_capability: Some("8.9".to_string()),
            gpu_capability_score: Some(90),
            active_requests: 2,
            average_response_time_ms: Some(150.0),
            initializing: false,
            ready_models: Some((1, 1)),
        };
        let result = load_manager.record_metrics(update).await;
        assert!(result.is_ok());

        let snapshot = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snapshot.cpu_usage, Some(50.0));
        assert_eq!(snapshot.memory_usage, Some(60.0));
        assert_eq!(snapshot.gpu_usage, Some(70.0));
    }

    // ===== begin_request / finish_request テスト =====

    #[tokio::test]
    async fn begin_request_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager.begin_request(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    // 選択(Online)→割当(begin_request)の間にエンドポイントが Offline/Error へ
    // 遷移する TOCTOU を塞ぐ。begin_request は dead 状態への割当を拒否する。
    #[tokio::test]
    async fn begin_request_offline_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .endpoint_registry()
            .update_status(endpoint_id, EndpointStatus::Offline, None, Some("down"))
            .await
            .expect("update status to offline");
        let result = load_manager.begin_request(endpoint_id).await;
        assert!(
            result.is_err(),
            "begin_request must reject offline endpoints (TOCTOU guard)"
        );
    }

    #[tokio::test]
    async fn begin_request_error_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        load_manager
            .endpoint_registry()
            .update_status(endpoint_id, EndpointStatus::Error, None, Some("boom"))
            .await
            .expect("update status to error");
        let result = load_manager.begin_request(endpoint_id).await;
        assert!(
            result.is_err(),
            "begin_request must reject error-state endpoints (TOCTOU guard)"
        );
    }

    // Pending は従来どおり許可する（begin_request は歴史的に状態を問わなかった。
    // 本番では選択が Online のみ返すため、Pending への直接割当は回帰させない）。
    #[tokio::test]
    async fn begin_request_pending_endpoint_is_allowed() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        // setup の既定状態は Pending
        let result = load_manager.begin_request(endpoint_id).await;
        assert!(
            result.is_ok(),
            "begin_request must still allow pending endpoints"
        );
    }

    #[tokio::test]
    async fn finish_request_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager
            .finish_request(
                Uuid::new_v4(),
                RequestOutcome::Success,
                StdDuration::from_millis(100),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn finish_request_with_tokens_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager
            .finish_request_with_tokens(
                Uuid::new_v4(),
                RequestOutcome::Success,
                StdDuration::from_millis(100),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn finish_request_success_updates_counts() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();
        load_manager
            .finish_request(
                endpoint_id,
                RequestOutcome::Success,
                StdDuration::from_millis(100),
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snap.successful_requests, 1);
    }

    #[tokio::test]
    async fn finish_request_error_updates_counts() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();
        load_manager
            .finish_request(
                endpoint_id,
                RequestOutcome::Error,
                StdDuration::from_millis(100),
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snap.failed_requests, 1);
    }

    #[tokio::test]
    async fn finish_request_queued_does_not_decrement_active() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();
        load_manager
            .finish_request(
                endpoint_id,
                RequestOutcome::Queued,
                StdDuration::from_millis(0),
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        // Queued does not decrement active
        assert_eq!(snap.successful_requests, 0);
        assert_eq!(snap.failed_requests, 0);
    }

    // ===== finish_request_with_tokens テスト =====

    #[tokio::test]
    async fn finish_request_with_tokens_records_token_usage() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();

        let token_usage = Some(crate::token::TokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
        });

        load_manager
            .finish_request_with_tokens(
                endpoint_id,
                RequestOutcome::Success,
                StdDuration::from_millis(200),
                token_usage,
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snap.total_input_tokens, 100);
        assert_eq!(snap.total_output_tokens, 50);
        assert_eq!(snap.total_tokens, 150);
    }

    #[tokio::test]
    async fn finish_request_with_tokens_none_usage() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();

        load_manager
            .finish_request_with_tokens(
                endpoint_id,
                RequestOutcome::Success,
                StdDuration::from_millis(100),
                None,
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snap.total_input_tokens, 0);
        assert_eq!(snap.total_output_tokens, 0);
        assert_eq!(snap.total_tokens, 0);
    }

    #[tokio::test]
    async fn finish_request_with_tokens_partial_usage() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let _lease = load_manager.begin_request(endpoint_id).await.unwrap();

        let token_usage = Some(crate::token::TokenUsage {
            input_tokens: Some(100),
            output_tokens: None,
            total_tokens: None,
        });

        load_manager
            .finish_request_with_tokens(
                endpoint_id,
                RequestOutcome::Success,
                StdDuration::from_millis(100),
                token_usage,
            )
            .await
            .unwrap();

        let snap = load_manager.snapshot(endpoint_id).await.unwrap();
        assert_eq!(snap.total_input_tokens, 100);
        assert_eq!(snap.total_output_tokens, 0);
        // total_tokens is derived: input(100) + output(None) = Some(100)
        assert_eq!(snap.total_tokens, 100);
    }

    // ===== snapshot / snapshots テスト =====

    #[tokio::test]
    async fn snapshot_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager.snapshot(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn snapshots_returns_all_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let snaps = load_manager.snapshots().await;
        assert_eq!(snaps.len(), 1);
    }

    // ===== metrics_history テスト =====

    #[tokio::test]
    async fn metrics_history_unknown_endpoint_returns_error() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager.metrics_history(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn metrics_history_empty_for_no_metrics() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;
        let history = load_manager.metrics_history(endpoint_id).await.unwrap();
        assert!(history.is_empty());
    }

    // ===== summary テスト =====

    #[tokio::test]
    async fn summary_empty_state() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let summary = load_manager.summary().await;
        assert_eq!(summary.total_nodes, 1); // setup creates 1 endpoint
        assert_eq!(summary.total_requests, 0);
        assert_eq!(summary.successful_requests, 0);
        assert_eq!(summary.failed_requests, 0);
    }

    // ===== record_request_history テスト =====

    #[tokio::test]
    async fn record_request_history_creates_entry() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let ts = Utc::now();
        load_manager
            .record_request_history(RequestOutcome::Success, ts)
            .await;
        let history = load_manager.request_history().await;
        let total_success: u64 = history.iter().map(|h| h.success).sum();
        assert_eq!(total_success, 1);
    }

    #[tokio::test]
    async fn record_request_history_same_minute_aggregates() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let ts = Utc::now();
        load_manager
            .record_request_history(RequestOutcome::Success, ts)
            .await;
        load_manager
            .record_request_history(RequestOutcome::Success, ts)
            .await;
        load_manager
            .record_request_history(RequestOutcome::Error, ts)
            .await;

        let history = load_manager.request_history().await;
        let total_success: u64 = history.iter().map(|h| h.success).sum();
        let total_error: u64 = history.iter().map(|h| h.error).sum();
        assert_eq!(total_success, 2);
        assert_eq!(total_error, 1);
    }

    // ===== wait_for_idle_node_with_timeout テスト =====

    #[tokio::test]
    async fn wait_for_idle_node_capacity_exceeded() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager
            .wait_for_idle_node_with_timeout(0, StdDuration::from_millis(100))
            .await;
        assert_eq!(result, WaitResult::CapacityExceeded);
    }

    // ===== wait_for_idle_node_with_timeout_for_model テスト =====

    #[tokio::test]
    async fn wait_for_idle_node_for_model_capacity_exceeded() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, _) = setup_test_load_manager().await;
        let result = load_manager
            .wait_for_idle_node_with_timeout_for_model(
                "test-model",
                0,
                StdDuration::from_millis(100),
            )
            .await;
        assert_eq!(result, WaitResult::CapacityExceeded);
    }

    // ===== seed_tps_from_db api_kind マッピングテスト =====

    #[tokio::test]
    async fn seed_tps_from_db_maps_api_kind_correctly() {
        let _lock = TEST_LOCK.lock().await;
        let (load_manager, endpoint_id) = setup_test_load_manager().await;

        let entries = vec![
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "model-completions".to_string(),
                api_kind: "completions".to_string(),
                total_output_tokens: 100,
                total_duration_ms: 1000,
                successful_requests: 1,
            },
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "model-responses".to_string(),
                api_kind: "responses".to_string(),
                total_output_tokens: 200,
                total_duration_ms: 2000,
                successful_requests: 2,
            },
            crate::db::endpoint_daily_stats::TpsSeedEntry {
                endpoint_id,
                model_id: "model-default".to_string(),
                api_kind: "unknown_type".to_string(),
                total_output_tokens: 50,
                total_duration_ms: 500,
                successful_requests: 1,
            },
        ];

        load_manager.seed_tps_from_db(entries).await;

        let tps = load_manager.get_model_tps(endpoint_id).await;
        assert_eq!(tps.len(), 3);
        assert!(tps
            .iter()
            .any(|t| t.model_id == "model-completions" && t.api_kind == TpsApiKind::Completions));
        assert!(tps
            .iter()
            .any(|t| t.model_id == "model-responses" && t.api_kind == TpsApiKind::Responses));
        assert!(tps
            .iter()
            .any(|t| t.model_id == "model-default" && t.api_kind == TpsApiKind::ChatCompletions));
    }

    // ===== build_history_window テスト =====

    #[test]
    fn build_history_window_returns_full_window() {
        let history = std::collections::VecDeque::new();
        let result = super::build_history_window(&history);
        assert_eq!(
            result.len(),
            super::types::REQUEST_HISTORY_WINDOW_MINUTES as usize
        );
    }
}

/// ロードマネージャー
///
/// # EndpointRegistry統合
///
/// EndpointRegistryを使用してエンドポイント情報を管理します。
#[derive(Clone)]
pub struct LoadManager {
    /// インスタンス固有ID（キャッシュキー用途）
    instance_id: u64,
    /// エンドポイントレジストリ
    endpoint_registry: Arc<EndpointRegistry>,
    state: Arc<RwLock<HashMap<Uuid, EndpointLoadState>>>,
    round_robin: Arc<AtomicUsize>,
    history: Arc<RwLock<VecDeque<RequestHistoryPoint>>>,
    /// ready通知
    ready_notify: Arc<Notify>,
    /// リクエストキュー待機中の通知
    queue_notify: Arc<Notify>,
    /// リクエストキュー待機数
    queue_waiters: Arc<AtomicUsize>,
    /// エンドポイント×モデル単位のTPS状態（SPEC-4bb5b55f）
    tps_tracker: Arc<RwLock<TpsTrackerMap>>,
}

impl LoadManager {
    /// 新しいロードマネージャーを作成
    pub fn new(endpoint_registry: Arc<EndpointRegistry>) -> Self {
        Self {
            instance_id: NEXT_LOAD_MANAGER_ID.fetch_add(1, AtomicOrdering::Relaxed),
            endpoint_registry,
            state: Arc::new(RwLock::new(HashMap::new())),
            round_robin: Arc::new(AtomicUsize::new(0)),
            history: Arc::new(RwLock::new(VecDeque::new())),
            ready_notify: Arc::new(Notify::new()),
            queue_notify: Arc::new(Notify::new()),
            queue_waiters: Arc::new(AtomicUsize::new(0)),
            tps_tracker: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// インスタンス単位のキャッシュキーを返す。
    pub fn cache_key(&self) -> u64 {
        self.instance_id
    }

    async fn compute_endpoint_tps_scores(
        &self,
        endpoints: &[crate::types::endpoint::Endpoint],
        model_id: Option<&str>,
        api_kind: Option<TpsApiKind>,
    ) -> HashMap<Uuid, f64> {
        let tracker = self.tps_tracker.read().await;
        let mut scores = HashMap::with_capacity(endpoints.len());

        for endpoint in endpoints {
            let score = if let Some(model_id) = model_id {
                let Some(api_kind) = api_kind else {
                    scores.insert(endpoint.id, 0.0);
                    continue;
                };

                tracker
                    .iter()
                    .filter(|((eid, mid, kind), _)| {
                        *eid == endpoint.id && mid == model_id && *kind == api_kind
                    })
                    .filter_map(|(_, state)| state.tps_ema)
                    .fold(0.0, f64::max)
            } else {
                let (total_tokens, total_duration_ms) = tracker
                    .iter()
                    .filter(|((eid, _, kind), _)| {
                        *eid == endpoint.id && api_kind.is_none_or(|expected| *kind == expected)
                    })
                    .fold((0u64, 0u64), |(tokens, duration), (_, state)| {
                        (
                            tokens.saturating_add(state.total_output_tokens),
                            duration.saturating_add(state.total_duration_ms),
                        )
                    });

                if total_duration_ms > 0 {
                    total_tokens as f64 / (total_duration_ms as f64 / 1000.0)
                } else {
                    0.0
                }
            };

            scores.insert(endpoint.id, score);
        }

        scores
    }

    async fn select_endpoint_by_tps_from_endpoints(
        &self,
        endpoints: Vec<crate::types::endpoint::Endpoint>,
        model_id: Option<&str>,
        api_kind: Option<TpsApiKind>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        if endpoints.is_empty() {
            return Err(match model_id {
                Some(model_id) => LbError::NoCapableEndpoints(model_id.to_string()),
                None => LbError::NoEndpointsAvailable,
            });
        }

        let candidates: Vec<_> = {
            let state = self.state.read().await;
            endpoints
                .into_iter()
                .filter(|ep| {
                    state
                        .get(&ep.id)
                        .map(|load| !load.initializing)
                        .unwrap_or(true)
                })
                .collect()
        };

        if candidates.is_empty() {
            return Err(LbError::NoEndpointsAvailable);
        }

        let scores = self
            .compute_endpoint_tps_scores(&candidates, model_id, api_kind)
            .await;
        let round_robin_cursor = self.round_robin.fetch_add(1, AtomicOrdering::SeqCst);
        let round_robin_start = round_robin_cursor % candidates.len().max(1);
        let round_robin_priority =
            compute_round_robin_priority_for_endpoints(&candidates, round_robin_start);

        let mut ordered = candidates;
        ordered.sort_by(|a, b| {
            let a_score = scores.get(&a.id).copied().unwrap_or(0.0);
            let b_score = scores.get(&b.id).copied().unwrap_or(0.0);

            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_rank = round_robin_priority
                        .get(&a.id)
                        .copied()
                        .unwrap_or(usize::MAX);
                    let b_rank = round_robin_priority
                        .get(&b.id)
                        .copied()
                        .unwrap_or(usize::MAX);
                    a_rank.cmp(&b_rank)
                })
        });

        Ok(ordered
            .into_iter()
            .next()
            .expect("candidates checked as non-empty"))
    }

    /// テスト用: 指定エンドポイントがアクティブになるまで待機する
    #[cfg(test)]
    pub async fn wait_for_endpoint_active(
        &self,
        endpoint_id: Uuid,
        timeout_duration: StdDuration,
    ) -> bool {
        let start = std::time::Instant::now();
        loop {
            if let Ok(snapshot) = self.snapshot(endpoint_id).await {
                if snapshot.active_requests > 0 {
                    return true;
                }
            }

            if start.elapsed() > timeout_duration {
                return false;
            }

            tokio::time::sleep(StdDuration::from_millis(10)).await;
        }
    }

    /// エンドポイントレジストリへの参照を取得
    pub fn endpoint_registry(&self) -> &Arc<EndpointRegistry> {
        &self.endpoint_registry
    }

    /// ヘルスメトリクスを記録
    pub async fn record_metrics(&self, update: MetricsUpdate) -> RouterResult<()> {
        let MetricsUpdate {
            endpoint_id,
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
            gpu_memory_total_mb,
            gpu_memory_used_mb,
            gpu_temperature,
            gpu_model_name,
            gpu_compute_capability,
            gpu_capability_score,
            active_requests,
            average_response_time_ms,
            initializing,
            ready_models,
        } = update;

        if self.endpoint_registry.get(endpoint_id).await.is_none() {
            return Err(LbError::EndpointNotFound(endpoint_id));
        }

        let _ = self
            .endpoint_registry
            .update_gpu_info(
                endpoint_id,
                None,
                gpu_memory_total_mb.map(|mb| mb * 1024 * 1024),
                gpu_memory_used_mb.map(|mb| mb * 1024 * 1024),
                gpu_capability_score.map(|s| s as f32),
                Some(active_requests),
            )
            .await;

        let mut state = self.state.write().await;
        let entry = state.entry(endpoint_id).or_default();
        let was_active = entry.combined_active() > 0;
        let was_initializing = entry.initializing;

        let derived_average = average_response_time_ms.or_else(|| entry.average_latency_ms());
        let timestamp = Utc::now();
        let metrics = HealthMetrics {
            endpoint_id,
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
            gpu_memory_total_mb,
            gpu_memory_used_mb,
            gpu_temperature,
            gpu_model_name,
            gpu_compute_capability,
            gpu_capability_score,
            active_requests,
            total_requests: entry.total_assigned,
            average_response_time_ms: derived_average,
            timestamp,
        };

        entry.last_metrics = Some(metrics.clone());
        entry.push_metrics(metrics);
        entry.initializing = initializing;
        entry.ready_models = ready_models;
        if !entry.initializing {
            self.ready_notify.notify_waiters();
        }
        if (was_active && entry.combined_active() == 0)
            || (was_initializing && !entry.initializing && entry.combined_active() == 0)
        {
            self.queue_notify.notify_waiters();
        }

        Ok(())
    }

    /// エンドポイント登録時に初期状態を同期
    pub async fn upsert_initial_state(
        &self,
        endpoint_id: Uuid,
        initializing: bool,
        ready_models: Option<(u8, u8)>,
    ) {
        let mut state = self.state.write().await;
        let entry = state.entry(endpoint_id).or_default();
        entry.initializing = initializing;
        entry.ready_models = ready_models;
        if !initializing {
            self.ready_notify.notify_waiters();
            if entry.combined_active() == 0 {
                self.queue_notify.notify_waiters();
            }
        }
    }

    /// 指定されたエンドポイントのロードスナップショットを取得
    pub async fn snapshot(&self, endpoint_id: Uuid) -> RouterResult<EndpointLoadSnapshot> {
        let endpoint = self
            .endpoint_registry
            .get(endpoint_id)
            .await
            .ok_or(LbError::EndpointNotFound(endpoint_id))?;
        let state = self.state.read().await;
        let load_state = state.get(&endpoint_id).cloned().unwrap_or_default();

        Ok(self.build_snapshot_from_endpoint(&endpoint, load_state, Utc::now()))
    }

    /// すべてのエンドポイントのロードスナップショットを取得
    pub async fn snapshots(&self) -> Vec<EndpointLoadSnapshot> {
        let endpoints = self.endpoint_registry.list().await;
        let state = self.state.read().await;

        let now = Utc::now();

        endpoints
            .iter()
            .map(|endpoint| {
                let load_state = state.get(&endpoint.id).cloned().unwrap_or_default();
                self.build_snapshot_from_endpoint(endpoint, load_state, now)
            })
            .collect()
    }

    /// 指定されたエンドポイントのメトリクス履歴を取得
    pub async fn metrics_history(&self, endpoint_id: Uuid) -> RouterResult<Vec<HealthMetrics>> {
        if self.endpoint_registry.get(endpoint_id).await.is_none() {
            return Err(LbError::EndpointNotFound(endpoint_id));
        }
        let state = self.state.read().await;
        let history = state
            .get(&endpoint_id)
            .map(|load_state| load_state.metrics_history.iter().cloned().collect())
            .unwrap_or_else(Vec::new);
        Ok(history)
    }

    /// システム全体の統計サマリーを取得（SPEC-f8e3a1b7: Endpoint版）
    pub async fn summary(&self) -> SystemSummary {
        use crate::types::endpoint::EndpointStatus;

        let endpoints = self.endpoint_registry.list().await;
        let state = self.state.read().await;

        let mut summary = SystemSummary {
            total_nodes: endpoints.len(),
            online_nodes: endpoints
                .iter()
                .filter(|ep| ep.status == EndpointStatus::Online)
                .count(),
            pending_nodes: endpoints
                .iter()
                .filter(|ep| ep.status == EndpointStatus::Pending)
                .count(),
            registering_nodes: 0,
            offline_nodes: endpoints
                .iter()
                .filter(|ep| {
                    ep.status == EndpointStatus::Offline || ep.status == EndpointStatus::Error
                })
                .count(),
            queued_requests: self.queue_waiters.load(AtomicOrdering::Relaxed),
            ..Default::default()
        };

        let mut total_latency_ms = 0u128;
        let mut latency_samples = 0u64;
        let mut weighted_average_sum = 0f64;
        let mut weighted_average_weight = 0f64;
        let mut latest_timestamp: Option<DateTime<Utc>> = None;
        let mut gpu_usage_total = 0f64;
        let mut gpu_usage_samples = 0u64;
        let mut gpu_memory_total = 0f64;
        let mut gpu_memory_samples = 0u64;
        let now = Utc::now();

        for endpoint in &endpoints {
            if let Some(load_state) = state.get(&endpoint.id) {
                let is_fresh = !load_state.is_stale(now);
                if is_fresh {
                    summary.total_active_requests = summary
                        .total_active_requests
                        .saturating_add(load_state.combined_active());
                }
                summary.total_requests = summary
                    .total_requests
                    .saturating_add(load_state.total_assigned);
                summary.successful_requests = summary
                    .successful_requests
                    .saturating_add(load_state.success_count);
                summary.failed_requests = summary
                    .failed_requests
                    .saturating_add(load_state.error_count);

                summary.total_input_tokens = summary
                    .total_input_tokens
                    .saturating_add(load_state.total_input_tokens);
                summary.total_output_tokens = summary
                    .total_output_tokens
                    .saturating_add(load_state.total_output_tokens);
                summary.total_tokens = summary.total_tokens.saturating_add(load_state.total_tokens);

                let completed = load_state.success_count + load_state.error_count;
                if completed > 0 {
                    total_latency_ms = total_latency_ms.saturating_add(load_state.total_latency_ms);
                    latency_samples = latency_samples.saturating_add(completed);
                }

                if is_fresh {
                    if let Some(timestamp) = load_state.last_updated() {
                        if latest_timestamp.is_none_or(|current| timestamp > current) {
                            latest_timestamp = Some(timestamp);
                        }
                    }
                    if let Some(avg) = load_state.effective_average_ms() {
                        let weight = load_state.total_assigned.max(1) as f64;
                        weighted_average_sum += avg as f64 * weight;
                        weighted_average_weight += weight;
                    }
                    if let Some(metrics) = load_state.last_metrics.as_ref() {
                        if let Some(gpu) = metrics.gpu_usage {
                            gpu_usage_total += gpu as f64;
                            gpu_usage_samples = gpu_usage_samples.saturating_add(1);
                        }
                        if let Some(gpu_mem) = metrics.gpu_memory_usage {
                            gpu_memory_total += gpu_mem as f64;
                            gpu_memory_samples = gpu_memory_samples.saturating_add(1);
                        }
                    }
                } else if latest_timestamp.is_none() {
                    if let Some(timestamp) = load_state.last_updated() {
                        latest_timestamp = Some(timestamp);
                    }
                }
            }
        }

        if weighted_average_weight > 0.0 {
            summary.average_response_time_ms =
                Some((weighted_average_sum / weighted_average_weight) as f32);
        } else if latency_samples > 0 {
            summary.average_response_time_ms =
                Some((total_latency_ms as f64 / latency_samples as f64) as f32);
        }

        if gpu_usage_samples > 0 {
            summary.average_gpu_usage = Some((gpu_usage_total / gpu_usage_samples as f64) as f32);
        }
        if gpu_memory_samples > 0 {
            summary.average_gpu_memory_usage =
                Some((gpu_memory_total / gpu_memory_samples as f64) as f32);
        }

        summary.last_metrics_updated_at = latest_timestamp;

        summary
    }

    fn build_snapshot_from_endpoint(
        &self,
        endpoint: &crate::types::endpoint::Endpoint,
        load_state: EndpointLoadState,
        now: DateTime<Utc>,
    ) -> EndpointLoadSnapshot {
        let cpu_usage = load_state
            .last_metrics
            .as_ref()
            .map(|metrics| metrics.cpu_usage);
        let memory_usage = load_state
            .last_metrics
            .as_ref()
            .map(|metrics| metrics.memory_usage);
        let gpu_usage = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_usage);
        let gpu_memory_usage = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_memory_usage);
        let gpu_memory_total_mb = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_memory_total_mb);
        let gpu_memory_used_mb = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_memory_used_mb);
        let gpu_temperature = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_temperature);
        let gpu_model_name = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_model_name.clone());
        let gpu_compute_capability = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_compute_capability.clone());
        let gpu_capability_score = load_state
            .last_metrics
            .as_ref()
            .and_then(|metrics| metrics.gpu_capability_score);
        let active_requests = load_state.combined_active();

        EndpointLoadSnapshot {
            endpoint_id: endpoint.id,
            machine_name: endpoint.name.clone(),
            status: endpoint.status,
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
            gpu_memory_total_mb,
            gpu_memory_used_mb,
            gpu_temperature,
            gpu_model_name,
            gpu_compute_capability,
            gpu_capability_score,
            active_requests,
            total_requests: load_state.total_assigned,
            successful_requests: load_state.success_count,
            failed_requests: load_state.error_count,
            average_response_time_ms: load_state.effective_average_ms(),
            last_updated: load_state.last_updated(),
            is_stale: load_state.is_stale(now),
            total_input_tokens: load_state.total_input_tokens,
            total_output_tokens: load_state.total_output_tokens,
            total_tokens: load_state.total_tokens,
        }
    }
}

fn compute_round_robin_priority_for_endpoints(
    endpoints: &[crate::types::endpoint::Endpoint],
    start_index: usize,
) -> HashMap<Uuid, usize> {
    let len = endpoints.len();
    let mut priority = HashMap::with_capacity(len);
    if len == 0 {
        return priority;
    }

    for offset in 0..len {
        let idx = (start_index + offset) % len;
        priority.insert(endpoints[idx].id, offset);
    }

    priority
}
