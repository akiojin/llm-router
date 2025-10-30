//! エージェント登録管理
//!
//! エージェントの状態をメモリ内で管理し、データベースと同期

use chrono::Utc;
use ollama_coordinator_common::{
    error::{CoordinatorError, CoordinatorResult},
    protocol::{RegisterRequest, RegisterResponse, RegisterStatus},
    types::{Agent, AgentStatus},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// エージェントレジストリ
#[derive(Clone)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
}

impl AgentRegistry {
    /// 新しいレジストリを作成
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// ストレージ初期化付きでレジストリを作成
    pub async fn with_storage() -> CoordinatorResult<Self> {
        // ストレージ初期化
        crate::db::init_storage().await?;

        let registry = Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        };

        // 起動時にストレージからエージェント情報を読み込み
        registry.load_from_storage().await?;

        Ok(registry)
    }

    /// ストレージからエージェント情報を読み込み
    async fn load_from_storage(&self) -> CoordinatorResult<()> {
        let loaded_agents = crate::db::load_agents().await?;
        let mut agents = self.agents.write().await;

        for agent in loaded_agents {
            agents.insert(agent.id, agent);
        }

        println!("Loaded {} agents from storage", agents.len());

        Ok(())
    }

    /// エージェントをストレージに保存
    async fn save_to_storage(&self, agent: &Agent) -> CoordinatorResult<()> {
        crate::db::save_agent(agent).await
    }

    /// エージェントを登録
    pub async fn register(&self, req: RegisterRequest) -> CoordinatorResult<RegisterResponse> {
        let mut agents = self.agents.write().await;

        // 同じマシン名のエージェントが既に存在するか確認
        let existing = agents
            .values()
            .find(|a| a.machine_name == req.machine_name)
            .map(|a| a.id);

        let (agent_id, status, agent) = if let Some(id) = existing {
            // 既存エージェントを更新
            let agent = agents.get_mut(&id).unwrap();
            agent.ip_address = req.ip_address;
            agent.ollama_version = req.ollama_version.clone();
            agent.ollama_port = req.ollama_port;
            agent.status = AgentStatus::Online;
            agent.last_seen = Utc::now();
            (id, RegisterStatus::Updated, agent.clone())
        } else {
            // 新規エージェントを登録
            let agent_id = Uuid::new_v4();
            let now = Utc::now();
            let agent = Agent {
                id: agent_id,
                machine_name: req.machine_name,
                ip_address: req.ip_address,
                ollama_version: req.ollama_version,
                ollama_port: req.ollama_port,
                status: AgentStatus::Online,
                registered_at: now,
                last_seen: now,
            };
            agents.insert(agent_id, agent.clone());
            (agent_id, RegisterStatus::Registered, agent)
        };

        // ロックを解放してからストレージ保存
        drop(agents);
        self.save_to_storage(&agent).await?;

        Ok(RegisterResponse { agent_id, status })
    }

    /// エージェントを取得
    pub async fn get(&self, agent_id: Uuid) -> CoordinatorResult<Agent> {
        let agents = self.agents.read().await;
        agents
            .get(&agent_id)
            .cloned()
            .ok_or(CoordinatorError::AgentNotFound(agent_id))
    }

    /// 全エージェントを取得
    pub async fn list(&self) -> Vec<Agent> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }

    /// エージェントの最終確認時刻を更新
    pub async fn update_last_seen(&self, agent_id: Uuid) -> CoordinatorResult<()> {
        let agent_to_save = {
            let mut agents = self.agents.write().await;
            let agent = agents
                .get_mut(&agent_id)
                .ok_or(CoordinatorError::AgentNotFound(agent_id))?;
            agent.last_seen = Utc::now();
            agent.status = AgentStatus::Online;
            agent.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&agent_to_save).await?;
        Ok(())
    }

    /// エージェントをオフラインにする
    pub async fn mark_offline(&self, agent_id: Uuid) -> CoordinatorResult<()> {
        let agent_to_save = {
            let mut agents = self.agents.write().await;
            let agent = agents
                .get_mut(&agent_id)
                .ok_or(CoordinatorError::AgentNotFound(agent_id))?;
            agent.status = AgentStatus::Offline;
            agent.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&agent_to_save).await?;
        Ok(())
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[tokio::test]
    async fn test_register_new_agent() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let response = registry.register(req).await.unwrap();
        assert_eq!(response.status, RegisterStatus::Registered);

        let agent = registry.get(response.agent_id).await.unwrap();
        assert_eq!(agent.machine_name, "test-machine");
        assert_eq!(agent.status, AgentStatus::Online);
    }

    #[tokio::test]
    async fn test_register_existing_agent() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let first_response = registry.register(req.clone()).await.unwrap();
        assert_eq!(first_response.status, RegisterStatus::Registered);

        let second_response = registry.register(req).await.unwrap();
        assert_eq!(second_response.status, RegisterStatus::Updated);
        assert_eq!(first_response.agent_id, second_response.agent_id);
    }

    #[tokio::test]
    async fn test_list_agents() {
        let registry = AgentRegistry::new();

        let req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        registry.register(req1).await.unwrap();

        let req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        registry.register(req2).await.unwrap();

        let agents = registry.list().await;
        assert_eq!(agents.len(), 2);
    }

    #[tokio::test]
    async fn test_mark_offline() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let response = registry.register(req).await.unwrap();
        registry.mark_offline(response.agent_id).await.unwrap();

        let agent = registry.get(response.agent_id).await.unwrap();
        assert_eq!(agent.status, AgentStatus::Offline);
    }
}
