//! エージェント登録管理
//!
//! エージェントの状態をメモリ内で管理し、データベースと同期

pub mod models;

use chrono::Utc;
use ollama_coordinator_common::{
    error::{CoordinatorError, CoordinatorResult},
    protocol::{RegisterRequest, RegisterResponse, RegisterStatus},
    types::{Agent, AgentMetrics, AgentStatus, GpuDeviceInfo},
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

/// エージェントレジストリ
#[derive(Clone)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    metrics: Arc<RwLock<HashMap<Uuid, AgentMetrics>>>,
    storage_enabled: bool,
}

impl AgentRegistry {
    /// 新しいレジストリを作成
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            storage_enabled: false,
        }
    }

    /// ストレージ初期化付きでレジストリを作成
    pub async fn with_storage() -> CoordinatorResult<Self> {
        // ストレージ初期化
        crate::db::init_storage().await?;

        let registry = Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            storage_enabled: true,
        };

        // 起動時にストレージからエージェント情報を読み込み
        registry.load_from_storage().await?;

        Ok(registry)
    }

    /// ストレージからエージェント情報を読み込み
    async fn load_from_storage(&self) -> CoordinatorResult<()> {
        if !self.storage_enabled {
            return Ok(());
        }

        let loaded_agents = crate::db::load_agents().await?;
        let mut agents = self.agents.write().await;

        let mut removed_count = 0;
        let mut removed_ids = Vec::new();
        let mut sanitized_agents = Vec::new();

        for mut agent in loaded_agents {
            // GPU非搭載 or 情報欠落エージェントは削除対象
            if !agent.gpu_available {
                info!(
                    agent_id = %agent.id,
                    machine_name = %agent.machine_name,
                    reason = "gpu_available is false",
                    "Removing GPU-less agent from database during startup cleanup"
                );
                removed_count += 1;
                removed_ids.push(agent.id);
                continue;
            }

            let mut sanitized = false;

            if agent.gpu_devices.is_empty() {
                if let Some(model) = agent.gpu_model.clone() {
                    let count = agent.gpu_count.unwrap_or(1).max(1);
                    agent.gpu_devices = vec![GpuDeviceInfo {
                        model,
                        count,
                        memory: None,
                    }];
                    sanitized = true;
                } else {
                    info!(
                        agent_id = %agent.id,
                        machine_name = %agent.machine_name,
                        reason = "gpu_devices array is empty and gpu_model is None",
                        "Removing agent with missing GPU device information from database"
                    );
                    removed_count += 1;
                    removed_ids.push(agent.id);
                    continue;
                }
            }

            if !agent.gpu_devices.iter().all(|device| device.is_valid()) {
                info!(
                    agent_id = %agent.id,
                    machine_name = %agent.machine_name,
                    reason = "gpu_devices contains invalid device (empty model or zero count)",
                    "Removing agent with invalid GPU device information from database"
                );
                removed_count += 1;
                removed_ids.push(agent.id);
                continue;
            }

            if sanitized {
                sanitized_agents.push(agent.clone());
            }

            agents.insert(agent.id, agent);
        }

        info!(
            agents_loaded = agents.len(),
            agents_removed = removed_count,
            "Completed agent registry initialization from storage"
        );

        drop(agents);

        // 削除対象エージェントをデータベースから削除
        for id in removed_ids {
            if let Err(err) = crate::db::delete_agent(id).await {
                error!(
                    agent_id = %id,
                    error = %err,
                    "Failed to delete GPU-less agent from database during cleanup"
                );
            }
        }

        // サニタイズされたエージェント情報をストレージに保存
        for agent in sanitized_agents {
            if let Err(err) = self.save_to_storage(&agent).await {
                warn!(
                    agent_id = %agent.id,
                    machine_name = %agent.machine_name,
                    error = %err,
                    "Failed to persist sanitized agent data to storage"
                );
            }
        }

        Ok(())
    }

    /// エージェントをストレージに保存
    async fn save_to_storage(&self, agent: &Agent) -> CoordinatorResult<()> {
        if !self.storage_enabled {
            return Ok(());
        }

        crate::db::save_agent(agent).await
    }

    /// エージェントを登録
    pub async fn register(&self, req: RegisterRequest) -> CoordinatorResult<RegisterResponse> {
        let mut agents = self.agents.write().await;

        // 同じマシン名のエージェントが既に存在するか確認
        let existing = agents
            .values()
            .find(|a| a.machine_name == req.machine_name && a.ollama_port == req.ollama_port)
            .map(|a| a.id);

        let (agent_id, status, agent) = if let Some(id) = existing {
            // 既存エージェントを更新
            let agent = agents.get_mut(&id).unwrap();
            let now = Utc::now();
            let was_online = agent.status == AgentStatus::Online;
            agent.ip_address = req.ip_address;
            agent.ollama_version = req.ollama_version.clone();
            agent.ollama_port = req.ollama_port;
            agent.gpu_available = req.gpu_available;
            agent.gpu_devices = req.gpu_devices.clone();
            agent.gpu_count = req.gpu_count;
            agent.gpu_model = req.gpu_model.clone();
            agent.status = AgentStatus::Online;
            agent.last_seen = now;
            if !was_online || agent.online_since.is_none() {
                agent.online_since = Some(now);
            }
            agent.agent_api_port = Some(req.ollama_port + 1);
            agent.initializing = true;
            agent.ready_models = Some((0, 0));
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
                online_since: Some(now),
                custom_name: None,
                tags: Vec::new(),
                notes: None,
                loaded_models: Vec::new(),
                gpu_devices: req.gpu_devices,
                gpu_available: req.gpu_available,
                gpu_count: req.gpu_count,
                gpu_model: req.gpu_model,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                agent_api_port: Some(req.ollama_port + 1),
                initializing: true,
                ready_models: Some((0, 0)),
            };
            agents.insert(agent_id, agent.clone());
            (agent_id, RegisterStatus::Registered, agent)
        };

        // ロックを解放してからストレージ保存
        drop(agents);
        self.save_to_storage(&agent).await?;

        Ok(RegisterResponse {
            agent_id,
            status,
            agent_api_port: Some(agent.ollama_port + 1),
            auto_distributed_model: None,
            download_task_id: None,
        })
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
        let mut list: Vec<Agent> = agents.values().cloned().collect();
        list.sort_by(|a, b| a.registered_at.cmp(&b.registered_at));
        list
    }

    /// エージェントの最終確認時刻を更新
    #[allow(clippy::too_many_arguments)]
    pub async fn update_last_seen(
        &self,
        agent_id: Uuid,
        loaded_models: Option<Vec<String>>,
        gpu_model_name: Option<String>,
        gpu_compute_capability: Option<String>,
        gpu_capability_score: Option<u32>,
        initializing: Option<bool>,
        ready_models: Option<(u8, u8)>,
    ) -> CoordinatorResult<()> {
        let agent_to_save = {
            let mut agents = self.agents.write().await;
            let agent = agents
                .get_mut(&agent_id)
                .ok_or(CoordinatorError::AgentNotFound(agent_id))?;
            let now = Utc::now();
            let was_online = agent.status == AgentStatus::Online;
            agent.last_seen = now;
            agent.status = AgentStatus::Online;
            if let Some(models) = loaded_models {
                agent.loaded_models = normalize_models(models);
            }
            // GPU能力情報を更新
            if gpu_model_name.is_some() {
                agent.gpu_model_name = gpu_model_name;
            }
            if gpu_compute_capability.is_some() {
                agent.gpu_compute_capability = gpu_compute_capability;
            }
            if gpu_capability_score.is_some() {
                agent.gpu_capability_score = gpu_capability_score;
            }
            if !was_online || agent.online_since.is_none() {
                agent.online_since = Some(now);
            }
            if let Some(init) = initializing {
                agent.initializing = init;
            }
            if ready_models.is_some() {
                agent.ready_models = ready_models;
            }
            agent.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&agent_to_save).await?;
        Ok(())
    }

    /// モデルを「インストール済み」としてマーク
    pub async fn mark_model_loaded(
        &self,
        agent_id: Uuid,
        model_name: &str,
    ) -> CoordinatorResult<()> {
        let normalized = normalize_models(vec![model_name.to_string()]);
        let model = normalized.first().cloned().unwrap_or_default();

        let agent_to_save = {
            let mut agents = self.agents.write().await;
            let agent = agents
                .get_mut(&agent_id)
                .ok_or(CoordinatorError::AgentNotFound(agent_id))?;
            if !agent.loaded_models.contains(&model) {
                agent.loaded_models.push(model);
                agent.loaded_models.sort();
            }
            agent.clone()
        };

        // 永続化（失敗しても致命ではないがログとして残す）
        if let Err(e) = self.save_to_storage(&agent_to_save).await {
            warn!(
                agent_id = %agent_id,
                error = %e,
                "Failed to persist loaded_models update"
            );
        }

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
            agent.online_since = None;
            agent.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&agent_to_save).await?;
        Ok(())
    }
}

