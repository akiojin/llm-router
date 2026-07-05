//! LoadManager の外部向け読み取り投影（スナップショット/メトリクス履歴/サマリ）
//!
//! arch-review [H6]: balancer/mod.rs の LoadManager god-object から、内部ロード
//! 状態を外部 DTO へ投影する読み取り専用メソッド群を submodule へ分離。
//! state/endpoint_registry/queue_waiters のロックに閉じた処理で公開 API は不変。

use super::types::{EndpointLoadSnapshot, EndpointLoadState, SystemSummary};
use super::LoadManager;
use crate::common::error::{LbError, RouterResult};
use crate::types::HealthMetrics;
use chrono::{DateTime, Utc};
use std::sync::atomic::Ordering as AtomicOrdering;
use uuid::Uuid;

impl LoadManager {
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
