//! Ollamaプロキシ APIハンドラー
//! Ollamaプロキシ APIハンドラー

use crate::{api::agent::AppError, balancer::RequestOutcome, AppState};
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use futures::TryStreamExt;
use ollama_coordinator_common::{
    error::CoordinatorError,
    protocol::{
        ChatRequest, ChatResponse, GenerateRequest, RecordStatus, RequestResponseRecord,
        RequestType,
    },
    types::AgentStatus,
};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

const DEFAULT_MAX_WAITERS: usize = 1024;

#[inline]
fn max_waiters() -> usize {
    #[cfg(test)]
    if let Ok(val) = std::env::var("COORDINATOR_MAX_WAITERS") {
        if let Ok(parsed) = val.parse::<usize>() {
            return parsed;
        }
    }

    DEFAULT_MAX_WAITERS
}

/// POST /api/chat - Ollama Chat APIプロキシ
pub async fn proxy_chat(
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, AppError> {
    proxy_chat_with_handlers(
        &state,
        req,
        client_addr.ip(),
        |response, _| forward_streaming_response(response).map_err(AppError::from),
        |payload, _| Ok((StatusCode::OK, Json(payload)).into_response()),
    )
    .await
}

/// POST /api/generate - Ollama Generate APIプロキシ
pub async fn proxy_generate(
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Response, AppError> {
    proxy_generate_with_handlers(
        &state,
        req,
        client_addr.ip(),
        |response, _| forward_streaming_response(response).map_err(AppError::from),
        |payload, _| Ok((StatusCode::OK, Json(payload)).into_response()),
    )
    .await
}

/// 汎用チャットプロキシ（成功時のレスポンス生成をカスタム可能）
pub(crate) async fn proxy_chat_with_handlers<S, C>(
    state: &AppState,
    req: ChatRequest,
    client_ip: IpAddr,
    stream_handler: S,
    completion_handler: C,
) -> Result<Response, AppError>
where
    S: FnOnce(reqwest::Response, &ChatRequest) -> Result<Response, AppError>,
    C: FnOnce(ChatResponse, &ChatRequest) -> Result<Response, AppError>,
{
    // 全エージェントが初期化中なら ready 出現を待つ（待機者上限で 503）
    if state.load_manager.all_initializing().await {
        let _ = state
            .load_manager
            .record_request_history(RequestOutcome::Queued, Utc::now())
            .await;
        if !state.load_manager.wait_for_ready(max_waiters()).await {
            return Err(CoordinatorError::ServiceUnavailable(
                "All agents are warming up models".into(),
            )
            .into());
        }
    }
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = serde_json::to_value(&req).unwrap_or_default();

    let agent = select_available_agent_for_model(state, &req.model).await?;
    if agent.initializing {
        return Err(CoordinatorError::ServiceUnavailable(
            "All agents are warming up models".into(),
        )
        .into());
    }
    let agent_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;
    let agent_api_port = agent.ollama_port + 1;

    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}/v1/chat/completions",
        agent.ip_address, agent_api_port
    );
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&req).send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Chat,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name: agent_machine_name.clone(),
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body: request_body.clone(),
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to proxy chat request: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            return Err(
                CoordinatorError::Http(format!("Failed to proxy chat request: {}", e)).into(),
            );
        }
    };

    if !response.status().is_success() {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;

        let status = response.status();
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body_bytes = response.bytes().await.unwrap_or_default();
        let message = if body_bytes.is_empty() {
            status.to_string()
        } else {
            String::from_utf8_lossy(&body_bytes).trim().to_string()
        };

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type: RequestType::Chat,
                model: req.model.clone(),
                agent_id,
                agent_machine_name: agent_machine_name.clone(),
                agent_ip,
                client_ip: Some(client_ip),
                request_body: request_body.clone(),
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Error {
                    message: message.clone(),
                },
                completed_at: Utc::now(),
            },
        );

        let payload = serde_json::json!({
            "error": {
                "message": message,
                "type": "ollama_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        return Ok((status_code, Json(payload)).into_response());
    }

    if req.stream {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type: RequestType::Chat,
                model: req.model.clone(),
                agent_id,
                agent_machine_name,
                agent_ip,
                client_ip: Some(client_ip),
                request_body,
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return stream_handler(response, &req);
    }

    let parsed = response.json::<ChatResponse>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(payload) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Success, duration)
                .await
                .map_err(AppError::from)?;

            let response_body = serde_json::to_value(&payload).ok();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Chat,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body,
                    response_body,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Success,
                    completed_at: Utc::now(),
                },
            );

            completion_handler(payload, &req)
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Chat,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body,
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to parse chat response: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            Err(CoordinatorError::Http(format!("Failed to parse chat response: {}", e)).into())
        }
    }
}