/// エージェント設定更新用ペイロード
pub struct AgentSettingsUpdate {
    /// カスタム表示名（Noneで未指定, Some(None)でリセット）
    pub custom_name: Option<Option<String>>,
    /// タグ配列
    pub tags: Option<Vec<String>>,
    /// メモ（Noneで未指定, Some(None)でリセット）
    pub notes: Option<Option<String>>,
}

impl AgentRegistry {
    /// エージェント設定を更新
    pub async fn update_settings(
        &self,
        agent_id: Uuid,
        settings: AgentSettingsUpdate,
    ) -> CoordinatorResult<Agent> {
        let updated_agent = {
            let mut agents = self.agents.write().await;
            let agent = agents
                .get_mut(&agent_id)
                .ok_or(CoordinatorError::AgentNotFound(agent_id))?;

            if let Some(custom_name) = settings.custom_name {
                agent.custom_name = custom_name.and_then(|name| {
                    let trimmed = name.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                });
            }

            if let Some(tags) = settings.tags {
                agent.tags = tags
                    .into_iter()
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }

            if let Some(notes) = settings.notes {
                agent.notes = notes.and_then(|note| {
                    let trimmed = note.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                });
            }

            agent.clone()
        };

        self.save_to_storage(&updated_agent).await?;
        Ok(updated_agent)
    }

