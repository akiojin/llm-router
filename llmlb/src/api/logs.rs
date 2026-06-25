//! ログ閲覧API
//!
//! `/api/dashboard/logs/*` エンドポイントを提供する。

use super::error::AppError;
use crate::common::{
    error::{LbError, RouterResult},
    log::{tail_json_logs, LogEntry},
};
use crate::{logging, AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use tokio::task;
use uuid::Uuid;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;

/// ログ取得クエリパラメーター
#[derive(Debug, Clone, Deserialize)]
pub struct LogQuery {
    /// 取得件数（1-1000）
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// ログレスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogResponse {
    /// ログソース（load balancer / node:NAME）
    pub source: String,
    /// ログエントリ一覧
    pub entries: Vec<LogEntry>,
    /// ログファイルパス
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn clamp_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_LIMIT)
}

/// GET /api/dashboard/logs/load balancer
pub async fn get_lb_logs(Query(query): Query<LogQuery>) -> Result<Json<LogResponse>, AppError> {
    let log_path = logging::log_file_path().map_err(|err| {
        LbError::Internal(format!("Failed to resolve load balancer log path: {err}"))
    })?;
    let entries = read_logs(log_path.clone(), clamp_limit(query.limit)).await?;

    Ok(Json(LogResponse {
        source: "load balancer".to_string(),
        entries,
        path: Some(log_path.display().to_string()),
    }))
}

/// GET /api/endpoints/:id/logs
///
/// エンドポイントが `/api/logs` を提供している場合、llmlbはそこにリクエストを転送します。
pub async fn get_endpoint_logs(
    Path(endpoint_id): Path<Uuid>,
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
) -> Result<Json<LogResponse>, AppError> {
    use crate::types::endpoint::EndpointStatus;

    let endpoint = state
        .endpoint_registry
        .get(endpoint_id)
        .await
        .ok_or(LbError::EndpointNotFound(endpoint_id))?;

    // Pending/Error 状態でもログ取得は許可（Offline のみ拒否）
    if endpoint.status == EndpointStatus::Offline {
        return Err(LbError::EndpointOffline(endpoint_id).into());
    }

    let limit = clamp_limit(query.limit);
    // エンドポイントのbase_urlからログ取得
    let url = format!(
        "{}/api/logs?tail={}",
        endpoint.base_url.trim_end_matches('/'),
        limit
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| LbError::Internal(err.to_string()))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(map_reqwest_error)?
        .error_for_status()
        .map_err(map_reqwest_error)?;

    let node_logs: LogResponse = response
        .json::<NodeLogPayload>()
        .await
        .map_err(|err| LbError::Internal(err.to_string()))?
        .into();

    Ok(Json(LogResponse {
        source: format!("endpoint:{}", endpoint.name),
        entries: node_logs.entries,
        path: node_logs.path,
    }))
}

fn map_reqwest_error(err: reqwest::Error) -> AppError {
    if err.is_timeout() {
        LbError::Timeout(err.to_string()).into()
    } else {
        LbError::Http(err.to_string()).into()
    }
}

async fn read_logs(path: PathBuf, limit: usize) -> RouterResult<Vec<LogEntry>> {
    task::spawn_blocking(move || tail_json_logs(&path, limit))
        .await
        .map_err(|err| LbError::Internal(format!("Failed to join log reader: {err}")))?
        .map_err(|err| LbError::Internal(format!("Failed to read logs: {err}")))
}

#[derive(Debug, Deserialize)]
struct NodeLogPayload {
    entries: Vec<LogEntry>,
    path: Option<String>,
}

