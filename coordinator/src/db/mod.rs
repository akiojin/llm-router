//! データベースアクセス層
//!
//! JSONファイルベースのデータ永続化

pub mod request_history;

#[cfg(test)]
pub(crate) mod test_utils {
    use once_cell::sync::Lazy;
    use tokio::sync::Mutex as TokioMutex;

    /// テスト用のグローバルロック（環境変数の競合を防ぐ）
    /// db配下のすべてのテストで共有
    pub static TEST_LOCK: Lazy<TokioMutex<()>> = Lazy::new(|| TokioMutex::new(()));
}

use chrono::Utc;
use ollama_coordinator_common::{
    error::{CoordinatorError, CoordinatorResult},
    types::Agent,
};
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};
use uuid::Uuid;

/// データファイルのパスを取得
fn get_data_file_path() -> CoordinatorResult<PathBuf> {
    // テスト用に環境変数でデータディレクトリを指定可能にする
    let data_dir = if let Ok(test_dir) = std::env::var("OLLAMA_COORDINATOR_DATA_DIR") {
        PathBuf::from(test_dir)
    } else {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| CoordinatorError::Database("Failed to get home directory".to_string()))?;

        PathBuf::from(home).join(".ollama-coordinator")
    };

    Ok(data_dir.join("agents.json"))
}

/// データディレクトリを初期化
pub async fn init_storage() -> CoordinatorResult<()> {
    let data_file = get_data_file_path()?;
    let data_dir = data_file
        .parent()
        .ok_or_else(|| CoordinatorError::Database("Invalid data file path".to_string()))?;

    // ディレクトリが存在しない場合は作成
    if !data_dir.exists() {
        fs::create_dir_all(data_dir).await.map_err(|e| {
            CoordinatorError::Database(format!("Failed to create data directory: {}", e))
        })?;
    }

    // ファイルが存在しない場合は空の配列を作成
    if !data_file.exists() {
        fs::write(&data_file, "[]").await.map_err(|e| {
            CoordinatorError::Database(format!("Failed to initialize data file: {}", e))
        })?;
    }

    Ok(())
}

/// エージェントを保存
pub async fn save_agent(agent: &Agent) -> CoordinatorResult<()> {
    let data_file = get_data_file_path()?;

    // ディレクトリが存在しない場合は作成
    let data_dir = data_file
        .parent()
        .ok_or_else(|| CoordinatorError::Database("Invalid data file path".to_string()))?;

    if !data_dir.exists() {
        fs::create_dir_all(data_dir).await.map_err(|e| {
            CoordinatorError::Database(format!("Failed to create data directory: {}", e))
        })?;
    }

    // 既存のエージェントを読み込み
    let mut agents = load_agents().await?;

    // 同じIDのエージェントがあれば更新、なければ追加
    if let Some(existing) = agents.iter_mut().find(|a| a.id == agent.id) {
        *existing = agent.clone();
    } else {
        agents.push(agent.clone());
    }

    // JSONに変換して保存
    let json = serde_json::to_string_pretty(&agents)
        .map_err(|e| CoordinatorError::Database(format!("Failed to serialize agents: {}", e)))?;

    fs::write(&data_file, json)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to write data file: {}", e)))?;

    Ok(())
}

/// 全エージェントを読み込み
pub async fn load_agents() -> CoordinatorResult<Vec<Agent>> {
    let data_file = get_data_file_path()?;

    // ファイルが存在しない場合は空の配列を返す
    if !data_file.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&data_file)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to read data file: {}", e)))?;

    // 空のファイルの場合は空の配列を返す
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Vec<Agent>>(&content) {
        Ok(agents) => Ok(agents),
        Err(err) => {
            warn!(
                "Detected corrupted agents.json, attempting recovery: {}",
                err
            );
            recover_corrupted_agents_file(&data_file).await?;
            Ok(Vec::new())
        }
    }
}

/// エージェントを削除
pub async fn delete_agent(agent_id: Uuid) -> CoordinatorResult<()> {
    let data_file = get_data_file_path()?;

    // 既存のエージェントを読み込み
    let mut agents = load_agents().await?;

    // 指定されたIDのエージェントを削除
    agents.retain(|a| a.id != agent_id);

    // JSONに変換して保存
    let json = serde_json::to_string_pretty(&agents)
        .map_err(|e| CoordinatorError::Database(format!("Failed to serialize agents: {}", e)))?;

    fs::write(&data_file, json)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to write data file: {}", e)))?;

    Ok(())
}

