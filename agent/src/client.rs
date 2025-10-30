//! Coordinator通信クライアント
//!
//! エージェント登録とハートビート送信

use uuid::Uuid;
use ollama_coordinator_common::{
    protocol::{RegisterRequest, RegisterResponse, HealthCheckRequest},
    error::{AgentError, AgentResult},
};

/// Coordinatorクライアント
pub struct CoordinatorClient {
    coordinator_url: String,
    agent_id: Option<Uuid>,
    http_client: reqwest::Client,
}

impl CoordinatorClient {
    /// 新しいCoordinatorクライアントを作成
    pub fn new(coordinator_url: String) -> Self {
        Self {
            coordinator_url,
            agent_id: None,
            http_client: reqwest::Client::new(),
        }
    }

    /// エージェントを登録
    pub async fn register(&mut self, req: RegisterRequest) -> AgentResult<RegisterResponse> {
        let url = format!("{}/api/agents", self.coordinator_url);

        let response = self
            .http_client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| AgentError::CoordinatorConnection(format!("Failed to register: {}", e)))?;

        if !response.status().is_success() {
            return Err(AgentError::Registration(format!(
                "Registration failed with status: {}",
                response.status()
            )));
        }

        let register_response = response
            .json::<RegisterResponse>()
            .await
            .map_err(|e| AgentError::Registration(format!("Failed to parse response: {}", e)))?;

        // エージェントIDを保存
        self.agent_id = Some(register_response.agent_id);

        println!("Registered with Coordinator: agent_id = {}", register_response.agent_id);

        Ok(register_response)
    }

    /// ヘルスチェックを送信
    pub async fn send_heartbeat(&self, req: HealthCheckRequest) -> AgentResult<()> {
        let url = format!("{}/api/health", self.coordinator_url);

        let response = self
            .http_client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| AgentError::Heartbeat(format!("Failed to send heartbeat: {}", e)))?;

        if !response.status().is_success() {
            return Err(AgentError::Heartbeat(format!(
                "Heartbeat failed with status: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// 保存されたエージェントIDを取得
    pub fn get_agent_id(&self) -> Option<Uuid> {
        self.agent_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_client_creation() {
        let client = CoordinatorClient::new("http://localhost:8080".to_string());
        assert_eq!(client.coordinator_url, "http://localhost:8080");
        assert!(client.agent_id.is_none());
    }

    #[test]
    fn test_get_agent_id_none() {
        let client = CoordinatorClient::new("http://localhost:8080".to_string());
        assert!(client.get_agent_id().is_none());
    }
}
