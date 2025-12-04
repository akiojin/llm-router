//! ノード登録管理
//!
//! ノードの状態をメモリ内で管理し、データベースと同期

pub mod models;

use chrono::Utc;
use llm_router_common::{
    error::{RouterError, RouterResult},
    protocol::{RegisterRequest, RegisterResponse, RegisterStatus},
    types::{AgentMetrics, GpuDeviceInfo, Node, NodeStatus},
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

/// ノードレジストリ
#[derive(Clone)]
pub struct NodeRegistry {
    nodes: Arc<RwLock<HashMap<Uuid, Node>>>,
    metrics: Arc<RwLock<HashMap<Uuid, AgentMetrics>>>,
    storage_enabled: bool,
}

impl NodeRegistry {
    /// 新しいレジストリを作成
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            storage_enabled: false,
        }
    }

    /// ストレージ初期化付きでレジストリを作成
    pub async fn with_storage() -> RouterResult<Self> {
        // ストレージ初期化
        crate::db::init_storage().await?;

        let registry = Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            storage_enabled: true,
        };

        // 起動時にストレージからノード情報を読み込み
        registry.load_from_storage().await?;

        Ok(registry)
    }

    /// ストレージからノード情報を読み込み
    async fn load_from_storage(&self) -> RouterResult<()> {
        if !self.storage_enabled {
            return Ok(());
        }

        let loaded_nodes = crate::db::load_nodes().await?;
        let mut nodes = self.nodes.write().await;

        let mut removed_count = 0;
        let mut removed_ids = Vec::new();
        let mut sanitized_nodes = Vec::new();

        for mut node in loaded_nodes {
            // GPU非搭載 or 情報欠落ノードは削除対象
            if !node.gpu_available {
                info!(
                    node_id = %node.id,
                    machine_name = %node.machine_name,
                    reason = "gpu_available is false",
                    "Removing GPU-less node from database during startup cleanup"
                );
                removed_count += 1;
                removed_ids.push(node.id);
                continue;
            }

            let mut sanitized = false;

            if node.gpu_devices.is_empty() {
                if let Some(model) = node.gpu_model.clone() {
                    let count = node.gpu_count.unwrap_or(1).max(1);
                    node.gpu_devices = vec![GpuDeviceInfo {
                        model,
                        count,
                        memory: None,
                    }];
                    sanitized = true;
                } else {
                    info!(
                        node_id = %node.id,
                        machine_name = %node.machine_name,
                        reason = "gpu_devices array is empty and gpu_model is None",
                        "Removing node with missing GPU device information from database"
                    );
                    removed_count += 1;
                    removed_ids.push(node.id);
                    continue;
                }
            }

            if !node.gpu_devices.iter().all(|device| device.is_valid()) {
                info!(
                    node_id = %node.id,
                    machine_name = %node.machine_name,
                    reason = "gpu_devices contains invalid device (empty model or zero count)",
                    "Removing node with invalid GPU device information from database"
                );
                removed_count += 1;
                removed_ids.push(node.id);
                continue;
            }

            if sanitized {
                sanitized_nodes.push(node.clone());
            }

            nodes.insert(node.id, node);
        }

        info!(
            nodes_loaded = nodes.len(),
            nodes_removed = removed_count,
            "Completed node registry initialization from storage"
        );

        drop(nodes);

        // 削除対象ノードをデータベースから削除
        for id in removed_ids {
            if let Err(err) = crate::db::delete_node(id).await {
                error!(
                    node_id = %id,
                    error = %err,
                    "Failed to delete GPU-less node from database during cleanup"
                );
            }
        }

        // サニタイズされたノード情報をストレージに保存
        for node in sanitized_nodes {
            if let Err(err) = self.save_to_storage(&node).await {
                warn!(
                    node_id = %node.id,
                    machine_name = %node.machine_name,
                    error = %err,
                    "Failed to persist sanitized node data to storage"
                );
            }
        }

        Ok(())
    }

    /// ノードをストレージに保存
    async fn save_to_storage(&self, node: &Node) -> RouterResult<()> {
        if !self.storage_enabled {
            return Ok(());
        }

        crate::db::save_node(node).await
    }

    /// ノードを登録
    pub async fn register(&self, req: RegisterRequest) -> RouterResult<RegisterResponse> {
        let mut nodes = self.nodes.write().await;

        // 同じマシン名のノードが既に存在するか確認
        let existing = nodes
            .values()
            .find(|n| n.machine_name == req.machine_name && n.runtime_port == req.runtime_port)
            .map(|n| n.id);

        let (node_id, status, node) = if let Some(id) = existing {
            // 既存ノードを更新
            let node = nodes.get_mut(&id).unwrap();
            let now = Utc::now();
            let was_online = node.status == NodeStatus::Online;
            node.ip_address = req.ip_address;
            node.runtime_version = req.runtime_version.clone();
            node.runtime_port = req.runtime_port;
            node.gpu_available = req.gpu_available;
            node.gpu_devices = req.gpu_devices.clone();
            node.gpu_count = req.gpu_count;
            node.gpu_model = req.gpu_model.clone();
            node.status = NodeStatus::Online;
            node.last_seen = now;
            if !was_online || node.online_since.is_none() {
                node.online_since = Some(now);
            }
            node.agent_api_port = Some(req.runtime_port + 1);
            node.initializing = true;
            node.ready_models = Some((0, 0));
            (id, RegisterStatus::Updated, node.clone())
        } else {
            // 新規ノードを登録
            let node_id = Uuid::new_v4();
            let now = Utc::now();
            let node = Node {
                id: node_id,
                machine_name: req.machine_name,
                ip_address: req.ip_address,
                runtime_version: req.runtime_version,
                runtime_port: req.runtime_port,
                status: NodeStatus::Online,
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
                agent_api_port: Some(req.runtime_port + 1),
                initializing: true,
                ready_models: Some((0, 0)),
            };
            nodes.insert(node_id, node.clone());
            (node_id, RegisterStatus::Registered, node)
        };

        // ロックを解放してからストレージ保存
        drop(nodes);
        self.save_to_storage(&node).await?;

        Ok(RegisterResponse {
            node_id,
            status,
            agent_api_port: Some(node.runtime_port + 1),
            auto_distributed_model: None,
            download_task_id: None,
            agent_token: None,
        })
    }

    /// ノードを取得
    pub async fn get(&self, node_id: Uuid) -> RouterResult<Node> {
        let nodes = self.nodes.read().await;
        nodes
            .get(&node_id)
            .cloned()
            .ok_or(RouterError::AgentNotFound(node_id))
    }

    /// 全ノードを取得
    pub async fn list(&self) -> Vec<Node> {
        let nodes = self.nodes.read().await;
        let mut list: Vec<Node> = nodes.values().cloned().collect();
        list.sort_by(|a, b| a.registered_at.cmp(&b.registered_at));
        list
    }

    /// ノードの最終確認時刻を更新
    #[allow(clippy::too_many_arguments)]
    pub async fn update_last_seen(
        &self,
        node_id: Uuid,
        loaded_models: Option<Vec<String>>,
        gpu_model_name: Option<String>,
        gpu_compute_capability: Option<String>,
        gpu_capability_score: Option<u32>,
        initializing: Option<bool>,
        ready_models: Option<(u8, u8)>,
    ) -> RouterResult<()> {
        let node_to_save = {
            let mut nodes = self.nodes.write().await;
            let node = nodes
                .get_mut(&node_id)
                .ok_or(RouterError::AgentNotFound(node_id))?;
            let now = Utc::now();
            let was_online = node.status == NodeStatus::Online;
            node.last_seen = now;
            node.status = NodeStatus::Online;
            if let Some(models) = loaded_models {
                node.loaded_models = normalize_models(models);
            }
            // GPU能力情報を更新
            if gpu_model_name.is_some() {
                node.gpu_model_name = gpu_model_name;
            }
            if gpu_compute_capability.is_some() {
                node.gpu_compute_capability = gpu_compute_capability;
            }
            if gpu_capability_score.is_some() {
                node.gpu_capability_score = gpu_capability_score;
            }
            if !was_online || node.online_since.is_none() {
                node.online_since = Some(now);
            }
            if let Some(init) = initializing {
                node.initializing = init;
            }
            if ready_models.is_some() {
                node.ready_models = ready_models;
            }
            node.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&node_to_save).await?;
        Ok(())
    }

    /// モデルを「インストール済み」としてマーク
    pub async fn mark_model_loaded(&self, node_id: Uuid, model_name: &str) -> RouterResult<()> {
        let normalized = normalize_models(vec![model_name.to_string()]);
        let model = normalized.first().cloned().unwrap_or_default();

        let node_to_save = {
            let mut nodes = self.nodes.write().await;
            let node = nodes
                .get_mut(&node_id)
                .ok_or(RouterError::AgentNotFound(node_id))?;
            if !node.loaded_models.contains(&model) {
                node.loaded_models.push(model);
                node.loaded_models.sort();
            }
            node.clone()
        };

        // 永続化（失敗しても致命ではないがログとして残す）
        if let Err(e) = self.save_to_storage(&node_to_save).await {
            warn!(
                node_id = %node_id,
                error = %e,
                "Failed to persist loaded_models update"
            );
        }

        Ok(())
    }

    /// ノードをオフラインにする
    pub async fn mark_offline(&self, node_id: Uuid) -> RouterResult<()> {
        let node_to_save = {
            let mut nodes = self.nodes.write().await;
            let node = nodes
                .get_mut(&node_id)
                .ok_or(RouterError::AgentNotFound(node_id))?;
            node.status = NodeStatus::Offline;
            node.online_since = None;
            node.clone()
        };

        // ロック解放後にストレージ保存
        self.save_to_storage(&node_to_save).await?;
        Ok(())
    }
}

