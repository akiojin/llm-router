//! ダッシュボードAPIハンドラー
//!
//! `/api/dashboard/*` 系のエンドポイントを提供し、エージェントの状態および
//! システム統計を返却する。

use super::agent::AppError;
use crate::{
    balancer::{AgentLoadSnapshot, RequestHistoryPoint},
    AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use ollama_coordinator_common::types::{AgentStatus, GpuDeviceInfo, HealthMetrics};
use serde::Serialize;
use std::{collections::HashMap, time::Instant};
use uuid::Uuid;

/// エージェントのダッシュボード表示用サマリー
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardAgent {
    /// エージェントID
    pub id: Uuid,
    /// マシン名
    pub machine_name: String,
    /// IPアドレス（文字列化）
    pub ip_address: String,
    /// Ollama バージョン
    pub ollama_version: String,
    /// Ollama ポート
    pub ollama_port: u16,
    /// ステータス
    pub status: AgentStatus,
    /// 登録日時
    pub registered_at: DateTime<Utc>,
    /// 最終確認時刻
    pub last_seen: DateTime<Utc>,
    /// 稼働秒数
    pub uptime_seconds: i64,
    /// ロード済みモデル一覧
    #[serde(default)]
    pub loaded_models: Vec<String>,
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
    /// GPUデバイス一覧
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_devices: Vec<GpuDeviceInfo>,
    /// GPU計算能力
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_capability_score: Option<u32>,
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 平均レスポンスタイム
    pub average_response_time_ms: Option<f32>,
    /// メトリクス最終更新時刻
    pub metrics_last_updated_at: Option<DateTime<Utc>>,
    /// メトリクスが古いか
    pub metrics_stale: bool,
    /// GPU利用可能フラグ
    pub gpu_available: Option<bool>,
    /// GPUモデル名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model: Option<String>,
    /// GPU個数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_count: Option<u32>,
}

/// システム統計レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardStats {
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
    /// 処理中リクエスト数
    pub total_active_requests: u32,
    /// 平均レスポンスタイム
    pub average_response_time_ms: Option<f32>,
    /// 平均GPU使用率
    pub average_gpu_usage: Option<f32>,
    /// 平均GPUメモリ使用率
    pub average_gpu_memory_usage: Option<f32>,
    /// 最新メトリクス更新時刻
    pub last_metrics_updated_at: Option<DateTime<Utc>>,
    /// 最新登録日時
    pub last_registered_at: Option<DateTime<Utc>>,
    /// 最新ヘルスチェック時刻
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// ダッシュボード概要レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardOverview {
    /// エージェント一覧
    pub agents: Vec<DashboardAgent>,
    /// システム統計
    pub stats: DashboardStats,
    /// リクエスト履歴
    pub history: Vec<RequestHistoryPoint>,
    /// レスポンス生成時刻
    pub generated_at: DateTime<Utc>,
    /// 集計に要した時間（ミリ秒）
    pub generation_time_ms: u64,
}

/// GET /api/dashboard/agents
pub async fn get_agents(State(state): State<AppState>) -> Json<Vec<DashboardAgent>> {
    Json(collect_agents(&state).await)
}

/// GET /api/dashboard/stats
pub async fn get_stats(State(state): State<AppState>) -> Json<DashboardStats> {
    Json(collect_stats(&state).await)
}

/// GET /api/dashboard/request-history
pub async fn get_request_history(State(state): State<AppState>) -> Json<Vec<RequestHistoryPoint>> {
    Json(collect_history(&state).await)
}

/// GET /api/dashboard/overview
pub async fn get_overview(State(state): State<AppState>) -> Json<DashboardOverview> {
    let started = Instant::now();
    let agents = collect_agents(&state).await;
    let stats = collect_stats(&state).await;
    let history = collect_history(&state).await;
    let generation_time_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let generated_at = Utc::now();
    Json(DashboardOverview {
        agents,
        stats,
        history,
        generated_at,
        generation_time_ms,
    })
}

