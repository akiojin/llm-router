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
mod reporting;
mod selection;
#[cfg(test)]
use selection::compute_round_robin_priority_for_endpoints;
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
#[cfg(test)]
use crate::common::protocol::TpsApiKind;
use crate::registry::endpoints::EndpointRegistry;
use crate::types::HealthMetrics;
use chrono::Utc;
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
mod tests;

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
}