/// ノード設定更新用ペイロード
pub struct NodeSettingsUpdate {
    /// カスタム表示名（Noneで未指定, Some(None)でリセット）
    pub custom_name: Option<Option<String>>,
    /// タグ配列
    pub tags: Option<Vec<String>>,
    /// メモ（Noneで未指定, Some(None)でリセット）
    pub notes: Option<Option<String>>,
}

impl NodeRegistry {
    /// ノード設定を更新
    pub async fn update_settings(
        &self,
        node_id: Uuid,
        settings: NodeSettingsUpdate,
    ) -> RouterResult<Node> {
        let updated_node = {
            let mut nodes = self.nodes.write().await;
            let node = nodes
                .get_mut(&node_id)
                .ok_or(RouterError::AgentNotFound(node_id))?;

            if let Some(custom_name) = settings.custom_name {
                node.custom_name = custom_name.and_then(|name| {
                    let trimmed = name.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                });
            }

            if let Some(tags) = settings.tags {
                node.tags = tags
                    .into_iter()
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }

            if let Some(notes) = settings.notes {
                node.notes = notes.and_then(|note| {
                    let trimmed = note.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                });
            }

            node.clone()
        };

        self.save_to_storage(&updated_node).await?;
        Ok(updated_node)
    }

    /// ノードを削除
    pub async fn delete(&self, node_id: Uuid) -> RouterResult<()> {
        let existed = {
            let mut nodes = self.nodes.write().await;
            nodes.remove(&node_id)
        };

        if existed.is_none() {
            return Err(RouterError::AgentNotFound(node_id));
        }

        if self.storage_enabled {
            crate::db::delete_node(node_id).await
        } else {
            Ok(())
        }
    }

