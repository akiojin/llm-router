//! エージェント登録APIハンドラー

use crate::{
    balancer::{AgentLoadSnapshot, SystemSummary},
    registry::AgentSettingsUpdate,
    AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use ollama_coordinator_common::{
    error::CoordinatorError,
    protocol::{RegisterRequest, RegisterResponse},
    types::Agent,
};
use serde::Deserialize;

/// POST /api/agents - エージェント登録
pub async fn register_agent(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    let response = state.registry.register(req).await?;
    Ok(Json(response))
}

/// GET /api/agents - エージェント一覧取得
pub async fn list_agents(State(state): State<AppState>) -> Json<Vec<Agent>> {
    let agents = state.registry.list().await;
    Json(agents)
}

/// PUT /api/agents/:id/settings - エージェント設定更新
pub async fn update_agent_settings(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<uuid::Uuid>,
    Json(payload): Json<UpdateAgentSettingsPayload>,
) -> Result<Json<Agent>, AppError> {
    let update = AgentSettingsUpdate {
        custom_name: payload.custom_name,
        tags: payload.tags,
        notes: payload.notes,
    };

    let agent = state.registry.update_settings(agent_id, update).await?;

    Ok(Json(agent))
}

/// エージェント設定更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateAgentSettingsPayload {
    /// 表示名（nullでリセット）
    #[serde(default)]
    pub custom_name: Option<Option<String>>,
    /// タグ一覧
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// メモ（nullでリセット）
    #[serde(default)]
    pub notes: Option<Option<String>>,
}

/// GET /api/agents/metrics - エージェントメトリクス取得
pub async fn list_agent_metrics(State(state): State<AppState>) -> Json<Vec<AgentLoadSnapshot>> {
    let snapshots = state.load_manager.snapshots().await;
    Json(snapshots)
}

/// GET /api/metrics/summary - システム統計
pub async fn metrics_summary(State(state): State<AppState>) -> Json<SystemSummary> {
    let summary = state.load_manager.summary().await;
    Json(summary)
}