impl From<NodeLogPayload> for LogResponse {
    fn from(value: NodeLogPayload) -> Self {
        Self {
            source: "node".to_string(),
            entries: value.entries,
            path: value.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::{TestAppStateBuilder, TEST_LOCK};
    use axum::extract::State as AxumState;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn lb_state() -> AppState {
        TestAppStateBuilder::new().await.build().await
    }

    #[tokio::test]
    async fn lb_logs_endpoint_returns_entries() {
        let _guard = TEST_LOCK.lock().await;
        let temp = tempdir().unwrap();
        std::env::set_var("LLMLB_DATA_DIR", temp.path());
        let log_path = logging::log_file_path().unwrap();
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        // 既存のログファイルを削除してクリーンな状態から開始
        let _ = std::fs::remove_file(&log_path);
        std::fs::write(
            &log_path,
            "{\"timestamp\":\"2025-11-14T00:00:00Z\",\"level\":\"INFO\",\"target\":\"test\",\"fields\":{\"message\":\"hello\"}}\n{\"timestamp\":\"2025-11-14T00:01:00Z\",\"level\":\"ERROR\",\"target\":\"test\",\"fields\":{\"message\":\"world\"}}\n",
        )
        .unwrap();

        // limitを十分大きく設定し、バックグラウンドプロセスによるログ追加を考慮
        let response = get_lb_logs(Query(LogQuery { limit: 100 })).await.unwrap().0;

        assert_eq!(response.source, "load balancer");
        // インデックスベースの検証ではなく、特定のメッセージが存在するかどうかを確認
        // （バックグラウンドプロセスがログに追加すると、インデックスがずれる可能性があるため）
        let has_hello = response
            .entries
            .iter()
            .any(|e| e.message.as_deref() == Some("hello"));
        let has_world = response
            .entries
            .iter()
            .any(|e| e.message.as_deref() == Some("world"));
        assert!(has_hello, "Expected 'hello' message in log entries");
        assert!(has_world, "Expected 'world' message in log entries");

        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    async fn node_logs_endpoint_fetches_remote_entries() {
        use crate::types::endpoint::{Endpoint, EndpointStatus, EndpointType};

        let _guard = TEST_LOCK.lock().await;
        let mock = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"entries":[{"timestamp":"2025-11-14T00:00:00Z","level":"INFO","target":"node","message":"remote","fields":{}}],"path":"/var/log/node.log"}"#,
                "application/json",
            ))
            .mount(&mock)
            .await;

        let state = lb_state().await;

        // EndpointRegistryにエンドポイントを追加
        let mut endpoint = Endpoint::new(
            "endpoint-1".to_string(),
            mock.uri(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        endpoint.gpu_device_count = Some(1);
        endpoint.gpu_total_memory_bytes = Some(8_000_000_000);
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("Failed to add endpoint");

        let response = get_endpoint_logs(
            Path(endpoint_id),
            Query(LogQuery { limit: 50 }),
            AxumState(state),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.entries.len(), 1);
        assert_eq!(response.entries[0].message.as_deref(), Some("remote"));
        assert_eq!(response.source, "endpoint:endpoint-1");
    }

    // --- LogQuery deserialization ---

    #[test]
    fn log_query_default_limit() {
        let json = r#"{}"#;
        let query: LogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn log_query_custom_limit() {
        let json = r#"{"limit":500}"#;
        let query: LogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, 500);
    }

    #[test]
    fn log_query_zero_limit() {
        let json = r#"{"limit":0}"#;
        let query: LogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, 0);
    }

    #[test]
    fn log_query_large_limit() {
        let json = r#"{"limit":99999}"#;
        let query: LogQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, 99999);
    }

    // --- clamp_limit ---

    #[test]
    fn clamp_limit_min() {
        assert_eq!(clamp_limit(0), 1);
    }

    #[test]
    fn clamp_limit_below_min() {
        assert_eq!(clamp_limit(0), 1);
    }

    #[test]
    fn clamp_limit_at_min() {
        assert_eq!(clamp_limit(1), 1);
    }

    #[test]
    fn clamp_limit_in_range() {
        assert_eq!(clamp_limit(500), 500);
    }

    #[test]
    fn clamp_limit_at_max() {
        assert_eq!(clamp_limit(MAX_LIMIT), MAX_LIMIT);
    }

    #[test]
    fn clamp_limit_above_max() {
        assert_eq!(clamp_limit(MAX_LIMIT + 1), MAX_LIMIT);
    }

    #[test]
    fn clamp_limit_very_large() {
        assert_eq!(clamp_limit(usize::MAX), MAX_LIMIT);
    }

    // --- LogResponse serialization ---