/// GET /api/dashboard/metrics/:agent_id
pub async fn get_agent_metrics(
    Path(agent_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<HealthMetrics>>, AppError> {
    let history = state.load_manager.metrics_history(agent_id).await?;
    Ok(Json(history))
}

async fn collect_agents(state: &AppState) -> Vec<DashboardAgent> {
    let registry = state.registry.clone();
    let load_manager = state.load_manager.clone();

    let agents = registry.list().await;
    let snapshots = load_manager.snapshots().await;
    let snapshot_map = snapshots
        .into_iter()
        .map(|snapshot| (snapshot.agent_id, snapshot))
        .collect::<HashMap<Uuid, AgentLoadSnapshot>>();

    let now = Utc::now();

    agents
        .into_iter()
        .map(|agent| {
            let uptime_seconds = (now - agent.registered_at).num_seconds().max(0);

            let snapshot = snapshot_map.get(&agent.id);
            let (
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
                total_requests,
                successful_requests,
                failed_requests,
                average_response_time_ms,
                metrics_last_updated_at,
                metrics_stale,
            ) = if let Some(snapshot) = snapshot {
                (
                    snapshot.cpu_usage,
                    snapshot.memory_usage,
                    snapshot.gpu_usage,
                    snapshot.gpu_memory_usage,
                    snapshot.gpu_memory_total_mb,
                    snapshot.gpu_memory_used_mb,
                    snapshot.gpu_temperature,
                    snapshot.gpu_model_name.clone(),
                    snapshot.gpu_compute_capability.clone(),
                    snapshot.gpu_capability_score,
                    snapshot.active_requests,
                    snapshot.total_requests,
                    snapshot.successful_requests,
                    snapshot.failed_requests,
                    snapshot.average_response_time_ms,
                    snapshot.last_updated,
                    snapshot.is_stale,
                )
            } else {
                (
                    None, None, None, None, None, None, None, None, None, None, 0, 0, 0, 0, None,
                    None, true,
                )
            };

            DashboardAgent {
                id: agent.id,
                machine_name: agent.machine_name,
                ip_address: agent.ip_address.to_string(),
                ollama_version: agent.ollama_version,
                ollama_port: agent.ollama_port,
                status: agent.status,
                registered_at: agent.registered_at,
                last_seen: agent.last_seen,
                uptime_seconds,
                loaded_models: agent.loaded_models.clone(),
                cpu_usage,
                memory_usage,
                gpu_usage,
                gpu_memory_usage,
                gpu_memory_total_mb,
                gpu_memory_used_mb,
                gpu_temperature,
                gpu_model_name,
                gpu_devices: agent.gpu_devices.clone(),
                gpu_compute_capability,
                gpu_capability_score,
                active_requests,
                total_requests,
                successful_requests,
                failed_requests,
                average_response_time_ms,
                metrics_last_updated_at,
                metrics_stale,
                gpu_available: Some(agent.gpu_available),
                gpu_model: agent.gpu_model.clone(),
                gpu_count: agent.gpu_count,
            }
        })
        .collect::<Vec<DashboardAgent>>()
}

async fn collect_stats(state: &AppState) -> DashboardStats {
    let load_manager = state.load_manager.clone();
    let registry = state.registry.clone();

    let summary = load_manager.summary().await;
    let agents = registry.list().await;

    let last_registered_at = agents.iter().map(|agent| agent.registered_at).max();
    let last_seen_at = agents.iter().map(|agent| agent.last_seen).max();

    DashboardStats {
        total_agents: summary.total_agents,
        online_agents: summary.online_agents,
        offline_agents: summary.offline_agents,
        total_requests: summary.total_requests,
        successful_requests: summary.successful_requests,
        failed_requests: summary.failed_requests,
        total_active_requests: summary.total_active_requests,
        average_response_time_ms: summary.average_response_time_ms,
        average_gpu_usage: summary.average_gpu_usage,
        average_gpu_memory_usage: summary.average_gpu_memory_usage,
        last_metrics_updated_at: summary.last_metrics_updated_at,
        last_registered_at,
        last_seen_at,
    }
}

async fn collect_history(state: &AppState) -> Vec<RequestHistoryPoint> {
    state.load_manager.request_history().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate, RequestOutcome},
        registry::AgentRegistry,
    };
    use ollama_coordinator_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::time::Duration;

    fn create_state() -> AppState {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        AppState {
            registry,
            load_manager,
        }
    }

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
        }]
    }

    #[tokio::test]
    async fn test_get_agents_returns_joined_state() {
        let state = create_state();

        // エージェントを登録
        let register_req = RegisterRequest {
            machine_name: "agent-01".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ollama_version: "0.1.0".into(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let agent_id = state
            .registry
            .register(register_req)
            .await
            .unwrap()
            .agent_id;

        // メトリクスを記録
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 32.5,
                memory_usage: 48.0,
                gpu_usage: Some(72.0),
                gpu_memory_usage: Some(68.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 2,
                average_response_time_ms: Some(110.0),
            })
            .await
            .unwrap();
        state.load_manager.begin_request(agent_id).await.unwrap();
        state
            .load_manager
            .finish_request(
                agent_id,
                RequestOutcome::Success,
                Duration::from_millis(120),
            )
            .await
            .unwrap();

        let response = get_agents(State(state.clone())).await;
        let body = response.0;

        assert_eq!(body.len(), 1);
        let agent = &body[0];
        assert_eq!(agent.machine_name, "agent-01");
        assert_eq!(agent.status, AgentStatus::Online);
        assert_eq!(agent.ollama_port, 11434);
        assert_eq!(agent.total_requests, 1);
        assert_eq!(agent.successful_requests, 1);
        assert_eq!(agent.failed_requests, 0);
        assert_eq!(agent.average_response_time_ms, Some(120.0));
        assert!(agent.cpu_usage.is_some());
        assert!(agent.memory_usage.is_some());
        assert_eq!(agent.gpu_usage, Some(72.0));
        assert_eq!(agent.gpu_memory_usage, Some(68.0));
    }

    #[tokio::test]
    async fn test_get_stats_summarises_registry_and_metrics() {
        let state = create_state();

        let first_agent = state
            .registry
            .register(RegisterRequest {
                machine_name: "agent-01".into(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        let _second_agent = state
            .registry
            .register(RegisterRequest {
                machine_name: "agent-02".into(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        // 1台分はメトリクスとリクエスト処理を記録
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id: first_agent,
                cpu_usage: 40.0,
                memory_usage: 65.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 3,
                average_response_time_ms: Some(95.0),
            })
            .await
            .unwrap();
        state.load_manager.begin_request(first_agent).await.unwrap();
        state
            .load_manager
            .finish_request(
                first_agent,
                RequestOutcome::Error,
                Duration::from_millis(150),
            )
            .await
            .unwrap();

        let stats = get_stats(State(state)).await.0;
        assert_eq!(stats.total_agents, 2);
        assert_eq!(stats.online_agents, 2);
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.failed_requests, 1);
        assert_eq!(stats.successful_requests, 0);
        assert!(stats.last_registered_at.is_some());
        assert!(stats.last_seen_at.is_some());
        assert!(stats.average_gpu_usage.is_none());
        assert!(stats.average_gpu_memory_usage.is_none());
    }

    #[tokio::test]
    async fn test_get_request_history_returns_series() {
        let state = create_state();

        let agent_id = state
            .registry
            .register(RegisterRequest {
                machine_name: "agent-history".into(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 11)),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        state.load_manager.begin_request(agent_id).await.unwrap();
        state
            .load_manager
            .finish_request(
                agent_id,
                RequestOutcome::Success,
                Duration::from_millis(150),
            )
            .await
            .unwrap();

        state.load_manager.begin_request(agent_id).await.unwrap();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Error, Duration::from_millis(200))
            .await
            .unwrap();

        let history = get_request_history(State(state.clone())).await.0;
        assert_eq!(history.len() as i64, 60);
        let latest = history.last().unwrap();
        assert!(latest.success >= 1);
        assert!(latest.error >= 1);
    }

    #[tokio::test]
    async fn test_get_overview_combines_all_sections() {
        let state = create_state();

        let agent_id = state
            .registry
            .register(RegisterRequest {
                machine_name: "overview".into(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 21)),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        state.load_manager.begin_request(agent_id).await.unwrap();
        state
            .load_manager
            .finish_request(
                agent_id,
                RequestOutcome::Success,
                Duration::from_millis(180),
            )
            .await
            .unwrap();

        let overview = get_overview(State(state)).await.0;
        assert_eq!(overview.agents.len(), 1);
        assert_eq!(overview.stats.total_agents, 1);
        assert_eq!(overview.history.len(), 60);
    }

    #[tokio::test]
    async fn test_get_agent_metrics_returns_history() {
        let state = create_state();

        let response = state
            .registry
            .register(RegisterRequest {
                machine_name: "metrics-agent".into(),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 31)),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap();

        let agent_id = response.agent_id;

        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 24.0,
                memory_usage: 45.0,
                gpu_usage: Some(35.0),
                gpu_memory_usage: Some(40.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: Some(110.0),
            })
            .await
            .unwrap();
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 32.0,
                memory_usage: 40.0,
                gpu_usage: Some(28.0),
                gpu_memory_usage: Some(30.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                average_response_time_ms: Some(95.0),
            })
            .await
            .unwrap();

        let metrics = get_agent_metrics(Path(agent_id), State(state))
            .await
            .unwrap()
            .0;
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].agent_id, agent_id);
        assert!(metrics[1].timestamp >= metrics[0].timestamp);
        assert_eq!(metrics[0].gpu_usage, Some(35.0));
        assert_eq!(metrics[1].gpu_memory_usage, Some(30.0));
    }
}
