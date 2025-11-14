//! Ollamaプロキシ APIハンドラー
//! Ollamaプロキシ APIハンドラー

use crate::{api::agent::AppError, balancer::RequestOutcome, AppState};
use axum::{
    body::Body,
    extract::State,
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
};
use std::{io, sync::Arc, time::Instant};
use uuid::Uuid;

/// POST /api/chat - Ollama Chat APIプロキシ
pub async fn proxy_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, AppError> {
    // リクエスト履歴用の情報を記録
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = serde_json::to_value(&req).unwrap_or_default();

    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;
    let agent_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;

    // リクエスト開始を記録
    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!("http://{}:{}/api/chat", agent.ip_address, agent.ollama_port);
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

            // エラーを記録
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

        // エラーレスポンスを記録
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

    let stream_enabled = req.stream;

    if stream_enabled {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        // ストリーミングレスポンスを記録（レスポンスボディはNone）
        // TODO: T021 - 将来的にバッファリングして完全なレスポンスを保存
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
                request_body,
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return forward_streaming_response(response).map_err(AppError::from);
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

            // 成功レスポンスを記録
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
                    request_body,
                    response_body,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Success,
                    completed_at: Utc::now(),
                },
            );

            Ok((StatusCode::OK, Json(payload)).into_response())
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            // パースエラーを記録
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Chat,
                    model: req.model,
                    agent_id,
                    agent_machine_name,
                    agent_ip,
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

/// POST /api/generate - Ollama Generate APIプロキシ
pub async fn proxy_generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Response, AppError> {
    // リクエスト履歴用の情報を記録
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = serde_json::to_value(&req).unwrap_or_default();

    // 利用可能なエージェントを選択
    let agent = select_available_agent(&state).await?;
    let agent_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;

    // リクエスト開始を記録
    state
        .load_manager
        .begin_request(agent_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}/api/generate",
        agent.ip_address, agent.ollama_port
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

            // エラーを記録
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

        // エラーレスポンスを記録
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

    let stream_enabled = req.stream;

    if stream_enabled {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(agent_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        // ストリーミングレスポンスを記録（レスポンスボディはNone）
        // TODO: T021 - 将来的にバッファリングして完全なレスポンスを保存
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
                request_body,
                response_body: None,
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return forward_streaming_response(response).map_err(AppError::from);
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

            // 成功レスポンスを記録
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
                    request_body,
                    response_body,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Success,
                    completed_at: Utc::now(),
                },
            );

            Ok((StatusCode::OK, Json(payload)).into_response())
        }
        Err(e) => {
            state
                .load_manager
                .finish_request(agent_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            // パースエラーを記録
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type: RequestType::Generate,
                    model: req.model,
                    agent_id,
                    agent_machine_name,
                    agent_ip,
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
async fn select_available_agent(
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
            state.load_manager.select_agent().await
        }
    }
}

fn forward_streaming_response(response: reqwest::Response) -> Result<Response, CoordinatorError> {
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
    Ok(axum_response)
}

/// リクエスト/レスポンスレコードを保存（Fire-and-forget）
fn save_request_record(
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
    use crate::{balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager};
    use ollama_coordinator_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::net::IpAddr;

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
        state.registry.register(register_req2).await.unwrap();

        let result = select_available_agent(&state).await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.machine_name, "machine2");
    }
}