/// 汎用Generateプロキシ（成功レスポンスをカスタム可能）
pub(crate) async fn proxy_generate_with_handlers<S, C>(
    state: &AppState,
    req: GenerateRequest,
    client_ip: IpAddr,
    stream_handler: S,
    completion_handler: C,
) -> Result<Response, AppError>
where
    S: FnOnce(reqwest::Response, &GenerateRequest) -> Result<Response, AppError>,
    C: FnOnce(serde_json::Value, &GenerateRequest) -> Result<Response, AppError>,
{
    if state.load_manager.all_initializing().await {
        let _ = state
            .load_manager
            .record_request_history(RequestOutcome::Queued, Utc::now())
            .await;
        if !state.load_manager.wait_for_ready(max_waiters()).await {
            return Err(CoordinatorError::ServiceUnavailable(
                "All agents are warming up models".into(),
            )
            .into());
        }
    }
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = serde_json::to_value(&req).unwrap_or_default();

    let agent = select_available_agent_for_model(state, &req.model).await?;
    let agent_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;

    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let agent_api_port = agent.ollama_port + 1;
    let ollama_url = format!(
        "http://{}:{}/v1/completions",
        agent.ip_address, agent_api_port
    );
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&req).send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Generate,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name: agent_machine_name.clone(),
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body: request_body.clone(),
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to proxy generate request: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            return Err(
                CoordinatorError::Http(format!("Failed to proxy generate request: {}", e)).into(),
            );
        }
    };

    if !response.status().is_success() {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;

        let status = response.status();
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body_bytes = response.bytes().await.unwrap_or_default();
        let message = if body_bytes.is_empty() {
            status.to_string()
        } else {
            String::from_utf8_lossy(&body_bytes).trim().to_string()
        };

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type: RequestType::Generate,
                model: req.model.clone(),
                agent_id,
                agent_machine_name: agent_machine_name.clone(),
                agent_ip,
                client_ip: Some(client_ip),
                request_body: request_body.clone(),
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Error {
                    message: message.clone(),
                },
                completed_at: Utc::now(),
            },
        );

        let payload = serde_json::json!({
            "error": {
                "message": message,
                "type": "ollama_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        return Ok((status_code, Json(payload)).into_response());
    }

    if req.stream {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type: RequestType::Generate,
                model: req.model.clone(),
                agent_id,
                agent_machine_name,
                agent_ip,
                client_ip: Some(client_ip),
                request_body,
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return stream_handler(response, &req);
    }

    let parsed = response.json::<serde_json::Value>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(payload) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Success, duration)
                .await
                .map_err(AppError::from)?;

            let response_body = Some(payload.clone());
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Generate,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body,
                    response_body,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Success,
                    completed_at: Utc::now(),
                },
            );

            completion_handler(payload, &req)
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Generate,
                    model: req.model.clone(),
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: Some(client_ip),
                    request_body,
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to parse generate response: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            Err(CoordinatorError::Http(format!("Failed to parse generate response: {}", e)).into())
        }
    }
}

