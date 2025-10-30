//! Ollamaプロキシ APIハンドラー

use crate::{api::agent::AppError, AppState};
use axum::{extract::State, Json};
use ollama_coordinator_common::{
    error::CoordinatorError,
    protocol::{ChatRequest, ChatResponse, GenerateRequest},
    types::AgentStatus,
};

/// POST /api/chat - Ollama Chat APIプロキシ
pub async fn proxy_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;

    // OllamaインスタンスにリクエストをProxy
    let ollama_url = format!("http://{}:{}/api/chat", agent.ip_address, agent.ollama_port);

    let client = reqwest::Client::new();
    let response = client
        .post(&ollama_url)
        .json(&req)
        .send()
        .await
        .map_err(|e| CoordinatorError::Http(format!("Failed to proxy chat request: {}", e)))?;

    if !response.status().is_success() {
        return Err(CoordinatorError::Http(format!(
            "Ollama returned error: {}",
            response.status()
        ))
        .into());
    }

    let chat_response = response
        .json::<ChatResponse>()
        .await
        .map_err(|e| CoordinatorError::Http(format!("Failed to parse chat response: {}", e)))?;

    Ok(Json(chat_response))
}

/// POST /api/generate - Ollama Generate APIプロキシ
pub async fn proxy_generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;

    // OllamaインスタンスにリクエストをProxy
    let ollama_url = format!(
        "http://{}:{}/api/generate",
        agent.ip_address, agent.ollama_port
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&ollama_url)
        .json(&req)
        .send()
        .await
        .map_err(|e| CoordinatorError::Http(format!("Failed to proxy generate request: {}", e)))?;

    if !response.status().is_success() {
        return Err(CoordinatorError::Http(format!(
            "Ollama returned error: {}",
            response.status()
        ))
        .into());
    }

    let generate_response = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| CoordinatorError::Http(format!("Failed to parse generate response: {}", e)))?;

    Ok(Json(generate_response))
}

/// 利用可能なエージェントを選択（シンプルなラウンドロビン）
async fn select_available_agent(
    state: &AppState,
) -> Result<ollama_coordinator_common::types::Agent, CoordinatorError> {
    let agents = state.registry.list().await;

    // オンラインのエージェントのみをフィルタ
    let online_agents: Vec<_> = agents
        .into_iter()
        .filter(|a| a.status == AgentStatus::Online)
        .collect();

    if online_agents.is_empty() {
        return Err(CoordinatorError::NoAgentsAvailable);
    }

    // 最初のオンラインエージェントを返す（TODO: より高度なロードバランシング）
    Ok(online_agents[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AgentRegistry;
    use ollama_coordinator_common::protocol::RegisterRequest;
    use std::net::IpAddr;

    fn create_test_state() -> AppState {
        AppState {
            registry: AgentRegistry::new(),
        }
    }

    #[tokio::test]
    async fn test_select_available_agent_no_agents() {
        let state = create_test_state();
        let result = select_available_agent(&state).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CoordinatorError::NoAgentsAvailable
        ));
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