    /// エージェントを削除
    pub async fn delete(&self, agent_id: Uuid) -> CoordinatorResult<()> {
        let existed = {
            let mut agents = self.agents.write().await;
            agents.remove(&agent_id)
        };

        if existed.is_none() {
            return Err(CoordinatorError::AgentNotFound(agent_id));
        }

        if self.storage_enabled {
            crate::db::delete_agent(agent_id).await
        } else {
            Ok(())
        }
    }

    /// エージェントメトリクスを更新
    ///
    /// エージェントから送信されたメトリクス情報（CPU使用率、メモリ使用率、アクティブリクエスト数等）を
    /// メモリ内のHashMapに保存する。エージェントが存在しない場合はエラーを返す。
    pub async fn update_metrics(&self, metrics: AgentMetrics) -> CoordinatorResult<()> {
        // エージェントが存在するか確認
        {
            let agents = self.agents.read().await;
            if !agents.contains_key(&metrics.agent_id) {
                return Err(CoordinatorError::AgentNotFound(metrics.agent_id));
            }
        }

        // メトリクスを保存
        let mut metrics_map = self.metrics.write().await;
        metrics_map.insert(metrics.agent_id, metrics);

        Ok(())
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_models(models: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for model in models {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            continue;
        }

        let canonical = trimmed.to_string();
        if seen.insert(canonical.clone()) {
            normalized.push(canonical);
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_coordinator_common::types::GpuDeviceInfo;
    use std::net::IpAddr;

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: None,
        }]
    }

    #[tokio::test]
    async fn test_register_new_agent() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = registry.register(req).await.unwrap();
        assert_eq!(response.status, RegisterStatus::Registered);

        let agent = registry.get(response.agent_id).await.unwrap();
        assert_eq!(agent.machine_name, "test-machine");
        assert_eq!(agent.status, AgentStatus::Online);
        assert!(agent.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_register_existing_agent() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let first_response = registry.register(req.clone()).await.unwrap();
        assert_eq!(first_response.status, RegisterStatus::Registered);

        let second_response = registry.register(req).await.unwrap();
        assert_eq!(second_response.status, RegisterStatus::Updated);
        assert_eq!(first_response.agent_id, second_response.agent_id);

        let agent = registry.get(first_response.agent_id).await.unwrap();
        assert!(agent.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_list_agents() {
        let registry = AgentRegistry::new();

        let req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        registry.register(req1).await.unwrap();

        let req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
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
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = registry.register(req).await.unwrap();
        registry.mark_offline(response.agent_id).await.unwrap();

        let agent = registry.get(response.agent_id).await.unwrap();
        assert_eq!(agent.status, AgentStatus::Offline);
        assert!(agent.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_update_settings() {
        let registry = AgentRegistry::new();
        let req = RegisterRequest {
            machine_name: "settings-machine".to_string(),
            ip_address: "192.168.1.150".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let agent_id = registry.register(req).await.unwrap().agent_id;

        let updated = registry
            .update_settings(
                agent_id,
                AgentSettingsUpdate {
                    custom_name: Some(Some("Display".into())),
                    tags: Some(vec!["primary".into(), "gpu".into()]),
                    notes: Some(Some("Important".into())),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.custom_name.as_deref(), Some("Display"));
        assert_eq!(updated.tags, vec!["primary", "gpu"]);
        assert_eq!(updated.notes.as_deref(), Some("Important"));
        assert!(updated.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_delete_agent_removes_from_registry() {
        let registry = AgentRegistry::new();
        let agent_id = registry
            .register(RegisterRequest {
                machine_name: "delete-me".to_string(),
                ip_address: "127.0.0.1".parse().unwrap(),
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

        registry.delete(agent_id).await.unwrap();
        assert!(registry.list().await.is_empty());
    }

    #[tokio::test]
    async fn test_update_last_seen_updates_models() {
        let registry = AgentRegistry::new();
        let agent_id = registry
            .register(RegisterRequest {
                machine_name: "models".into(),
                ip_address: "127.0.0.1".parse().unwrap(),
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

        registry
            .update_last_seen(
                agent_id,
                Some(vec![
                    " gpt-oss:20b ".into(),
                    "gpt-oss:20b".into(),
                    "".into(),
                    "phi-3".into(),
                ]),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let agent = registry.get(agent_id).await.unwrap();
        assert_eq!(agent.loaded_models, vec!["gpt-oss:20b", "phi-3"]);
    }

    #[test]
    fn test_normalize_models_removes_duplicates() {
        let models = vec![
            "a ".into(),
            "b".into(),
            "a".into(),
            " ".into(),
            "".into(),
            "c".into(),
            "b".into(),
        ];
        assert_eq!(
            normalize_models(models),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
