//! エージェント登録APIハンドラー

use axum::{
    extract::State,
    http::StatusCode,
    Json,
    response::IntoResponse,
};
use ollama_coordinator_common::{
    protocol::{RegisterRequest, RegisterResponse},
    error::CoordinatorError,
};
use crate::AppState;

/// POST /api/agents - エージェント登録
pub async fn register_agent(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    let response = state.registry.register(req).await?;
    Ok(Json(response))
}

/// Axum用のエラーレスポンス型
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
            CoordinatorError::NoAgentsAvailable => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            CoordinatorError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            CoordinatorError::Http(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            CoordinatorError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            CoordinatorError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            CoordinatorError::Common(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AgentRegistry;
    use std::net::IpAddr;

    fn create_test_state() -> AppState {
        AppState {
            registry: AgentRegistry::new(),
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
}