    /// ノードメトリクスを更新
    ///
    /// ノードから送信されたメトリクス情報（CPU使用率、メモリ使用率、アクティブリクエスト数等）を
    /// メモリ内のHashMapに保存する。ノードが存在しない場合はエラーを返す。
    pub async fn update_metrics(&self, metrics: AgentMetrics) -> RouterResult<()> {
        // ノードが存在するか確認
        {
            let nodes = self.nodes.read().await;
            if !nodes.contains_key(&metrics.node_id) {
                return Err(RouterError::AgentNotFound(metrics.node_id));
            }
        }

        // メトリクスを保存
        let mut metrics_map = self.metrics.write().await;
        metrics_map.insert(metrics.node_id, metrics);

        Ok(())
    }
}

impl Default for NodeRegistry {
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
    use llm_router_common::types::GpuDeviceInfo;
    use std::net::IpAddr;

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: None,
        }]
    }

    #[tokio::test]
    async fn test_register_new_node() {
        let registry = NodeRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = registry.register(req).await.unwrap();
        assert_eq!(response.status, RegisterStatus::Registered);

        let node = registry.get(response.node_id).await.unwrap();
        assert_eq!(node.machine_name, "test-machine");
        assert_eq!(node.status, NodeStatus::Online);
        assert!(node.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_register_existing_node() {
        let registry = NodeRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let first_response = registry.register(req.clone()).await.unwrap();
        assert_eq!(first_response.status, RegisterStatus::Registered);

        let second_response = registry.register(req).await.unwrap();
        assert_eq!(second_response.status, RegisterStatus::Updated);
        assert_eq!(first_response.node_id, second_response.node_id);

        let node = registry.get(first_response.node_id).await.unwrap();
        assert!(node.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_list_nodes() {
        let registry = NodeRegistry::new();

        let req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        registry.register(req1).await.unwrap();

        let req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        registry.register(req2).await.unwrap();

        let nodes = registry.list().await;
        assert_eq!(nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_mark_offline() {
        let registry = NodeRegistry::new();
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = registry.register(req).await.unwrap();
        registry.mark_offline(response.node_id).await.unwrap();

        let node = registry.get(response.node_id).await.unwrap();
        assert_eq!(node.status, NodeStatus::Offline);
        assert!(node.loaded_models.is_empty());
    }

    #[tokio::test]
    async fn test_update_settings() {
        let registry = NodeRegistry::new();
        let req = RegisterRequest {
            machine_name: "settings-machine".to_string(),
            ip_address: "192.168.1.150".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let node_id = registry.register(req).await.unwrap().node_id;

        let updated = registry
            .update_settings(
                node_id,
                NodeSettingsUpdate {
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
    async fn test_delete_node_removes_from_registry() {
        let registry = NodeRegistry::new();
        let node_id = registry
            .register(RegisterRequest {
                machine_name: "delete-me".to_string(),
                ip_address: "127.0.0.1".parse().unwrap(),
                runtime_version: "0.1.0".to_string(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .node_id;

        registry.delete(node_id).await.unwrap();
        assert!(registry.list().await.is_empty());
    }

    #[tokio::test]
    async fn test_update_last_seen_updates_models() {
        let registry = NodeRegistry::new();
        let node_id = registry
            .register(RegisterRequest {
                machine_name: "models".into(),
                ip_address: "127.0.0.1".parse().unwrap(),
                runtime_version: "0.1.0".into(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .node_id;

        registry
            .update_last_seen(
                node_id,
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

        let node = registry.get(node_id).await.unwrap();
        assert_eq!(node.loaded_models, vec!["gpt-oss:20b", "phi-3"]);
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