/// DELETE /api/agents/:id - エージェントを削除
pub async fn delete_agent(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, AppError> {
    state.registry.delete(agent_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/agents/:id/disconnect - エージェントを強制オフラインにする
pub async fn disconnect_agent(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, AppError> {
    state.registry.mark_offline(agent_id).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Axum用のエラーレスポンス型
#[derive(Debug)]
pub struct AppError(CoordinatorError);

impl From<CoordinatorError> for AppError {
    fn from(err: CoordinatorError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self.0 {
            CoordinatorError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            CoordinatorError::NoAgentsAvailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string())
            }
            CoordinatorError::Database(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string())
            }
            CoordinatorError::Http(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            CoordinatorError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            CoordinatorError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string())
            }
            CoordinatorError::Common(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate, RequestOutcome},
        registry::AgentRegistry,
    };
    use ollama_coordinator_common::{protocol::RegisterStatus, types::AgentStatus};
    use std::net::IpAddr;
    use std::time::Duration;

    fn create_test_state() -> AppState {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        AppState {
            registry,
            load_manager,
        }
    }

    #[tokio::test]
    async fn test_register_agent_success() {
        let state = create_test_state();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let result = register_agent(State(state), Json(req)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert!(!response.agent_id.to_string().is_empty());
    }

    #[tokio::test]
    async fn test_list_agents_empty() {
        let state = create_test_state();
        let result = list_agents(State(state)).await;
        assert_eq!(result.0.len(), 0);
    }

    #[tokio::test]
    async fn test_list_agents_with_agents() {
        let state = create_test_state();

        // エージェントを2つ登録
        let req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let _ = register_agent(State(state.clone()), Json(req1))
            .await
            .unwrap();

        let req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let _ = register_agent(State(state.clone()), Json(req2))
            .await
            .unwrap();

        let result = list_agents(State(state)).await;
        assert_eq!(result.0.len(), 2);
    }

    #[tokio::test]
    async fn test_register_same_machine_different_port_creates_multiple_agents() {
        let state = create_test_state();

        let req1 = RegisterRequest {
            machine_name: "shared-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let res1 = register_agent(State(state.clone()), Json(req1))
            .await
            .unwrap()
            .0;
        assert_eq!(res1.status, RegisterStatus::Registered);

        let req2 = RegisterRequest {
            machine_name: "shared-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 12434,
        };
        let res2 = register_agent(State(state.clone()), Json(req2))
            .await
            .unwrap()
            .0;
        assert_eq!(res2.status, RegisterStatus::Registered);

        let agents = list_agents(State(state)).await.0;
        assert_eq!(agents.len(), 2);
    }

    #[tokio::test]
    async fn test_list_agent_metrics_returns_snapshot() {
        let state = create_test_state();

        // エージェントを登録
        let req = RegisterRequest {
            machine_name: "metrics-machine".to_string(),
            ip_address: "192.168.1.150".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let response = register_agent(State(state.clone()), Json(req))
            .await
            .unwrap()
            .0;

        // メトリクスを記録
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id: response.agent_id,
                cpu_usage: 42.0,
                memory_usage: 33.0,
                gpu_usage: Some(55.0),
                gpu_memory_usage: Some(48.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: None,
            })
            .await
            .unwrap();

        let metrics = list_agent_metrics(State(state)).await;
        assert_eq!(metrics.0.len(), 1);

        let snapshot = &metrics.0[0];
        assert_eq!(snapshot.agent_id, response.agent_id);
        assert_eq!(snapshot.cpu_usage.unwrap(), 42.0);
        assert_eq!(snapshot.memory_usage.unwrap(), 33.0);
        assert_eq!(snapshot.gpu_usage, Some(55.0));
        assert_eq!(snapshot.gpu_memory_usage, Some(48.0));
        assert_eq!(snapshot.active_requests, 1);
        assert!(!snapshot.is_stale);
    }

    #[tokio::test]
    async fn test_metrics_summary_empty() {
        let state = create_test_state();
        let summary = metrics_summary(State(state)).await;
        assert_eq!(summary.total_agents, 0);
        assert_eq!(summary.online_agents, 0);
        assert_eq!(summary.total_requests, 0);
        assert_eq!(summary.total_active_requests, 0);
        assert!(summary.average_response_time_ms.is_none());
        assert!(summary.last_metrics_updated_at.is_none());
    }

    #[tokio::test]
    async fn test_metrics_summary_counts_requests() {
        let state = create_test_state();

        // エージェントを登録
        let register_req = RegisterRequest {
            machine_name: "stats-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let response = register_agent(State(state.clone()), Json(register_req))
            .await
            .unwrap()
            .0;

        // ハートビートでメトリクス更新
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id: response.agent_id,
                cpu_usage: 55.0,
                memory_usage: 44.0,
                gpu_usage: Some(60.0),
                gpu_memory_usage: Some(62.0),
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

        // リクエストを成功・失敗で記録
        state
            .load_manager
            .begin_request(response.agent_id)
            .await
            .unwrap();
        state
            .load_manager
            .finish_request(
                response.agent_id,
                RequestOutcome::Success,
                Duration::from_millis(120),
            )
            .await
            .unwrap();

        state
            .load_manager
            .begin_request(response.agent_id)
            .await
            .unwrap();
        state
            .load_manager
            .finish_request(
                response.agent_id,
                RequestOutcome::Error,
                Duration::from_millis(200),
            )
            .await
            .unwrap();

        let summary = metrics_summary(State(state)).await;
        assert_eq!(summary.total_agents, 1);
        assert_eq!(summary.online_agents, 1);
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.successful_requests, 1);
        assert_eq!(summary.failed_requests, 1);
        assert_eq!(summary.total_active_requests, 2);
        let avg = summary.average_response_time_ms.unwrap();
        assert!((avg - 160.0).abs() < 0.1);
        assert!(summary.last_metrics_updated_at.is_some());
    }

    #[tokio::test]
    async fn test_update_agent_settings_endpoint() {
        let state = create_test_state();

        let agent_id = register_agent(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "agent-settings".into(),
                ip_address: "10.0.0.5".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
            }),
        )
        .await
        .unwrap()
        .0
        .agent_id;

        let payload = UpdateAgentSettingsPayload {
            custom_name: Some(Some("Primary".into())),
            tags: Some(vec!["dallas".into(), "gpu".into()]),
            notes: Some(Some("Keep online".into())),
        };

        let agent = update_agent_settings(
            State(state.clone()),
            axum::extract::Path(agent_id),
            Json(payload),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(agent.custom_name.as_deref(), Some("Primary"));
        assert_eq!(agent.tags, vec!["dallas", "gpu"]);
        assert_eq!(agent.notes.as_deref(), Some("Keep online"));
    }

    #[tokio::test]
    async fn test_delete_agent_endpoint() {
        let state = create_test_state();
        let response = register_agent(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "delete-agent".into(),
                ip_address: "10.0.0.7".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
            }),
        )
        .await
        .unwrap()
        .0;

        let status = delete_agent(State(state.clone()), axum::extract::Path(response.agent_id))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);

        let agents = list_agents(State(state)).await.0;
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn test_disconnect_agent_endpoint_marks_offline() {
        let state = create_test_state();
        let agent_id = register_agent(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "disconnect-agent".into(),
                ip_address: "10.0.0.8".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
            }),
        )
        .await
        .unwrap()
        .0
        .agent_id;

        let status = disconnect_agent(State(state.clone()), axum::extract::Path(agent_id))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);

        let agent = state.registry.get(agent_id).await.unwrap();
        assert_eq!(agent.status, AgentStatus::Offline);
    }
}
