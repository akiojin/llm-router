//! ログ閲覧API
//!
//! `/api/dashboard/logs/*` エンドポイントを提供する。

use super::agent::AppError;
use crate::{logging, AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use ollama_coordinator_common::{
    error::{CoordinatorError, CoordinatorResult},
    log::{tail_json_logs, LogEntry},
    types::AgentStatus,
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
    /// ログソース（coordinator / agent:NAME）
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

/// GET /api/dashboard/logs/coordinator
pub async fn get_coordinator_logs(
    Query(query): Query<LogQuery>,
) -> Result<Json<LogResponse>, AppError> {
    let log_path = logging::log_file_path().map_err(|err| {
        CoordinatorError::Internal(format!("Failed to resolve coordinator log path: {err}"))
    })?;
    let entries = read_logs(log_path.clone(), clamp_limit(query.limit)).await?;

    Ok(Json(LogResponse {
        source: "coordinator".to_string(),
        entries,
        path: Some(log_path.display().to_string()),
    }))
}

/// GET /api/dashboard/logs/agents/:agent_id
pub async fn get_agent_logs(
    Path(agent_id): Path<Uuid>,
    Query(query): Query<LogQuery>,
    State(state): State<AppState>,
) -> Result<Json<LogResponse>, AppError> {
    let agent = state.registry.get(agent_id).await?;
    if agent.status != AgentStatus::Online {
        return Err(CoordinatorError::AgentOffline(agent_id).into());
    }

    let limit = clamp_limit(query.limit);
    let agent_api_port = agent.ollama_port.saturating_add(1); // APIポートはOllamaポート+1
    let url = format!(
        "http://{}:{}/api/logs?tail={}",
        agent.ip_address, agent_api_port, limit
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| CoordinatorError::Internal(err.to_string()))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(map_reqwest_error)?
        .error_for_status()
        .map_err(map_reqwest_error)?;

    let agent_logs: LogResponse = response
        .json::<AgentLogPayload>()
        .await
        .map_err(|err| CoordinatorError::Internal(err.to_string()))?
        .into();

    Ok(Json(LogResponse {
        source: format!("agent:{}", agent.machine_name),
        entries: agent_logs.entries,
        path: agent_logs.path,
    }))
}

fn map_reqwest_error(err: reqwest::Error) -> AppError {
    if err.is_timeout() {
        CoordinatorError::Timeout(err.to_string()).into()
    } else {
        CoordinatorError::Http(err.to_string()).into()
    }
}

async fn read_logs(path: PathBuf, limit: usize) -> CoordinatorResult<Vec<LogEntry>> {
    task::spawn_blocking(move || tail_json_logs(&path, limit))
        .await
        .map_err(|err| CoordinatorError::Internal(format!("Failed to join log reader: {err}")))?
        .map_err(|err| CoordinatorError::Internal(format!("Failed to read logs: {err}")))
}

#[derive(Debug, Deserialize)]
struct AgentLogPayload {
    entries: Vec<LogEntry>,
    path: Option<String>,
}

impl From<AgentLogPayload> for LogResponse {
    fn from(value: AgentLogPayload) -> Self {
        Self {
            source: "agent".to_string(),
            entries: value.entries,
            path: value.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::LoadManager, db::test_utils::TEST_LOCK, registry::AgentRegistry,
        tasks::DownloadTaskManager,
    };
    use axum::extract::State as AxumState;
    use ollama_coordinator_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::{net::IpAddr, sync::Arc};
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: Some(8_000_000_000),
        }]
    }

    fn coordinator_state() -> AppState {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history = Arc::new(
            crate::db::request_history::RequestHistoryStorage::new().expect("history init"),
        );
        let task_manager = DownloadTaskManager::new();
        AppState {
            registry,
            load_manager,
            request_history,
            task_manager,
        }
    }

    #[tokio::test]
    async fn coordinator_logs_endpoint_returns_entries() {
        let _guard = TEST_LOCK.lock().await;
        let temp = tempdir().unwrap();
        std::env::set_var("OLLAMA_COORDINATOR_DATA_DIR", temp.path());
        let log_path = logging::log_file_path().unwrap();
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &log_path,
            "{\"timestamp\":\"2025-11-14T00:00:00Z\",\"level\":\"INFO\",\"target\":\"test\",\"fields\":{\"message\":\"hello\"}}\n{\"timestamp\":\"2025-11-14T00:01:00Z\",\"level\":\"ERROR\",\"target\":\"test\",\"fields\":{\"message\":\"world\"}}\n",
        )
        .unwrap();

        let response = get_coordinator_logs(Query(LogQuery { limit: 10 }))
            .await
            .unwrap()
            .0;

        assert_eq!(response.source, "coordinator");
        assert_eq!(response.entries.len(), 2);
        assert_eq!(response.entries[1].message.as_deref(), Some("world"));

        std::env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[tokio::test]
    async fn agent_logs_endpoint_fetches_remote_entries() {
        let _guard = TEST_LOCK.lock().await;
        let mock = MockServer::start().await;
        let agent_port = mock.address().port();
        let agent_ip: IpAddr = mock.address().ip();

        Mock::given(method("GET"))
            .and(path("/api/logs"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"entries":[{"timestamp":"2025-11-14T00:00:00Z","level":"INFO","target":"agent","message":"remote","fields":{}}],"path":"/var/log/agent.log"}"#,
                "application/json",
            ))
            .mount(&mock)
            .await;

        let state = coordinator_state();
        let register_req = RegisterRequest {
            machine_name: "agent-1".to_string(),
            ip_address: agent_ip,
            ollama_version: "0.1.0".to_string(),
            ollama_port: agent_port.saturating_sub(1),
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let register_res = state.registry.register(register_req).await.unwrap();
        let agent_id = register_res.agent_id;

        let response = get_agent_logs(
            Path(agent_id),
            Query(LogQuery { limit: 50 }),
            AxumState(state),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.entries.len(), 1);
        assert_eq!(response.entries[0].message.as_deref(), Some("remote"));
        assert_eq!(response.source, "agent:agent-1");
    }
}