/// 利用可能なエージェントを選択
///
/// 環境変数LOAD_BALANCER_MODEで動作モードを切り替え:
/// - "metrics": メトリクスベース選択（T014-T015）
/// - その他（デフォルト）: 既存の高度なロードバランシング
pub(crate) async fn select_available_agent_for_model(
    state: &AppState,
    model: &str,
) -> Result<ollama_coordinator_common::types::Agent, CoordinatorError> {
    let _mode = std::env::var("LOAD_BALANCER_MODE").unwrap_or_else(|_| "auto".to_string());

    // まずモデルを保持しているオンラインエージェントを優先
    let required = model.trim().to_lowercase();
    let agents = state.registry.list().await;
    let mut candidates: Vec<_> = agents
        .into_iter()
        .filter(|a| {
            a.status == AgentStatus::Online && a.loaded_models.iter().any(|m| m == &required)
        })
        .collect();

    if candidates.is_empty() {
        // 既存の挙動にフォールバック
        return select_available_agent(state).await;
    }

    // 簡易: 最終確認が新しい順で選択
    candidates.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    Ok(candidates.remove(0))
}

pub(crate) async fn select_available_agent(
    state: &AppState,
) -> Result<ollama_coordinator_common::types::Agent, CoordinatorError> {
    let mode = std::env::var("LOAD_BALANCER_MODE").unwrap_or_else(|_| "auto".to_string());

    match mode.as_str() {
        "metrics" => {
            // メトリクスベース選択（T014-T015で実装）
            state.load_manager.select_agent_by_metrics().await
        }
        _ => {
            // デフォルト: 既存の高度なロードバランシング
            let agent = state.load_manager.select_agent().await?;
            if agent.initializing {
                return Err(CoordinatorError::ServiceUnavailable(
                    "All agents are warming up models".into(),
                ));
            }
            Ok(agent)
        }
    }
}

pub(crate) fn forward_streaming_response(
    response: reqwest::Response,
) -> Result<Response, CoordinatorError> {
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response.bytes_stream().map_err(io::Error::other);
    let body = Body::from_stream(stream);
    let mut axum_response = Response::new(body);
    *axum_response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);
    {
        let response_headers = axum_response.headers_mut();
        for (name, value) in headers.iter() {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                response_headers.insert(header_name, header_value);
            }
        }
    }
    use axum::http::header;
    if !axum_response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").starts_with("text/event-stream"))
        .unwrap_or(false)
    {
        axum_response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    Ok(axum_response)
}

