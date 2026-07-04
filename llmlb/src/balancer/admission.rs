//! LoadManager の受付制御・リクエストライフサイクル
//!
//! arch-review [H1]: LoadManager God object から、ノード可用性判定・アイドル待機・
//! リクエストの開始/完了（begin_request/finish_request/finish_request_with_tokens）
//! といった受付制御とライフサイクル管理を submodule へ切り出した。
//! state/queue_notify/queue_waiters 等のロックに閉じた処理で、公開 API は不変。

use super::types::QueueWaiterGuard;
use super::{LoadManager, RequestLease, RequestOutcome, WaitResult};
use crate::common::error::{LbError, RouterResult};
use chrono::Utc;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration as StdDuration;
use uuid::Uuid;

impl LoadManager {
    /// 初期化完了しているノードが存在するか
    pub async fn has_ready_nodes(&self) -> bool {
        let state = self.state.read().await;
        state.values().any(|s| !s.initializing)
    }

    /// 全ノードが初期化中かを判定
    pub async fn all_initializing(&self) -> bool {
        let state = self.state.read().await;
        !state.is_empty() && state.values().all(|s| s.initializing)
    }

    /// リクエストキュー待機数を取得
    pub fn queue_waiters(&self) -> usize {
        self.queue_waiters.load(AtomicOrdering::Relaxed)
    }

    async fn has_idle_nodes(&self) -> bool {
        let endpoints = self.endpoint_registry.list_online().await;
        if endpoints.is_empty() {
            return false;
        }

        let state = self.state.read().await;
        endpoints.iter().any(|endpoint| {
            let load = state.get(&endpoint.id);
            let is_not_initializing = load.map(|l| !l.initializing).unwrap_or(true);
            let is_idle = load.map(|l| l.combined_active() == 0).unwrap_or(true);
            is_not_initializing && is_idle
        })
    }

    async fn has_idle_nodes_for_model(&self, model_id: &str) -> bool {
        let endpoints = self.endpoint_registry.find_by_model(model_id).await;
        if endpoints.is_empty() {
            return false;
        }

        let state = self.state.read().await;
        endpoints.iter().any(|endpoint| {
            let load = state.get(&endpoint.id);
            let is_not_initializing = load.map(|l| !l.initializing).unwrap_or(true);
            let is_idle = load.map(|l| l.combined_active() == 0).unwrap_or(true);
            is_not_initializing && is_idle
        })
    }

    /// タイムアウト付きでアイドルノード待機
    pub async fn wait_for_idle_node_with_timeout(
        &self,
        max_waiters: usize,
        timeout_duration: StdDuration,
    ) -> WaitResult {
        let current = self.queue_waiters.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        if current > max_waiters {
            self.queue_waiters.fetch_sub(1, AtomicOrdering::SeqCst);
            return WaitResult::CapacityExceeded;
        }

        let _guard = QueueWaiterGuard::new(self.queue_waiters.clone());

        if self.has_idle_nodes().await {
            return WaitResult::Ready;
        }

        let result = tokio::time::timeout(timeout_duration, self.queue_notify.notified()).await;

        match result {
            Ok(_) => WaitResult::Ready,
            Err(_) => WaitResult::Timeout,
        }
    }

    /// タイムアウト付きでモデル対応のアイドルノード待機
    pub async fn wait_for_idle_node_with_timeout_for_model(
        &self,
        model_id: &str,
        max_waiters: usize,
        timeout_duration: StdDuration,
    ) -> WaitResult {
        let current = self.queue_waiters.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        if current > max_waiters {
            self.queue_waiters.fetch_sub(1, AtomicOrdering::SeqCst);
            return WaitResult::CapacityExceeded;
        }

        let _guard = QueueWaiterGuard::new(self.queue_waiters.clone());

        if self.has_idle_nodes_for_model(model_id).await {
            return WaitResult::Ready;
        }

        let result = tokio::time::timeout(timeout_duration, self.queue_notify.notified()).await;

        match result {
            Ok(_) => WaitResult::Ready,
            Err(_) => WaitResult::Timeout,
        }
    }

