//! OpenAI互換APIエンドポイント (/v1/*)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use ollama_coordinator_common::{
    error::{CommonError, CoordinatorError},
    protocol::{RecordStatus, RequestResponseRecord, RequestType},
};
use serde_json::{json, Value};
use std::time::Instant;
use uuid::Uuid;

use crate::{
    api::{
        agent::AppError,
        proxy::{forward_streaming_response, save_request_record, select_available_agent},
    },
    balancer::RequestOutcome,
    AppState,
};

/// POST /v1/chat/completions - OpenAI互換チャットAPI
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let model = extract_model(&payload)?;
    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/chat/completions",
        model,
        stream,
        RequestType::Chat,
    )
    .await
}

/// POST /v1/completions - OpenAI互換テキスト補完API
pub async fn completions(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let model = extract_model(&payload)?;
    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/completions",
        model,
        stream,
        RequestType::Generate,
    )
    .await
}

/// POST /v1/embeddings - OpenAI互換Embeddings API
pub async fn embeddings(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let model = extract_model(&payload)?;
    proxy_openai_post(
        &state,
        payload,
        "/v1/embeddings",
        model,
        false,
        RequestType::Embeddings,
    )
    .await
}

/// GET /v1/models - モデル一覧取得
pub async fn list_models(State(state): State<AppState>) -> Result<Response, AppError> {
    proxy_openai_get(&state, "/v1/models").await
}

/// GET /v1/models/:id - モデル詳細取得
pub async fn get_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Response, AppError> {
    let path = format!("/v1/models/{}", model_id);
    proxy_openai_get(&state, &path).await
}

fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| validation_error("`model` field is required for OpenAI-compatible requests"))
}

fn extract_stream(payload: &Value) -> bool {
    payload
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

async fn proxy_openai_post(
    state: &AppState,
    payload: Value,
    target_path: &str,
    model: String,
    stream: bool,
    request_type: RequestType,
) -> Result<Response, AppError> {
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = payload.clone();

    let agent = select_available_agent(state).await?;
    let agent_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;

    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}{}",
        agent.ip_address, agent.ollama_port, target_path
    );
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&payload).send().await {
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
                    request_type,
                    model: model.clone(),
                    agent_id,
                    agent_machine_name: agent_machine_name.clone(),
                    agent_ip,
                    client_ip: None,
                    request_body: request_body.clone(),
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to proxy OpenAI request: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            return Err(
                CoordinatorError::Http(format!("Failed to proxy OpenAI request: {}", e)).into(),
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
                request_type,
                model: model.clone(),
                agent_id,
                agent_machine_name: agent_machine_name.clone(),
                agent_ip,
                client_ip: None,
                request_body: request_body.clone(),
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Error {
                    message: message.clone(),
                },
                completed_at: Utc::now(),
            },
        );

        let payload = json!({
            "error": {
                "message": message,
                "type": "ollama_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        return Ok((status_code, Json(payload)).into_response());
    }

    if stream {
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
                request_type,
                model,
                agent_id,
                agent_machine_name,
                agent_ip,
                client_ip: None,
                request_body,
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return forward_streaming_response(response).map_err(AppError::from);
    }

    let parsed = response.json::<Value>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(body) => {
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
                    request_type,
                    model,
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: None,
                    request_body,
                    response_body: Some(body.clone()),
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Success,
                    completed_at: Utc::now(),
                },
            );

            Ok((StatusCode::OK, Json(body)).into_response())
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
                    request_type,
                    model,
                    agent_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: None,
                    request_body,
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("Failed to parse OpenAI response: {}", e),
                    },
                    completed_at: Utc::now(),
                },
            );

            Err(CoordinatorError::Http(format!("Failed to parse OpenAI response: {}", e)).into())
        }
    }
}

async fn proxy_openai_get(state: &AppState, target_path: &str) -> Result<Response, AppError> {
    let agent = select_available_agent(state).await?;
    let agent_id = agent.id;

    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}{}",
        agent.ip_address, agent.ollama_port, target_path
    );
    let start = Instant::now();

    let response = client.get(&ollama_url).send().await.map_err(|e| {
        AppError::from(CoordinatorError::Http(format!(
            "Failed to proxy OpenAI models request: {}",
            e
        )))
    })?;

    let duration = start.elapsed();
    let outcome = if response.status().is_success() {
        RequestOutcome::Success
    } else {
        RequestOutcome::Error
    };
    state
        .load_manager
        .finish_request(agent_id, outcome, duration)
        .await
        .map_err(AppError::from)?;

    if !response.status().is_success() {
        let status = response.status();
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body_bytes = response.bytes().await.unwrap_or_default();
        let message = if body_bytes.is_empty() {
            status.to_string()
        } else {
            String::from_utf8_lossy(&body_bytes).trim().to_string()
        };

        let payload = json!({
            "error": {
                "message": message,
                "type": "ollama_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        return Ok((status_code, Json(payload)).into_response());
    }

    let body = response.json::<Value>().await.map_err(|e| {
        AppError::from(CoordinatorError::Http(format!(
            "Failed to parse OpenAI models response: {}",
            e
        )))
    })?;

    Ok((StatusCode::OK, Json(body)).into_response())
}

fn validation_error(message: impl Into<String>) -> AppError {
    let err = CoordinatorError::Common(CommonError::Validation(message.into()));
    err.into()
}