/// リクエスト/レスポンスレコードを保存（Fire-and-forget）
pub(crate) fn save_request_record(
    storage: Arc<crate::db::request_history::RequestHistoryStorage>,
    record: RequestResponseRecord,
) {
    tokio::spawn(async move {
        if let Err(e) = storage.save_record(&record).await {
            tracing::error!("Failed to save request record: {}", e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate},
        registry::AgentRegistry,
        tasks::DownloadTaskManager,
    };
    use ollama_coordinator_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::net::IpAddr;
    use std::time::Duration;

    fn create_test_state() -> AppState {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history =
            std::sync::Arc::new(crate::db::request_history::RequestHistoryStorage::new().unwrap());
        let task_manager = DownloadTaskManager::new();
        AppState {
            registry,
            load_manager,
            request_history,
            task_manager,
        }
    }

    async fn mark_ready(state: &AppState, agent_id: Uuid) {
        // レジストリ側のフラグも更新し、ロードバランサが初期化完了と判断できるようにする
        state
            .registry
            .update_last_seen(agent_id, None, None, None, None, Some(false), Some((4, 4)))
            .await
            .ok();

        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                average_response_time_ms: Some(1.0),
                initializing: false,
                ready_models: Some((4, 4)),
            })
            .await
            .ok();
    }

    #[tokio::test]
    async fn test_select_available_agent_no_agents() {
        let state = create_test_state();
        let result = select_available_agent(&state).await;
        assert!(matches!(result, Err(CoordinatorError::NoAgentsAvailable)));
    }

    #[tokio::test]
    async fn test_select_available_agent_success() {
        let state = create_test_state();

        // エージェントを登録
        let register_req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        state.registry.register(register_req).await.unwrap();

        // mark as ready so load balancer can pick
        let agents = state.registry.list().await;
        mark_ready(&state, agents[0].id).await;

        let result = select_available_agent(&state).await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.machine_name, "test-machine");
    }

    #[tokio::test]
    async fn test_select_available_agent_skips_offline() {
        let state = create_test_state();

        // エージェント1を登録
        let register_req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let response1 = state.registry.register(register_req1).await.unwrap();

        // エージェント1をオフラインにする
        state
            .registry
            .mark_offline(response1.agent_id)
            .await
            .unwrap();

        // エージェント2を登録
        let register_req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let response2 = state.registry.register(register_req2).await.unwrap();

        // mark second agent ready
        mark_ready(&state, response2.agent_id).await;

        let result = select_available_agent(&state).await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.machine_name, "machine2");
    }

    #[tokio::test]
    async fn proxy_chat_waits_until_agent_ready_then_succeeds() {
        use axum::{routing::post, Json, Router};
        use ollama_coordinator_common::protocol::{ChatMessage, ChatRequest, ChatResponse};
        use tokio::{net::TcpListener, sync::oneshot, time::timeout};

        // ---- stub agent (OpenAI互換API) ----
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let agent_router = Router::new().route(
            "/api/chat",
            post(|Json(_req): Json<ChatRequest>| async {
                let resp = ChatResponse {
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "ready".into(),
                    },
                    done: true,
                };
                (StatusCode::OK, Json(resp))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agent_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                agent_router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        });

        // ---- coordinator state ----
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history =
            std::sync::Arc::new(crate::db::request_history::RequestHistoryStorage::new().unwrap());
        let task_manager = crate::tasks::DownloadTaskManager::new();
        let state = AppState {
            registry,
            load_manager,
            request_history,
            task_manager,
        };

        // register agent (ollama_port = APIポート-1と報告)
        let register_req = RegisterRequest {
            machine_name: "ready-agent".into(),
            ip_address: agent_addr.ip(),
            ollama_version: "0.0.0-test".into(),
            ollama_port: agent_addr.port().saturating_sub(1),
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: Some(16_000_000_000),
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let reg = state.registry.register(register_req).await.unwrap();
        let agent_id = reg.agent_id;

        // all initializing
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 0.0,
                memory_usage: 0.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 0,
                average_response_time_ms: None,
                initializing: true,
                ready_models: Some((0, 5)),
            })
            .await
            .unwrap();

        // after a short delay, mark as ready to unblock wait_for_ready
        let lm = state.load_manager.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let _ = lm
                .record_metrics(MetricsUpdate {
                    agent_id,
                    cpu_usage: 0.0,
                    memory_usage: 0.0,
                    gpu_usage: None,
                    gpu_memory_usage: None,
                    gpu_memory_total_mb: None,
                    gpu_memory_used_mb: None,
                    gpu_temperature: None,
                    gpu_model_name: None,
                    gpu_compute_capability: None,
                    gpu_capability_score: None,
                    active_requests: 0,
                    average_response_time_ms: Some(10.0),
                    initializing: false,
                    ready_models: Some((5, 5)),
                })
                .await;
        });

        // exercise proxy (non-streaming)
        let req = ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            stream: false,
        };

        let response = timeout(
            Duration::from_secs(2),
            proxy_chat_with_handlers(
                &state,
                req,
                "127.0.0.1".parse().unwrap(),
                |resp, _| forward_streaming_response(resp).map_err(AppError::from),
                |payload, _| Ok((StatusCode::OK, Json(payload)).into_response()),
            ),
        )
        .await
        .expect("proxy should not time out")
        .expect("proxy should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        // shutdown stub
        let _ = shutdown_tx.send(());
    }
}