    /// リクエスト開始を記録
    ///
    /// 選択(find_by_model は Online のみ返す)から本メソッド呼び出しまでの間に
    /// エンドポイントが Offline/Error へ遷移する TOCTOU を塞ぐため、dead 状態
    /// (Offline/Error)への割当を拒否する。Pending は従来どおり許可する。
    pub async fn begin_request(&self, endpoint_id: Uuid) -> RouterResult<RequestLease> {
        let Some(endpoint) = self.endpoint_registry.get(endpoint_id).await else {
            return Err(LbError::EndpointNotFound(endpoint_id));
        };
        if endpoint.status == crate::types::endpoint::EndpointStatus::Offline
            || endpoint.status == crate::types::endpoint::EndpointStatus::Error
        {
            return Err(LbError::EndpointOffline(endpoint_id));
        }

        let mut state = self.state.write().await;
        let entry = state.entry(endpoint_id).or_default();
        entry.assigned_active = entry.assigned_active.saturating_add(1);
        entry.total_assigned = entry.total_assigned.saturating_add(1);

        Ok(RequestLease::new(self.clone(), endpoint_id))
    }

    /// リクエスト完了を記録
    pub async fn finish_request(
        &self,
        endpoint_id: Uuid,
        outcome: RequestOutcome,
        duration: StdDuration,
    ) -> RouterResult<()> {
        // token_usage=None のとき finish_request_with_tokens と挙動が完全一致するため委譲。
        self.finish_request_with_tokens(endpoint_id, outcome, duration, None)
            .await
    }

    /// リクエスト完了を記録（トークン使用量含む）
    pub async fn finish_request_with_tokens(
        &self,
        endpoint_id: Uuid,
        outcome: RequestOutcome,
        duration: StdDuration,
        token_usage: Option<crate::token::TokenUsage>,
    ) -> RouterResult<()> {
        if self.endpoint_registry.get(endpoint_id).await.is_none() {
            return Err(LbError::EndpointNotFound(endpoint_id));
        }

        let mut state = self.state.write().await;
        let entry = state.entry(endpoint_id).or_default();

        if let RequestOutcome::Queued = outcome {
        } else {
            if entry.assigned_active > 0 {
                entry.assigned_active -= 1;
            }

            match outcome {
                RequestOutcome::Success => {
                    entry.success_count = entry.success_count.saturating_add(1)
                }
                RequestOutcome::Error => entry.error_count = entry.error_count.saturating_add(1),
                RequestOutcome::Queued => {}
            }

            entry.total_latency_ms = entry.total_latency_ms.saturating_add(duration.as_millis());

            if let Some(ref usage) = token_usage {
                if let Some(input) = usage.input_tokens {
                    entry.total_input_tokens =
                        entry.total_input_tokens.saturating_add(input as u64);
                }
                if let Some(output) = usage.output_tokens {
                    entry.total_output_tokens =
                        entry.total_output_tokens.saturating_add(output as u64);
                }
                let total = usage.total_tokens.or_else(|| {
                    match (usage.input_tokens, usage.output_tokens) {
                        (Some(i), Some(o)) => Some(i + o),
                        (Some(i), None) => Some(i),
                        (None, Some(o)) => Some(o),
                        (None, None) => None,
                    }
                });
                if let Some(t) = total {
                    entry.total_tokens = entry.total_tokens.saturating_add(t as u64);
                }
            }
        }

        let updated_average = entry.average_latency_ms();

        if let Some(metrics) = entry.last_metrics.as_mut() {
            metrics.total_requests = entry.total_assigned;
            if updated_average.is_some() {
                metrics.average_response_time_ms = updated_average;
            }
            if let Some(latest) = entry.metrics_history.back_mut() {
                latest.total_requests = metrics.total_requests;
                if let Some(avg) = metrics.average_response_time_ms {
                    latest.average_response_time_ms = Some(avg);
                }
                latest.gpu_usage = metrics.gpu_usage;
                latest.gpu_memory_usage = metrics.gpu_memory_usage;
            }
        }

        let should_notify_idle = entry.combined_active() == 0;

        drop(state);
        if should_notify_idle {
            self.queue_notify.notify_waiters();
        }
        self.record_request_history(outcome, Utc::now()).await;

        Ok(())
    }
}
