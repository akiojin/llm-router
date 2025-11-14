//! ログ閲覧エンドポイント

use super::models::AppError;
use crate::logging;
use axum::{extract::Query, Json};
use ollama_coordinator_common::log::{tail_json_logs, LogEntry};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::task;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;

/// クエリパラメーター（ログ取得件数）
#[derive(Debug, Clone, Deserialize)]
pub struct LogQuery {
    /// 取得するログ件数（1-1000）
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// ログ一覧レスポンス
#[derive(Debug, Clone, Serialize)]
pub struct AgentLogResponse {
    /// ログエントリ一覧
    pub entries: Vec<LogEntry>,
    /// ログファイルパス（存在する場合）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn clamp_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_LIMIT)
}

/// GET /logs - エージェントローカルログを取得
pub async fn list_logs(Query(query): Query<LogQuery>) -> Result<Json<AgentLogResponse>, AppError> {
    let path = logging::log_file_path()
        .map_err(|err| AppError::from(format!("Failed to resolve agent log path: {err}")))?;
    let entries = read_logs(path.clone(), clamp_limit(query.limit))
        .await
        .map_err(AppError::from)?;
    Ok(Json(AgentLogResponse {
        entries,
        path: Some(path.display().to_string()),
    }))
}

async fn read_logs(path: PathBuf, limit: usize) -> Result<Vec<LogEntry>, String> {
    task::spawn_blocking(move || tail_json_logs(&path, limit))
        .await
        .map_err(|err| format!("Failed to join log reader: {err}"))?
        .map_err(|err| format!("Failed to read log file: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use serde_json::json;
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[tokio::test]
    async fn list_logs_returns_entries() {
        let _guard = ENV_LOCK.lock().await;
        let temp = tempdir().unwrap();
        std::env::set_var("OLLAMA_AGENT_DATA_DIR", temp.path());
        let path = logging::log_file_path().unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({
                "timestamp": "2025-11-14T00:00:00Z",
                "level": "INFO",
                "target": "agent",
                "fields": { "message": "agent-started" }
            })
        )
        .unwrap();

        let response = list_logs(Query(LogQuery { limit: 10 })).await.unwrap().0;
        assert_eq!(response.entries.len(), 1);
        assert_eq!(
            response.entries[0].message.as_deref(),
            Some("agent-started")
        );
        assert!(response.path.is_some());

        std::env::remove_var("OLLAMA_AGENT_DATA_DIR");
    }
}
