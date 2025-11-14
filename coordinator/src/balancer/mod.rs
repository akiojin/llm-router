//! ロードバランサーモジュール
//!
//! エージェントに関する最新メトリクスとリクエスト統計を集約し、
//! 高度なロードバランシング戦略を提供する。

use crate::registry::AgentRegistry;
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use ollama_coordinator_common::{
    error::{CoordinatorError, CoordinatorResult},
    types::{Agent, AgentStatus, HealthMetrics},
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
        Arc,
    },
    time::Duration as StdDuration,
};
use tokio::sync::RwLock;
use uuid::Uuid;

/// メトリクスを新鮮とみなすための許容秒数
const METRICS_STALE_THRESHOLD_SECS: i64 = 120;
/// リクエスト履歴の保持分数
const REQUEST_HISTORY_WINDOW_MINUTES: i64 = 60;
/// エージェントメトリクス履歴の最大保持件数
const METRICS_HISTORY_CAPACITY: usize = 360;

/// リクエスト結果
#[derive(Debug, Clone, Copy)]
pub enum RequestOutcome {
    /// 正常終了
    Success,
    /// エラー終了
    Error,
}

fn compare_average_ms(a: Option<f32>, b: Option<f32>) -> Ordering {
    match (a, b) {
        (Some(ax), Some(bx)) => ax.partial_cmp(&bx).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_coordinator_common::protocol::RegisterRequest;
    use ollama_coordinator_common::types::GpuDeviceInfo;
    use std::net::{IpAddr, Ipv4Addr};

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: None,
        }]
    }

    #[test]
    fn compare_average_ms_orders_values() {
        assert_eq!(compare_average_ms(Some(120.0), Some(180.0)), Ordering::Less);
        assert_eq!(
            compare_average_ms(Some(220.0), Some(180.0)),
            Ordering::Greater
        );
        assert_eq!(compare_average_ms(Some(100.0), None), Ordering::Less);
        assert_eq!(compare_average_ms(None, Some(90.0)), Ordering::Greater);
        assert_eq!(compare_average_ms(None, None), Ordering::Equal);
    }

    #[test]
    fn effective_average_ms_prefers_metrics_value() {
        let timestamp = Utc::now();
        let state = AgentLoadState {
            success_count: 5,
            total_latency_ms: 500,
            last_metrics: Some(HealthMetrics {
                agent_id: Uuid::new_v4(),
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

    #[tokio::test]
    async fn load_manager_prefers_lower_latency_when_active_equal() {
        let registry = AgentRegistry::new();
        let manager = LoadManager::new(registry.clone());

        let slow_agent = registry
            .register(RegisterRequest {
                machine_name: "slow".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        let fast_agent = registry
            .register(RegisterRequest {
                machine_name: "fast".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        manager
            .record_metrics(MetricsUpdate {
                agent_id: slow_agent,
                cpu_usage: 20.0,
                memory_usage: 30.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: Some(240.0),
            })
            .await
            .unwrap();
        manager
            .record_metrics(MetricsUpdate {
                agent_id: fast_agent,
                cpu_usage: 20.0,
                memory_usage: 30.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: Some(120.0),
            })
            .await
            .unwrap();

        let selected = manager.select_agent().await.unwrap();
        assert_eq!(selected.id, fast_agent);
    }

    #[tokio::test]
    async fn metrics_history_tracks_recent_points() {
        let registry = AgentRegistry::new();
        let manager = LoadManager::new(registry.clone());

        let agent_id = registry
            .register(RegisterRequest {
                machine_name: "history".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        for i in 0..(METRICS_HISTORY_CAPACITY + 10) {
            manager
                .record_metrics(MetricsUpdate {
                    agent_id,
                    cpu_usage: i as f32,
                    memory_usage: (i * 2) as f32,
                    gpu_usage: Some((i % 100) as f32),
                    gpu_memory_usage: Some(((i * 2) % 100) as f32),
                    gpu_memory_total_mb: None,
                    gpu_memory_used_mb: None,
                    gpu_temperature: None,
                    gpu_model_name: None,
                    gpu_compute_capability: None,
                    gpu_capability_score: None,
                    active_requests: 1,
                    average_response_time_ms: Some(100.0),
                })
                .await
                .unwrap();
        }

        let history = manager.metrics_history(agent_id).await.unwrap();
        assert_eq!(history.len(), METRICS_HISTORY_CAPACITY);
        let last = history.last().unwrap();
        assert_eq!(last.cpu_usage as usize, METRICS_HISTORY_CAPACITY + 9);
        assert_eq!(
            last.memory_usage as usize,
            (METRICS_HISTORY_CAPACITY + 9) * 2
        );
    }

    #[tokio::test]
    async fn select_agent_by_metrics_prefers_lower_load() {
        let registry = AgentRegistry::new();
        let manager = LoadManager::new(registry.clone());

        // エージェント1: 低負荷
        let low_load_agent = registry
            .register(RegisterRequest {
                machine_name: "low-load".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 10)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        // エージェント2: 高負荷
        let high_load_agent = registry
            .register(RegisterRequest {
                machine_name: "high-load".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 11)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        // 低負荷エージェント: CPU 20%, メモリ 30%, アクティブ 1
        // スコア = 20 + 30 + (1 * 10) = 60
        manager
            .record_metrics(MetricsUpdate {
                agent_id: low_load_agent,
                cpu_usage: 20.0,
                memory_usage: 30.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: Some(100.0),
            })
            .await
            .unwrap();

        // 高負荷エージェント: CPU 70%, メモリ 50%, アクティブ 5
        // スコア = 70 + 50 + (5 * 10) = 170
        manager
            .record_metrics(MetricsUpdate {
                agent_id: high_load_agent,
                cpu_usage: 70.0,
                memory_usage: 50.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 5,
                average_response_time_ms: Some(200.0),
            })
            .await
            .unwrap();

        // 低負荷エージェントが選ばれることを期待
        let selected = manager.select_agent_by_metrics().await.unwrap();
        assert_eq!(selected.id, low_load_agent);
    }

    #[tokio::test]
    async fn select_agent_by_metrics_deprioritizes_agents_without_metrics() {
        let registry = AgentRegistry::new();
        let manager = LoadManager::new(registry.clone());

        // エージェント1: メトリクスあり
        let with_metrics = registry
            .register(RegisterRequest {
                machine_name: "with-metrics".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 20)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        // エージェント2: メトリクスなし
        let _without_metrics = registry
            .register(RegisterRequest {
                machine_name: "without-metrics".to_string(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 21)),
                ollama_version: "0.1.0".to_string(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        // エージェント1にのみメトリクスを記録
        manager
            .record_metrics(MetricsUpdate {
                agent_id: with_metrics,
                cpu_usage: 50.0,
                memory_usage: 40.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 2,
                average_response_time_ms: Some(150.0),
            })
            .await
            .unwrap();

        // メトリクスのあるエージェントが選ばれることを期待
        // （メトリクスなしエージェントはcandidatesに含まれず、ラウンドロビンにフォールバック）
        let selected = manager.select_agent_by_metrics().await.unwrap();
        // メトリクスがある方が優先されるはず
        assert_eq!(selected.id, with_metrics);
    }
}

/// エージェントの最新ロード状態
#[derive(Debug, Clone, Default)]
struct AgentLoadState {
    last_metrics: Option<HealthMetrics>,
    assigned_active: u32,
    total_assigned: u64,
    success_count: u64,
    error_count: u64,
    total_latency_ms: u128,
    metrics_history: VecDeque<HealthMetrics>,
}

impl AgentLoadState {
    fn combined_active(&self) -> u32 {
        let heartbeat_active = self
            .last_metrics
            .as_ref()
            .map(|m| m.active_requests)
            .unwrap_or(0);
        heartbeat_active.saturating_add(self.assigned_active)
    }

    fn average_latency_ms(&self) -> Option<f32> {
        let completed = self.success_count + self.error_count;
        if completed == 0 {
            None
        } else {
            Some((self.total_latency_ms as f64 / completed as f64) as f32)
        }
    }

    fn last_updated(&self) -> Option<DateTime<Utc>> {
        self.last_metrics.as_ref().map(|m| m.timestamp)
    }

    fn is_stale(&self, now: DateTime<Utc>) -> bool {
        match self.last_updated() {
            Some(ts) => (now - ts).num_seconds() > METRICS_STALE_THRESHOLD_SECS,
            None => true,
        }
    }

    fn effective_average_ms(&self) -> Option<f32> {
        self.last_metrics
            .as_ref()
            .and_then(|m| m.average_response_time_ms)
            .or_else(|| self.average_latency_ms())
    }

    fn push_metrics(&mut self, metrics: HealthMetrics) {
        self.metrics_history.push_back(metrics);
        if self.metrics_history.len() > METRICS_HISTORY_CAPACITY {
            self.metrics_history.pop_front();
        }
    }
}

/// エージェントのロードスナップショット
#[derive(Debug, Clone, Serialize)]
pub struct AgentLoadSnapshot {
    /// エージェントID
    pub agent_id: Uuid,
    /// マシン名
    pub machine_name: String,
    /// エージェント状態
    pub status: AgentStatus,
    /// CPU使用率
    pub cpu_usage: Option<f32>,
    /// メモリ使用率
    pub memory_usage: Option<f32>,
    /// GPU使用率
    pub gpu_usage: Option<f32>,
    /// GPUメモリ使用率
    pub gpu_memory_usage: Option<f32>,
    /// GPUメモリ総容量 (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU使用メモリ (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU温度 (℃)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_temperature: Option<f32>,
    /// GPUモデル名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model_name: Option<String>,
    /// GPU計算能力
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_capability_score: Option<u32>,
    /// 処理中リクエスト数（Coordinator観点+エージェント自己申告）
    pub active_requests: u32,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 平均レスポンスタイム (ms)
    pub average_response_time_ms: Option<f32>,
    /// メトリクス最終更新時刻
    pub last_updated: Option<DateTime<Utc>>,
    /// メトリクスが鮮度閾値を超えているか
    pub is_stale: bool,
}

/// システム全体の統計サマリー
#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemSummary {
    /// 登録エージェント総数
    pub total_agents: usize,
    /// オンラインエージェント数
    pub online_agents: usize,
    /// オフラインエージェント数
    pub offline_agents: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 平均レスポンスタイム (ms)
    pub average_response_time_ms: Option<f32>,
    /// 平均GPU使用率 (0-100)
    pub average_gpu_usage: Option<f32>,
    /// 平均GPUメモリ使用率 (0-100)
    pub average_gpu_memory_usage: Option<f32>,
    /// 処理中リクエスト総数
    pub total_active_requests: u32,
    /// 最新メトリクス更新時刻
    pub last_metrics_updated_at: Option<DateTime<Utc>>,
}

/// ロードマネージャー
#[derive(Clone)]
pub struct LoadManager {
    registry: AgentRegistry,
    state: Arc<RwLock<HashMap<Uuid, AgentLoadState>>>,
    round_robin: Arc<AtomicUsize>,
    history: Arc<RwLock<VecDeque<RequestHistoryPoint>>>,
}

/// ハートビートから記録するメトリクス値
#[derive(Debug, Clone)]
pub struct MetricsUpdate {
    /// 対象エージェントのID
    pub agent_id: Uuid,
    /// CPU使用率（パーセンテージ）
    pub cpu_usage: f32,
    /// メモリ使用率（パーセンテージ）
    pub memory_usage: f32,
    /// GPU使用率（パーセンテージ）
    pub gpu_usage: Option<f32>,
    /// GPUメモリ使用率（パーセンテージ）
    pub gpu_memory_usage: Option<f32>,
    /// GPUメモリ総容量 (MB)
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU使用メモリ (MB)
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU温度 (℃)
    pub gpu_temperature: Option<f32>,
    /// GPUモデル名
    pub gpu_model_name: Option<String>,
    /// GPU計算能力
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    pub gpu_capability_score: Option<u32>,
    /// アクティブなリクエスト数
    pub active_requests: u32,
    /// 平均レスポンスタイム（ミリ秒）
    pub average_response_time_ms: Option<f32>,
}

impl LoadManager {
    /// 新しいロードマネージャーを作成
    pub fn new(registry: AgentRegistry) -> Self {
        Self {
            registry,
            state: Arc::new(RwLock::new(HashMap::new())),
            round_robin: Arc::new(AtomicUsize::new(0)),
            history: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// ヘルスメトリクスを記録
    pub async fn record_metrics(&self, update: MetricsUpdate) -> CoordinatorResult<()> {
        let MetricsUpdate {
            agent_id,
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
        } = update;

        // エージェントが存在することを確認
        self.registry.get(agent_id).await?;

        let mut state = self.state.write().await;
        let entry = state.entry(agent_id).or_default();

        let derived_average = average_response_time_ms.or_else(|| entry.average_latency_ms());
        let timestamp = Utc::now();
        let metrics = HealthMetrics {
            agent_id,
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

        Ok(())
    }

    /// リクエスト開始を記録
    pub async fn begin_request(&self, agent_id: Uuid) -> CoordinatorResult<()> {
        self.registry.get(agent_id).await?;

        let mut state = self.state.write().await;
        let entry = state.entry(agent_id).or_default();
        entry.assigned_active = entry.assigned_active.saturating_add(1);
        entry.total_assigned = entry.total_assigned.saturating_add(1);

        Ok(())
    }

    /// リクエスト完了を記録
    pub async fn finish_request(
        &self,
        agent_id: Uuid,
        outcome: RequestOutcome,
        duration: StdDuration,
    ) -> CoordinatorResult<()> {
        self.registry.get(agent_id).await?;

        let mut state = self.state.write().await;
        let entry = state.entry(agent_id).or_default();

        if entry.assigned_active > 0 {
            entry.assigned_active -= 1;
        }

        match outcome {
            RequestOutcome::Success => entry.success_count = entry.success_count.saturating_add(1),
            RequestOutcome::Error => entry.error_count = entry.error_count.saturating_add(1),
        }

        entry.total_latency_ms = entry.total_latency_ms.saturating_add(duration.as_millis());

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

        drop(state);
        self.record_request_history(outcome, Utc::now()).await;

        Ok(())
    }

    /// 適切なエージェントを選択
    pub async fn select_agent(&self) -> CoordinatorResult<Agent> {
        let agents = self.registry.list().await;

        let online_agents: Vec<_> = agents
            .into_iter()
            .filter(|agent| agent.status == AgentStatus::Online)
            .collect();

        if online_agents.is_empty() {
            return Err(CoordinatorError::NoAgentsAvailable);
        }

        let state = self.state.read().await;
        let now = Utc::now();

        let mut load_based_candidates: Vec<(Agent, AgentLoadState)> = Vec::new();
        for agent in &online_agents {
            if let Some(load_state) = state.get(&agent.id) {
                if let Some(metrics) = &load_state.last_metrics {
                    if !load_state.is_stale(now) && metrics.cpu_usage <= 80.0 {
                        load_based_candidates.push((agent.clone(), load_state.clone()));
                    }
                }
            }
        }

        if !load_based_candidates.is_empty() {
            load_based_candidates.sort_by(|a, b| {
                let a_active = a.1.combined_active();
                let b_active = b.1.combined_active();
                let a_avg = a.1.effective_average_ms();
                let b_avg = b.1.effective_average_ms();
                a_active
                    .cmp(&b_active)
                    .then_with(|| compare_average_ms(a_avg, b_avg))
                    .then_with(|| a.1.total_assigned.cmp(&b.1.total_assigned))
                    .then_with(|| a.0.machine_name.cmp(&b.0.machine_name))
            });

            return Ok(load_based_candidates[0].0.clone());
        }

        // すべてのエージェントが高負荷またはメトリクスなし → ラウンドロビン
        let next_index = self
            .round_robin
            .fetch_add(1, AtomicOrdering::SeqCst)
            .rem_euclid(online_agents.len());
        Ok(online_agents[next_index].clone())
    }

    /// 指定されたエージェントのロードスナップショットを取得
    pub async fn snapshot(&self, agent_id: Uuid) -> CoordinatorResult<AgentLoadSnapshot> {
        let agent = self.registry.get(agent_id).await?;
        let state = self.state.read().await;
        let load_state = state.get(&agent_id).cloned().unwrap_or_default();

        Ok(self.build_snapshot(agent, load_state, Utc::now()))
    }

    /// すべてのエージェントのロードスナップショットを取得
    pub async fn snapshots(&self) -> Vec<AgentLoadSnapshot> {
        let agents = self.registry.list().await;
        let state = self.state.read().await;

        let now = Utc::now();

        agents
            .into_iter()
            .map(|agent| {
                let load_state = state.get(&agent.id).cloned().unwrap_or_default();
                self.build_snapshot(agent, load_state, now)
            })
            .collect()
    }

    /// 指定されたエージェントのメトリクス履歴を取得
    pub async fn metrics_history(&self, agent_id: Uuid) -> CoordinatorResult<Vec<HealthMetrics>> {
        self.registry.get(agent_id).await?;
        let state = self.state.read().await;
        let history = state
            .get(&agent_id)
            .map(|load_state| load_state.metrics_history.iter().cloned().collect())
            .unwrap_or_else(Vec::new);
        Ok(history)
    }

    /// システム全体の統計サマリーを取得
    pub async fn summary(&self) -> SystemSummary {
        let agents = self.registry.list().await;
        let state = self.state.read().await;

        let mut summary = SystemSummary {
            total_agents: agents.len(),
            online_agents: agents
                .iter()
                .filter(|agent| agent.status == AgentStatus::Online)
                .count(),
            offline_agents: agents
                .iter()
                .filter(|agent| agent.status == AgentStatus::Offline)
                .count(),
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

        for agent in &agents {
            if let Some(load_state) = state.get(&agent.id) {
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
                    // フレッシュなメトリクスがない場合でも最も新しい値を保持
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

    /// リクエスト履歴を取得
    pub async fn request_history(&self) -> Vec<RequestHistoryPoint> {
        let history = self.history.read().await;
        build_history_window(&history)
    }

    async fn record_request_history(&self, outcome: RequestOutcome, timestamp: DateTime<Utc>) {
        let minute = align_to_minute(timestamp);
        let mut history = self.history.write().await;

        if let Some(last) = history.back_mut() {
            if last.minute == minute {
                increment_history(last, outcome);
            } else {
                history.push_back(new_history_point(minute, outcome));
            }
        } else {
            history.push_back(new_history_point(minute, outcome));
        }

        prune_history(&mut history, minute);
    }

    fn build_snapshot(
        &self,
        agent: Agent,
        load_state: AgentLoadState,
        now: DateTime<Utc>,
    ) -> AgentLoadSnapshot {
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

        AgentLoadSnapshot {
            agent_id: agent.id,
            machine_name: agent.machine_name,
            status: agent.status,
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
        }
    }

    /// メトリクスベースのエージェント選択
    ///
    /// エージェントの最新メトリクス（CPU使用率、メモリ使用率、アクティブリクエスト数）を基に
    /// 負荷スコアを計算し、最も低いスコアのエージェントを選択します。
    ///
    /// # 負荷スコア計算式
    ///
    /// ```text
    /// score = cpu_usage + memory_usage + (active_requests × 10)
    /// ```
    ///
    /// - `cpu_usage`: CPU使用率（0.0～100.0）
    /// - `memory_usage`: メモリ使用率（0.0～100.0）
    /// - `active_requests`: アクティブリクエスト数（重み付け：×10）
    ///
    /// # フォールバック戦略
    ///
    /// 以下のいずれかの条件に該当する場合、ラウンドロビン選択にフォールバックします：
    ///
    /// - すべてのエージェントのCPU使用率が80%を超えている
    /// - メトリクスを持つエージェントが存在しない
    /// - すべてのメトリクスが古い（120秒以上前）
    ///
    /// # 戻り値
    ///
    /// - `Ok(Agent)`: 選択されたエージェント
    /// - `Err(CoordinatorError::NoAgentsAvailable)`: オンラインエージェントが存在しない
    ///
    /// # 例
    ///
    /// ```ignore
    /// let manager = LoadManager::new(registry);
    /// let agent = manager.select_agent_by_metrics().await?;
    /// println!("Selected agent: {}", agent.machine_name);
    /// ```
    pub async fn select_agent_by_metrics(&self) -> CoordinatorResult<Agent> {
        let agents = self.registry.list().await;

        let online_agents: Vec<_> = agents
            .into_iter()
            .filter(|agent| agent.status == AgentStatus::Online)
            .collect();

        if online_agents.is_empty() {
            return Err(CoordinatorError::NoAgentsAvailable);
        }

        let state = self.state.read().await;
        let now = Utc::now();

        // メトリクスを持つエージェントの負荷スコアを計算
        let mut candidates: Vec<(Agent, f64)> = Vec::new();

        for agent in &online_agents {
            if let Some(load_state) = state.get(&agent.id) {
                if let Some(metrics) = &load_state.last_metrics {
                    if !load_state.is_stale(now) {
                        // 負荷スコア = cpu_usage + memory_usage + (active_requests * 10)
                        let score = metrics.cpu_usage as f64
                            + metrics.memory_usage as f64
                            + (load_state.combined_active() as f64 * 10.0);
                        candidates.push((agent.clone(), score));
                    }
                }
            }
        }

        // すべてのエージェントがCPU > 80%かチェック
        let all_high_load = !candidates.is_empty()
            && candidates.iter().all(|(agent, _)| {
                if let Some(load_state) = state.get(&agent.id) {
                    if let Some(metrics) = &load_state.last_metrics {
                        return metrics.cpu_usage > 80.0;
                    }
                }
                false
            });

        if all_high_load || candidates.is_empty() {
            // フォールバック: ラウンドロビン
            let next_index = self
                .round_robin
                .fetch_add(1, AtomicOrdering::SeqCst)
                .rem_euclid(online_agents.len());
            return Ok(online_agents[next_index].clone());
        }

        // 最小スコアのエージェントを選択
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(candidates[0].0.clone())
    }
}

fn align_to_minute(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.with_second(0).unwrap().with_nanosecond(0).unwrap()
}

fn prune_history(history: &mut VecDeque<RequestHistoryPoint>, newest: DateTime<Utc>) {
    let cutoff = newest - ChronoDuration::minutes(REQUEST_HISTORY_WINDOW_MINUTES - 1);
    while let Some(front) = history.front() {
        if front.minute < cutoff {
            history.pop_front();
        } else {
            break;
        }
    }
}

fn new_history_point(minute: DateTime<Utc>, outcome: RequestOutcome) -> RequestHistoryPoint {
    let mut point = RequestHistoryPoint {
        minute,
        success: 0,
        error: 0,
    };
    increment_history(&mut point, outcome);
    point
}

fn increment_history(point: &mut RequestHistoryPoint, outcome: RequestOutcome) {
    match outcome {
        RequestOutcome::Success => point.success = point.success.saturating_add(1),
        RequestOutcome::Error => point.error = point.error.saturating_add(1),
    }
}

fn build_history_window(history: &VecDeque<RequestHistoryPoint>) -> Vec<RequestHistoryPoint> {
    let now = align_to_minute(Utc::now());
    let mut map: HashMap<DateTime<Utc>, RequestHistoryPoint> = history
        .iter()
        .cloned()
        .map(|point| (point.minute, point))
        .collect();
    fill_history(now, &mut map)
}

fn fill_history(
    now: DateTime<Utc>,
    map: &mut HashMap<DateTime<Utc>, RequestHistoryPoint>,
) -> Vec<RequestHistoryPoint> {
    let start = now - ChronoDuration::minutes(REQUEST_HISTORY_WINDOW_MINUTES - 1);
    let mut cursor = start;
    let mut result = Vec::with_capacity(REQUEST_HISTORY_WINDOW_MINUTES as usize);

    while cursor <= now {
        if let Some(point) = map.remove(&cursor) {
            result.push(point);
        } else {
            result.push(RequestHistoryPoint {
                minute: cursor,
                success: 0,
                error: 0,
            });
        }
        cursor += ChronoDuration::minutes(1);
    }

    result
}

/// リクエスト履歴ポイント
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct RequestHistoryPoint {
    /// 分単位のタイムスタンプ
    pub minute: DateTime<Utc>,
    /// 成功数
    pub success: u64,
    /// 失敗数
    pub error: u64,
}
