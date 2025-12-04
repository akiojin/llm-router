//! データベースアクセス層
//!
//! JSONファイルベースのデータ永続化

pub mod request_history;

/// ユーザー管理
pub mod users;

/// APIキー管理
pub mod api_keys;

/// エージェントトークン管理
pub mod agent_tokens;
pub mod models;

/// データベースマイグレーション
pub mod migrations;

#[cfg(test)]
pub(crate) mod test_utils {
    use once_cell::sync::Lazy;
    use tokio::sync::Mutex as TokioMutex;

    /// テスト用のグローバルロック（環境変数の競合を防ぐ）
    /// db配下のすべてのテストで共有
    pub static TEST_LOCK: Lazy<TokioMutex<()>> = Lazy::new(|| TokioMutex::new(()));
}

use chrono::Utc;
use llm_router_common::{
    error::{RouterError, RouterResult},
    types::Node,
};
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};
use uuid::Uuid;

/// データファイルのパスを取得
fn get_data_file_path() -> RouterResult<PathBuf> {
    // テスト用に環境変数でデータディレクトリを指定可能にする
    let data_dir = if let Ok(test_dir) = std::env::var("LLM_ROUTER_DATA_DIR") {
        PathBuf::from(test_dir)
    } else {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| RouterError::Database("Failed to get home directory".to_string()))?;

        PathBuf::from(home).join(".llm-router")
    };

    Ok(data_dir.join("nodes.json"))
}

/// データディレクトリを初期化
pub async fn init_storage() -> RouterResult<()> {
    let data_file = get_data_file_path()?;
    let data_dir = data_file
        .parent()
        .ok_or_else(|| RouterError::Database("Invalid data file path".to_string()))?;

    // ディレクトリが存在しない場合は作成
    if !data_dir.exists() {
        fs::create_dir_all(data_dir).await.map_err(|e| {
            RouterError::Database(format!("Failed to create data directory: {}", e))
        })?;
    }

    // ファイルが存在しない場合は空の配列を作成
    if !data_file.exists() {
        fs::write(&data_file, "[]")
            .await
            .map_err(|e| RouterError::Database(format!("Failed to initialize data file: {}", e)))?;
    }

    Ok(())
}

/// ノードを保存
pub async fn save_node(node: &Node) -> RouterResult<()> {
    let data_file = get_data_file_path()?;

    // ディレクトリが存在しない場合は作成
    let data_dir = data_file
        .parent()
        .ok_or_else(|| RouterError::Database("Invalid data file path".to_string()))?;

    if !data_dir.exists() {
        fs::create_dir_all(data_dir).await.map_err(|e| {
            RouterError::Database(format!("Failed to create data directory: {}", e))
        })?;
    }

    // 既存のノードを読み込み
    let mut nodes = load_nodes().await?;

    // 同じIDのノードがあれば更新、なければ追加
    if let Some(existing) = nodes.iter_mut().find(|n| n.id == node.id) {
        *existing = node.clone();
    } else {
        nodes.push(node.clone());
    }

    // JSONに変換して保存
    let json = serde_json::to_string_pretty(&nodes)
        .map_err(|e| RouterError::Database(format!("Failed to serialize nodes: {}", e)))?;

    fs::write(&data_file, json)
        .await
        .map_err(|e| RouterError::Database(format!("Failed to write data file: {}", e)))?;

    Ok(())
}

/// 全ノードを読み込み
pub async fn load_nodes() -> RouterResult<Vec<Node>> {
    let data_file = get_data_file_path()?;

    // ファイルが存在しない場合は空の配列を返す
    if !data_file.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&data_file)
        .await
        .map_err(|e| RouterError::Database(format!("Failed to read data file: {}", e)))?;

    // 空のファイルの場合は空の配列を返す
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Vec<Node>>(&content) {
        Ok(nodes) => Ok(nodes),
        Err(err) => {
            warn!(
                "Detected corrupted nodes.json, attempting recovery: {}",
                err
            );
            recover_corrupted_nodes_file(&data_file).await?;
            // 再読込して返す (空なら空配列)
            let reopened = fs::read_to_string(&data_file).await.unwrap_or_default();
            if reopened.trim().is_empty() {
                return Ok(Vec::new());
            }
            match serde_json::from_str::<Vec<Node>>(&reopened) {
                Ok(nodes2) => Ok(nodes2),
                Err(_) => Ok(Vec::new()),
            }
        }
    }
}

/// ノードを削除
pub async fn delete_node(node_id: Uuid) -> RouterResult<()> {
    let data_file = get_data_file_path()?;

    // 既存のノードを読み込み
    let mut nodes = load_nodes().await?;

    // 指定されたIDのノードを削除
    nodes.retain(|n| n.id != node_id);

    // JSONに変換して保存
    let json = serde_json::to_string_pretty(&nodes)
        .map_err(|e| RouterError::Database(format!("Failed to serialize nodes: {}", e)))?;

    fs::write(&data_file, json)
        .await
        .map_err(|e| RouterError::Database(format!("Failed to write data file: {}", e)))?;

    Ok(())
}

