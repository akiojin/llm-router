//! ヘルスチェックAPIハンドラー

use crate::{api::agent::AppError, AppState};
use axum::{extract::State, Json};
use ollama_coordinator_common::protocol::HealthCheckRequest;

/// POST /api/health - ヘルスチェック受信
pub async fn health_check(
    State(state): State<AppState>,
    Json(req): Json<HealthCheckRequest>,
) -> Result<Json<()>, AppError> {
    // エージェントの最終確認時刻を更新
    state.registry.update_last_seen(req.agent_id).await?;

    // TODO: T044でヘルスメトリクスをデータベースに保存

    Ok(Json(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AgentRegistry;
    use ollama_coordinator_common::protocol::RegisterRequest;
    use std::net::IpAddr;
    use uuid::Uuid;

    fn create_test_state() -> AppState {
        AppState {
            registry: AgentRegistry::new(),
        }
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let state = create_test_state();

        // まずエージェントを登録
        let register_req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let register_response = state.registry.register(register_req).await.unwrap();

        // ヘルスチェックを送信
        let health_req = HealthCheckRequest {
            agent_id: register_response.agent_id,
            cpu_usage: 45.5,
            memory_usage: 60.2,
            active_requests: 3,
        };

        let result = health_check(State(state.clone()), Json(health_req)).await;
        assert!(result.is_ok());

        // エージェントが更新されたことを確認
        let agent = state
            .registry
            .get(register_response.agent_id)
            .await
            .unwrap();
        assert_eq!(
            agent.status,
            ollama_coordinator_common::types::AgentStatus::Online
        );
    }

    #[tokio::test]
    async fn test_health_check_unknown_agent() {
        let state = create_test_state();

        let health_req = HealthCheckRequest {
            agent_id: Uuid::new_v4(),
            cpu_usage: 45.5,
            memory_usage: 60.2,
            active_requests: 3,
        };

        let result = health_check(State(state), Json(health_req)).await;
        assert!(result.is_err());
    }
}