/// 破損した agents.json をバックアップして空のファイルに復旧
async fn recover_corrupted_agents_file(data_file: &PathBuf) -> CoordinatorResult<()> {
    if !data_file.exists() {
        // ファイルが存在しない場合は新規作成のみ行う
        fs::write(data_file, "[]").await.map_err(|e| {
            CoordinatorError::Database(format!("Failed to initialize data file: {}", e))
        })?;
        return Ok(());
    }

    let backup_name = format!(
        "agents.json.corrupted-{}",
        Utc::now().format("%Y%m%d%H%M%S")
    );
    let parent_dir = data_file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let backup_path = parent_dir.join(backup_name);

    if let Err(rename_err) = fs::rename(data_file, &backup_path).await {
        warn!(
            "Failed to move corrupted agents.json: {}. Attempting to overwrite with empty file.",
            rename_err
        );
    } else {
        info!(
            "Moved corrupted agents.json to backup: {}",
            backup_path.display()
        );
    }

    fs::write(data_file, "[]")
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to reset data file: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ollama_coordinator_common::types::{AgentStatus, GpuDeviceInfo};
    use std::net::IpAddr;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_init_storage() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", _guard.path());

        let result = init_storage().await;
        assert!(result.is_ok());

        // ファイルが作成されていることを確認
        let data_file = get_data_file_path().unwrap();
        assert!(data_file.exists());

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[tokio::test]
    async fn test_save_and_load_agent() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let now = Utc::now();
        let agent = Agent {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
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
        save_agent(&agent).await.unwrap();

        // 読み込み
        let loaded_agents = load_agents().await.unwrap();
        assert_eq!(loaded_agents.len(), 1);
        assert_eq!(loaded_agents[0].id, agent.id);
        assert_eq!(loaded_agents[0].machine_name, agent.machine_name);

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[tokio::test]
    async fn test_delete_agent() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let now = Utc::now();
        let agent = Agent {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
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

        save_agent(&agent).await.unwrap();
        delete_agent(agent.id).await.unwrap();

        let loaded_agents = load_agents().await.unwrap();
        assert_eq!(loaded_agents.len(), 0);

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[tokio::test]
    async fn test_update_existing_agent() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        // 一時ディレクトリを使用（_guardでスコープ内保持）
        let _guard = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let agent_id = Uuid::new_v4();
        let now = Utc::now();
        let agent1 = Agent {
            id: agent_id,
            machine_name: "test-machine-1".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
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

        save_agent(&agent1).await.unwrap();

        // 同じIDで異なる内容のエージェントを保存
        let updated = Utc::now();
        let agent2 = Agent {
            id: agent_id,
            machine_name: "test-machine-2".to_string(),
            ip_address: "192.168.1.101".parse::<IpAddr>().unwrap(),
            ollama_version: "0.2.0".to_string(),
            ollama_port: 11435,
            status: AgentStatus::Offline,
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

        save_agent(&agent2).await.unwrap();

        // 読み込んで更新されていることを確認
        let loaded_agents = load_agents().await.unwrap();
        assert_eq!(loaded_agents.len(), 1);
        assert_eq!(loaded_agents[0].machine_name, "test-machine-2");
        assert_eq!(loaded_agents[0].ollama_port, 11435);

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[tokio::test]
    async fn test_load_agents_recovers_from_corrupted_file() {
        let _lock = test_utils::TEST_LOCK.lock().await;

        let _guard = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", _guard.path());

        init_storage().await.unwrap();

        let data_path = get_data_file_path().unwrap();
        fs::write(&data_path, b"{invalid json").await.unwrap();

        let agents = load_agents().await.unwrap();
        assert!(agents.is_empty());

        let entries = std::fs::read_dir(_guard.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert!(
            entries
                .iter()
                .any(|name| name.starts_with("agents.json.corrupted-")),
            "Expected corrupted backup file to be created, found: {:?}",
            entries
        );

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }
}