/// 破損した nodes.json をバックアップして空のファイルに復旧
async fn recover_corrupted_nodes_file(data_file: &PathBuf) -> RouterResult<()> {
    if !data_file.exists() {
        // ファイルが存在しない場合は新規作成のみ行う
        fs::write(data_file, "[]")
            .await
            .map_err(|e| RouterError::Database(format!("Failed to initialize data file: {}", e)))?;
        return Ok(());
    }

    let backup_name = format!("nodes.json.corrupted-{}", Utc::now().format("%Y%m%d%H%M%S"));
    let parent_dir = data_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let backup_path = parent_dir.join(backup_name);

    if let Err(rename_err) = fs::rename(data_file, &backup_path).await {
        warn!(
            "Failed to move corrupted nodes.json: {}. Attempting to overwrite with empty file.",
            rename_err
        );
    } else {
        info!(
            "Moved corrupted nodes.json to backup: {}",
            backup_path.display()
        );
    }

    fs::write(data_file, "[]")
        .await
        .map_err(|e| RouterError::Database(format!("Failed to reset data file: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use llm_router_common::types::{GpuDeviceInfo, NodeStatus};
    use std::net::IpAddr;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_init_storage() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", _guard.path());

        let result = init_storage().await;
        assert!(result.is_ok());

        // ファイルが作成されていることを確認
        let data_file = get_data_file_path().unwrap();
        assert!(data_file.exists());

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_save_and_load_node() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let now = Utc::now();
        let node = Node {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            status: NodeStatus::Online,
            registered_at: now,
            last_seen: now,
            online_since: Some(now),
            custom_name: None,
            tags: Vec::new(),
            notes: None,
            loaded_models: Vec::new(),
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_available: true,
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            agent_api_port: Some(11435),
            initializing: false,
            ready_models: None,
        };

        // 保存
        save_node(&node).await.unwrap();

        // 読み込み
        let loaded_nodes = load_nodes().await.unwrap();
        assert_eq!(loaded_nodes.len(), 1);
        assert_eq!(loaded_nodes[0].id, node.id);
        assert_eq!(loaded_nodes[0].machine_name, node.machine_name);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_delete_node() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let now = Utc::now();
        let node = Node {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            status: NodeStatus::Online,
            registered_at: now,
            last_seen: now,
            online_since: Some(now),
            custom_name: None,
            tags: Vec::new(),
            notes: None,
            loaded_models: Vec::new(),
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_available: true,
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            agent_api_port: Some(11435),
            initializing: false,
            ready_models: None,
        };

        save_node(&node).await.unwrap();
        delete_node(node.id).await.unwrap();

        let loaded_nodes = load_nodes().await.unwrap();
        assert_eq!(loaded_nodes.len(), 0);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_update_existing_node() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let node_id = Uuid::new_v4();
        let now = Utc::now();
        let node1 = Node {
            id: node_id,
            machine_name: "test-machine-1".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            status: NodeStatus::Online,
            registered_at: now,
            last_seen: now,
            online_since: Some(now),
            custom_name: None,
            tags: Vec::new(),
            notes: None,
            loaded_models: Vec::new(),
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_available: true,
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            agent_api_port: Some(11435),
            initializing: false,
            ready_models: None,
        };

        save_node(&node1).await.unwrap();

        // 同じIDで異なる内容のノードを保存
        let updated = Utc::now();
        let node2 = Node {
            id: node_id,
            machine_name: "test-machine-2".to_string(),
            ip_address: "192.168.1.101".parse::<IpAddr>().unwrap(),
            runtime_version: "0.2.0".to_string(),
            runtime_port: 11435,
            status: NodeStatus::Offline,
            registered_at: updated,
            last_seen: updated,
            online_since: None,
            custom_name: Some("Updated".into()),
            tags: vec!["primary".into()],
            notes: None,
            loaded_models: vec!["gpt-oss:7b".into()],
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_available: true,
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            agent_api_port: Some(11435),
            initializing: false,
            ready_models: None,
        };

        save_node(&node2).await.unwrap();

        // 読み込んで更新されていることを確認
        let loaded_nodes = load_nodes().await.unwrap();
        assert_eq!(loaded_nodes.len(), 1);
        assert_eq!(loaded_nodes[0].machine_name, "test-machine-2");
        assert_eq!(loaded_nodes[0].runtime_port, 11435);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_load_nodes_recovers_from_corrupted_file() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        let _guard = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let data_path = get_data_file_path().unwrap();
        fs::write(&data_path, b"{invalid json").await.unwrap();

        let nodes = load_nodes().await.unwrap();
        assert!(nodes.is_empty());

        let entries = std::fs::read_dir(_guard.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert!(
            entries
                .iter()
                .any(|name| name.starts_with("nodes.json.corrupted-")),
            "Expected corrupted backup file to be created, found: {:?}",
            entries
        );

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }
}
