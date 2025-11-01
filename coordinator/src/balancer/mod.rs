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
    use std::net::{IpAddr, Ipv4Addr};

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
            })
            .await
            .unwrap()
            .agent_id;

        manager
            .record_metrics(slow_agent, 20.0, 30.0, None, None, 1, Some(240.0))
            .await
            .unwrap();
        manager
            .record_metrics(fast_agent, 20.0, 30.0, None, None, 1, Some(120.0))
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
            })
            .await
            .unwrap()
            .agent_id;

        for i in 0..(METRICS_HISTORY_CAPACITY + 10) {
            manager
                .record_metrics(
                    agent_id,
                    i as f32,
                    (i * 2) as f32,
                    Some((i % 100) as f32),
                    Some(((i * 2) % 100) as f32),
                    1,
                    Some(100.0),
                )
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
}

/// エージェントの最新ロード状態
#[derive(Debug, Clone)]
struct AgentLoadState {
    last_metrics: Option<HealthMetrics>,
    assigned_active: u32,
    total_assigned: u64,
    success_count: u64,
    error_count: u64,
    total_latency_ms: u128,
    metrics_history: VecDeque<HealthMetrics>,
}

impl Default for AgentLoadState {
    fn default() -> Self {
        Self {
            last_metrics: None,
            assigned_active: 0,
            total_assigned: 0,
            success_count: 0,
            error_count: 0,
            total_latency_ms: 0,
            metrics_history: VecDeque::new(),
        }
    }
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
    pub async fn record_metrics(
        &self,
        agent_id: Uuid,
        cpu_usage: f32,
        memory_usage: f32,
        gpu_usage: Option<f32>,
        gpu_memory_usage: Option<f32>,
        active_requests: u32,
        average_response_time_ms: Option<f32>,
    ) -> CoordinatorResult<()> {
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
        let active_requests = load_state.combined_active();

        AgentLoadSnapshot {
            agent_id: agent.id,
            machine_name: agent.machine_name,
            status: agent.status,
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
            active_requests,
            total_requests: load_state.total_assigned,
            successful_requests: load_state.success_count,
            failed_requests: load_state.error_count,
            average_response_time_ms: load_state.effective_average_ms(),
            last_updated: load_state.last_updated(),
            is_stale: load_state.is_stale(now),
        }
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