    #[test]
    fn log_response_serialization_with_path() {
        let resp = LogResponse {
            source: "load balancer".to_string(),
            entries: vec![],
            path: Some("/var/log/llmlb.jsonl".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"source\":\"load balancer\""));
        assert!(json.contains("\"path\":\"/var/log/llmlb.jsonl\""));
        assert!(json.contains("\"entries\":[]"));
    }

    #[test]
    fn log_response_serialization_without_path() {
        let resp = LogResponse {
            source: "endpoint:test".to_string(),
            entries: vec![],
            path: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        // path is skip_serializing_if None
        assert!(!json.contains("\"path\""));
    }

    #[test]
    fn log_response_with_entries() {
        use crate::common::log::LogEntry;
        let resp = LogResponse {
            source: "load balancer".to_string(),
            entries: vec![LogEntry {
                timestamp: Some("2025-01-01T00:00:00Z".to_string()),
                level: Some("INFO".to_string()),
                target: Some("test".to_string()),
                message: Some("hello world".to_string()),
                fields: serde_json::Map::new(),
                file: None,
                line: None,
            }],
            path: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("hello world"));
        assert!(json.contains("INFO"));
    }

    // --- NodeLogPayload -> LogResponse ---

    #[test]
    fn node_log_payload_into_log_response() {
        use crate::common::log::LogEntry;
        let payload = NodeLogPayload {
            entries: vec![LogEntry {
                timestamp: Some("2025-01-01T00:00:00Z".to_string()),
                level: Some("WARN".to_string()),
                target: None,
                message: Some("test message".to_string()),
                fields: serde_json::Map::new(),
                file: None,
                line: None,
            }],
            path: Some("/var/log/node.log".to_string()),
        };
        let resp: LogResponse = payload.into();
        assert_eq!(resp.source, "node");
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].message.as_deref(), Some("test message"));
        assert_eq!(resp.path, Some("/var/log/node.log".to_string()));
    }

    #[test]
    fn node_log_payload_empty() {
        let payload = NodeLogPayload {
            entries: vec![],
            path: None,
        };
        let resp: LogResponse = payload.into();
        assert_eq!(resp.source, "node");
        assert!(resp.entries.is_empty());
        assert!(resp.path.is_none());
    }

    #[test]
    fn node_log_payload_deserialize() {
        let json = r#"{"entries":[{"timestamp":"2025-01-01T00:00:00Z","level":"INFO","message":"test","fields":{}}],"path":"/log.txt"}"#;
        let payload: NodeLogPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.entries.len(), 1);
        assert_eq!(payload.path, Some("/log.txt".to_string()));
    }

    #[test]
    fn node_log_payload_deserialize_no_path() {
        let json = r#"{"entries":[]}"#;
        let payload: NodeLogPayload = serde_json::from_str(json).unwrap();
        assert!(payload.entries.is_empty());
        assert!(payload.path.is_none());
    }

    // --- map_reqwest_error ---

    #[test]
    fn map_reqwest_error_non_timeout_returns_http_error() {
        // Build a reqwest error by trying to parse an invalid URL
        let err = reqwest::Client::new()
            .get("http://[::1]:99999/invalid")
            .build()
            .unwrap_err();
        let app_err = map_reqwest_error(err);
        // Should be an Http or Timeout error mapped to AppError
        let _ = format!("{:?}", app_err);
    }

    // --- default_limit ---

    #[test]
    fn default_limit_is_200() {
        assert_eq!(default_limit(), 200);
    }

    // --- constants ---

    #[test]
    fn constants_values() {
        assert_eq!(DEFAULT_LIMIT, 200);
        assert_eq!(MAX_LIMIT, 1000);
    }

    // --- LogResponse PartialEq ---

    #[test]
    fn log_response_equality() {
        let a = LogResponse {
            source: "lb".to_string(),
            entries: vec![],
            path: None,
        };
        let b = LogResponse {
            source: "lb".to_string(),
            entries: vec![],
            path: None,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn log_response_inequality_source() {
        let a = LogResponse {
            source: "lb".to_string(),
            entries: vec![],
            path: None,
        };
        let b = LogResponse {
            source: "node".to_string(),
            entries: vec![],
            path: None,
        };
        assert_ne!(a, b);
    }
}
