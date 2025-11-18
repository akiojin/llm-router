//! ログ閲覧API (Node側)

use crate::logging;
use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use ollama_router_common::log::tail_json_logs;
use serde::{Deserialize, Serialize};
use tokio::task;

const DEFAULT_TAIL: usize = 200;
const MAX_TAIL: usize = 2000;

/// ログ取得のクエリパラメータ
#[derive(Debug, Deserialize)]
pub struct LogQuery {
    #[serde(default = "default_tail")]
    /// 末尾から取得する行数
    pub tail: usize,
}

fn default_tail() -> usize {
    DEFAULT_TAIL
}

fn clamp_tail(tail: usize) -> usize {
    tail.clamp(1, MAX_TAIL)
}

/// ログ取得レスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct LogResponse {
    /// 取得したログエントリ
    pub entries: Vec<ollama_router_common::log::LogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// 対象ログファイルパス（存在しない場合は None）
    pub path: Option<String>,
}

/// GET /api/logs?tail=N
pub async fn list_logs(Query(query): Query<LogQuery>) -> impl IntoResponse {
    let tail = clamp_tail(query.tail);
    let log_path = match logging::log_file_path() {
        Ok(path) => path,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(LogResponse {
                    entries: vec![],
                    path: None,
                }),
            );
        }
    };

    let entries = match read_logs(log_path.clone(), tail).await {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to read logs");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LogResponse {
                    entries: vec![],
                    path: Some(log_path.display().to_string()),
                }),
            );
        }
    };

    (
        StatusCode::OK,
        Json(LogResponse {
            entries,
            path: Some(log_path.display().to_string()),
        }),
    )
}

async fn read_logs(
    path: std::path::PathBuf,
    tail: usize,
) -> Result<Vec<ollama_router_common::log::LogEntry>, String> {
    task::spawn_blocking(move || tail_json_logs(&path, tail))
        .await
        .map_err(|e| format!("join error: {e}"))?
        .map_err(|e| format!("read error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    #[serial]
    async fn returns_empty_when_missing() {
        // override data dir so log file is absent
        let tmp = tempdir().unwrap();
        std::env::set_var("OLLAMA_NODE_DATA_DIR", tmp.path());

        let res = list_logs(Query(LogQuery { tail: 10 }))
            .await
            .into_response();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body: LogResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(body.entries.is_empty());

        std::env::remove_var("OLLAMA_NODE_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn tails_logs() {
        let tmp = tempdir().unwrap();
        std::env::set_var("OLLAMA_NODE_DATA_DIR", tmp.path());
        let log_path = logging::log_file_path().unwrap();
        fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        fs::write(
            &log_path,
            "{\"timestamp\":\"2025-11-17T00:00:00Z\",\"level\":\"INFO\",\"target\":\"t\",\"fields\":{\"message\":\"first\"}}\n{\"timestamp\":\"2025-11-17T00:01:00Z\",\"level\":\"INFO\",\"target\":\"t\",\"fields\":{\"message\":\"second\"}}\n",
        )
        .unwrap();

        let res = list_logs(Query(LogQuery { tail: 1 })).await.into_response();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body: LogResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.entries.len(), 1);
        assert_eq!(body.entries[0].message.as_deref(), Some("second"));

        std::env::remove_var("OLLAMA_NODE_DATA_DIR");
    }
}
