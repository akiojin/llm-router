//! Ollamaプロキシ APIハンドラー

use crate::{api::agent::AppError, balancer::RequestOutcome, AppState};
use axum::{extract::State, Json};
use ollama_coordinator_common::{
    error::CoordinatorError,
    protocol::{ChatRequest, ChatResponse, GenerateRequest},
};
use std::time::Instant;

/// POST /api/chat - Ollama Chat APIプロキシ
pub async fn proxy_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;
    let agent_id = agent.id;

    // リクエスト開始を記録
    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!("http://{}:{}/api/chat", agent.ip_address, agent.ollama_port);
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&req).send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            return Err(
                CoordinatorError::Http(format!("Failed to proxy chat request: {}", e)).into(),
            );
        }
    };

    if !response.status().is_success() {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;

        return Err(CoordinatorError::Http(format!(
            "Ollama returned error: {}",
            response.status()
        ))
        .into());
    }

    let parsed = response.json::<ChatResponse>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(payload) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Success, duration)
                .await
                .map_err(AppError::from)?;

            Ok(Json(payload))
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            Err(CoordinatorError::Http(format!("Failed to parse chat response: {}", e)).into())
        }
    }
}

/// POST /api/generate - Ollama Generate APIプロキシ
pub async fn proxy_generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;
    let agent_id = agent.id;

    // リクエスト開始を記録
    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}/api/generate",
        agent.ip_address, agent.ollama_port
    );
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&req).send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            return Err(
                CoordinatorError::Http(format!("Failed to proxy generate request: {}", e)).into(),
            );
        }
    };

    if !response.status().is_success() {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;

        return Err(CoordinatorError::Http(format!(
            "Ollama returned error: {}",
            response.status()
        ))
        .into());
    }

    let parsed = response.json::<serde_json::Value>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(payload) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Success, duration)
                .await
                .map_err(AppError::from)?;

            Ok(Json(payload))
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            Err(CoordinatorError::Http(format!("Failed to parse generate response: {}", e)).into())
        }
    }
}

/// 利用可能なエージェントを選択（負荷ベース + ラウンドロビンフォールバック）
async fn select_available_agent(
    state: &AppState,
) -> Result<ollama_coordinator_common::types::Agent, CoordinatorError> {
    state.load_manager.select_agent().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{balancer::LoadManager, registry::AgentRegistry};
    use ollama_coordinator_common::protocol::RegisterRequest;
    use std::net::IpAddr;

    fn create_test_state() -> AppState {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        AppState {
            registry,
            load_manager,
        }
    }

    #[tokio::test]
    async fn test_select_available_agent_no_agents() {
        let state = create_test_state();
        let result = select_available_agent(&state).await;
        assert!(matches!(result, Err(CoordinatorError::NoAgentsAvailable)));
    }

    #[tokio::test]
    async fn test_select_available_agent_success() {
        let state = create_test_state();

        // エージェントを登録
        let register_req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        state.registry.register(register_req).await.unwrap();

        let result = select_available_agent(&state).await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.machine_name, "test-machine");
    }

    #[tokio::test]
    async fn test_select_available_agent_skips_offline() {
        let state = create_test_state();

        // エージェント1を登録
        let register_req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let response1 = state.registry.register(register_req1).await.unwrap();

        // エージェント1をオフラインにする
        state
            .registry
            .mark_offline(response1.agent_id)
            .await
            .unwrap();

        // エージェント2を登録
        let register_req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        state.registry.register(register_req2).await.unwrap();

        let result = select_available_agent(&state).await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.machine_name, "machine2");
    }
}
