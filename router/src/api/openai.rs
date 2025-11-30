//! OpenAI互換APIエンドポイント (/v1/*)

use axum::body::Body;
use axum::{
    extract::{Path, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use llm_router_common::{
    error::{CommonError, RouterError},
    protocol::{RecordStatus, RequestResponseRecord, RequestType},
};
use reqwest;
use serde_json::{json, Value};
use std::{net::IpAddr, time::Instant};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    api::{
        nodes::AppError,
        proxy::{forward_streaming_response, save_request_record, select_available_node},
    },
    balancer::RequestOutcome,
    cloud_metrics, AppState,
};

fn map_reqwest_error(err: reqwest::Error) -> AppError {
    AppError::from(RouterError::Http(err.to_string()))
}

fn auth_error(msg: &str) -> AppError {
    AppError::from(RouterError::Authentication(msg.to_string()))
}

fn get_required_key(provider: &str, env_key: &str, err_msg: &str) -> Result<String, AppError> {
    match std::env::var(env_key) {
        Ok(v) => {
            info!(provider = provider, key = env_key, "cloud api key present");
            Ok(v)
        }
        Err(_) => {
            warn!(provider = provider, key = env_key, "cloud api key missing");
            Err(auth_error(err_msg))
        }
    }
}

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
pub async fn list_models(State(_state): State<AppState>) -> Result<Response, AppError> {
    // ルーターがサポートするモデルを返す（プロキシせずローカルリストを使用）
    let client = crate::ollama::OllamaClient::new()?;
    let models = client.get_predefined_models();

    // OpenAI互換レスポンス形式に合わせる
    // https://platform.openai.com/docs/api-reference/models/list
    let data: Vec<Value> = models
        .into_iter()
        .map(|m| {
            json!({
                "id": m.name,
                "object": "model",
                "created": 0,
                "owned_by": "coordinator",
            })
        })
        .collect();

    let body = json!({
        "object": "list",
        "data": data,
    });

    Ok((StatusCode::OK, Json(body)).into_response())
}

/// GET /v1/models/:id - モデル詳細取得
pub async fn get_model(
    State(_state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Response, AppError> {
    let client = crate::ollama::OllamaClient::new()?;
    let exists = client
        .get_predefined_models()
        .into_iter()
        .any(|m| m.name == model_id);

    if !exists {
        // 404 を OpenAI 換算で返す
        let body = json!({
            "error": {
                "message": "The model does not exist",
                "type": "invalid_request_error",
                "param": "model",
                "code": "model_not_found"
            }
        });
        return Ok((StatusCode::NOT_FOUND, Json(body)).into_response());
    }

    let body = json!({
        "id": model_id,
        "object": "model",
        "created": 0,
        "owned_by": "coordinator",
    });

    Ok((StatusCode::OK, Json(body)).into_response())
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

fn parse_cloud_model(model: &str) -> Option<(String, String)> {
    // Accept prefixes like "openai:foo", "google:bar", "anthropic:baz"
    let prefixes = ["openai:", "google:", "anthropic:", "ahtnorpic:"];
    for p in prefixes.iter() {
        if model.starts_with(p) {
            let rest = model.trim_start_matches(p);
            if rest.is_empty() {
                return None;
            }
            let provider = if *p == "ahtnorpic:" {
                "anthropic"
            } else {
                p.trim_end_matches(':')
            };
            return Some((provider.to_string(), rest.to_string()));
        }
    }
    None
}

/// クラウドプロバイダ用の仮想ノード情報を生成する
fn cloud_virtual_agent(provider: &str) -> (Uuid, String, IpAddr) {
    let node_id = match provider {
        "openai" => Uuid::parse_str("00000000-0000-0000-0000-00000000c001").unwrap(),
        "google" => Uuid::parse_str("00000000-0000-0000-0000-00000000c002").unwrap(),
        "anthropic" => Uuid::parse_str("00000000-0000-0000-0000-00000000c003").unwrap(),
        _ => Uuid::parse_str("00000000-0000-0000-0000-00000000c0ff").unwrap(),
    };
    let machine_name = format!("cloud:{provider}");
    let agent_ip: IpAddr = "0.0.0.0".parse().unwrap();
    (node_id, machine_name, agent_ip)
}

struct CloudProxyResult {
    response: Response,
    response_body: Option<Value>,
    status: StatusCode,
    error_message: Option<String>,
}

fn map_openai_messages_to_google_contents(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|m| {
            let role = m.get("role")?.as_str().unwrap_or("user");
            let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let mapped_role = match role {
                "assistant" => "model",
                _ => "user",
            };
            Some(json!({
                "role": mapped_role,
                "parts": [{"text": text}]
            }))
        })
        .collect()
}

fn map_openai_messages_to_anthropic(messages: &[Value]) -> (Option<String>, Vec<Value>) {
    let mut system_msgs: Vec<String> = Vec::new();
    let mut regular: Vec<Value> = Vec::new();
    for m in messages.iter() {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        match role {
            "system" => system_msgs.push(text.to_string()),
            "assistant" => regular.push(json!({
                "role": "assistant",
                "content": [{"type":"text","text": text}]
            })),
            _ => regular.push(json!({
                "role": "user",
                "content": [{"type":"text","text": text}]
            })),
        }
    }
    let system = if system_msgs.is_empty() {
        None
    } else {
        Some(system_msgs.join("\n"))
    };
    (system, regular)
}

async fn proxy_openai_provider(
    target_path: &str,
    mut payload: Value,
    stream: bool,
    model: String,
) -> Result<CloudProxyResult, AppError> {
    let req_id = Uuid::new_v4();
    let started = Instant::now();
    let api_key = get_required_key(
        "openai",
        "OPENAI_API_KEY",
        "OPENAI_API_KEY is required for openai: models",
    )?;
    let base = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".into());

    // strip provider prefix before forwarding
    payload["model"] = Value::String(model);

    let client = reqwest::Client::new();
    let url = format!("{base}{target_path}");
    let res = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(map_reqwest_error)?;

    if stream {
        info!(
            provider = "openai",
            model = payload.get("model").and_then(|v| v.as_str()).unwrap_or(""),
            request_id = %req_id,
            latency_ms = started.elapsed().as_millis(),
            stream = true,
            status = %res.status(),
            "cloud proxy stream (openai)"
        );
        cloud_metrics::record(
            "openai",
            res.status().as_u16(),
            started.elapsed().as_millis(),
        );
        let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let response = forward_streaming_response(res).map_err(AppError::from)?;
        return Ok(CloudProxyResult {
            response,
            response_body: None,
            status,
            error_message: if status.is_success() {
                None
            } else {
                Some(status.to_string())
            },
        });
    }

    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ct = res.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let bytes = res.bytes().await.map_err(map_reqwest_error)?;
    let parsed_body = serde_json::from_slice::<Value>(&bytes).ok();
    let error_message = if status.is_success() {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes).trim().to_string())
    };
    let mut resp = Response::builder().status(status);
    if let Some(ct) = ct {
        if let Ok(hv) = HeaderValue::from_str(ct.to_str().unwrap_or("")) {
            resp = resp.header(CONTENT_TYPE, hv);
        }
    }
    let built = resp.body(Body::from(bytes)).unwrap();
    info!(
        provider = "openai",
        model = payload.get("model").and_then(|v| v.as_str()).unwrap_or(""),
        request_id = %req_id,
        latency_ms = started.elapsed().as_millis(),
        stream = false,
        status = %status,
        "cloud proxy complete (openai)"
    );
    cloud_metrics::record("openai", status.as_u16(), started.elapsed().as_millis());
    Ok(CloudProxyResult {
        response: built,
        response_body: parsed_body,
        status,
        error_message,
    })
}

fn map_generation_config(payload: &Value) -> Value {
    json!({
        "temperature": payload.get("temperature").and_then(|v| v.as_f64()),
        "topP": payload.get("top_p").and_then(|v| v.as_f64()),
        "maxOutputTokens": payload.get("max_tokens").and_then(|v| v.as_i64()),
    })
}

async fn proxy_google_provider(
    model: String,
    payload: Value,
    stream: bool,
) -> Result<CloudProxyResult, AppError> {
    let req_id = Uuid::new_v4();
    let started = Instant::now();
    let api_key = get_required_key(
        "google",
        "GOOGLE_API_KEY",
        "GOOGLE_API_KEY is required for google: models",
    )?;
    let base = std::env::var("GOOGLE_API_BASE_URL")
        .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".into());
    let messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let contents = map_openai_messages_to_google_contents(&messages);
    let mut body = json!({
        "contents": contents,
        "generationConfig": map_generation_config(&payload),
    });
    // drop nulls in generationConfig
    if let Some(gen) = body["generationConfig"].as_object_mut() {
        gen.retain(|_, v| !v.is_null());
    }

    let endpoint_suffix = if stream {
        format!("models/{model}:streamGenerateContent")
    } else {
        format!("models/{model}:generateContent")
    };
    let url = format!("{base}/{endpoint_suffix}");

    let client = reqwest::Client::new();
    let req = client.post(&url).query(&[("key", api_key)]).json(&body);
    let res = req.send().await.map_err(map_reqwest_error)?;

    if stream {
        info!(
            provider = "google",
            model = %model,
            request_id = %req_id,
            latency_ms = started.elapsed().as_millis(),
            stream = true,
            status = %res.status(),
            "cloud proxy stream (google)"
        );
        cloud_metrics::record(
            "google",
            res.status().as_u16(),
            started.elapsed().as_millis(),
        );
        let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let response = forward_streaming_response(res).map_err(AppError::from)?;
        return Ok(CloudProxyResult {
            response,
            response_body: None,
            status,
            error_message: if status.is_success() {
                None
            } else {
                Some(status.to_string())
            },
        });
    }

    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let data: Value = res.json().await.map_err(map_reqwest_error)?;
    let text = data
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    let resp_body = json!({
        "id": format!("google-{}", Uuid::new_v4()),
        "object": "chat.completion",
        "model": format!("google:{model}"),
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
        "finish_reason": "stop"
    }],
    });

    let built = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Body::from(resp_body.to_string()))
        .map_err(|e| AppError::from(RouterError::Http(e.to_string())))?;

    info!(
        provider = "google",
        model = %model,
        request_id = %req_id,
        latency_ms = started.elapsed().as_millis(),
        stream = false,
        status = %status,
        "cloud proxy complete (google)"
    );

    cloud_metrics::record("google", status.as_u16(), started.elapsed().as_millis());

    let error_message = if status.is_success() {
        None
    } else {
        serde_json::to_string(&data).ok()
    };

    Ok(CloudProxyResult {
        response: built,
        response_body: Some(resp_body),
        status,
        error_message,
    })
}

async fn proxy_anthropic_provider(
    model: String,
    payload: Value,
    stream: bool,
) -> Result<CloudProxyResult, AppError> {
    let req_id = Uuid::new_v4();
    let started = Instant::now();
    let api_key = get_required_key(
        "anthropic",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_API_KEY is required for anthropic: models",
    )?;
    let base = std::env::var("ANTHROPIC_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".into());
    let messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let (system, mapped) = map_openai_messages_to_anthropic(&messages);
    let max_tokens = payload
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(1024);
    let mut body = json!({
        "model": model,
        "messages": mapped,
        "max_tokens": max_tokens,
        "stream": stream,
        "temperature": payload.get("temperature").and_then(|v| v.as_f64()),
        "top_p": payload.get("top_p").and_then(|v| v.as_f64()),
    });
    if let Some(s) = system {
        body["system"] = Value::String(s);
    }
    // prune nulls
    if let Some(obj) = body.as_object_mut() {
        obj.retain(|_, v| !v.is_null());
    }

    let url = format!("{base}/v1/messages");
    let client = reqwest::Client::new();
    let req = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    let res = req.send().await.map_err(map_reqwest_error)?;

    if stream {
        info!(
            provider = "anthropic",
            model = %model,
            request_id = %req_id,
            latency_ms = started.elapsed().as_millis(),
            stream = true,
            status = %res.status(),
            "cloud proxy stream (anthropic)"
        );
        cloud_metrics::record(
            "anthropic",
            res.status().as_u16(),
            started.elapsed().as_millis(),
        );
        let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let response = forward_streaming_response(res).map_err(AppError::from)?;
        return Ok(CloudProxyResult {
            response,
            response_body: None,
            status,
            error_message: if status.is_success() {
                None
            } else {
                Some(status.to_string())
            },
        });
    }

    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let data: Value = res.json().await.map_err(map_reqwest_error)?;
    let text = data
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    let id = data
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("anthropic-{}", Uuid::new_v4()));
    let model_label = data
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| model.clone());

    let resp_body = json!({
        "id": id,
        "object": "chat.completion",
        "model": format!("anthropic:{}", model_label),
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
        "finish_reason": "stop"
    }],
    });

    let built = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Body::from(resp_body.to_string()))
        .map_err(|e| AppError::from(RouterError::Http(e.to_string())))?;

    info!(
        provider = "anthropic",
        model = %model_label,
        request_id = %req_id,
        latency_ms = started.elapsed().as_millis(),
        stream = false,
        status = %status,
        "cloud proxy complete (anthropic)"
    );

    cloud_metrics::record("anthropic", status.as_u16(), started.elapsed().as_millis());

    let error_message = if status.is_success() {
        None
    } else {
        serde_json::to_string(&data).ok()
    };

    Ok(CloudProxyResult {
        response: built,
        response_body: Some(resp_body),
        status,
        error_message,
    })
}

async fn proxy_openai_cloud_post(
    state: &AppState,
    target_path: &str,
    model: &str,
    stream: bool,
    payload: Value,
    request_type: RequestType,
) -> Result<Response, AppError> {
    let (provider, model_name) = parse_cloud_model(model)
        .ok_or_else(|| validation_error("cloud model prefix is invalid"))?;
    let (node_id, agent_machine_name, agent_ip) = cloud_virtual_agent(&provider);
    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = payload.clone();
    let started = Instant::now();

    let outcome = match match provider.as_str() {
        "openai" => proxy_openai_provider(target_path, payload, stream, model_name).await,
        "google" => proxy_google_provider(model_name, payload, stream).await,
        "anthropic" => proxy_anthropic_provider(model_name, payload, stream).await,
        _ => Err(validation_error("unsupported cloud provider prefix")),
    } {
        Ok(res) => res,
        Err(e) => {
            let duration = started.elapsed();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type,
                    model: model.to_string(),
                    node_id,
                    agent_machine_name,
                    agent_ip,
                    client_ip: None,
                    request_body,
                    response_body: None,
                    duration_ms: duration.as_millis() as u64,
                    status: RecordStatus::Error {
                        message: format!("{e:?}"),
                    },
                    completed_at: Utc::now(),
                },
            );
            return Err(e);
        }
    };

    let duration = started.elapsed();
    let status = outcome.status;
    let status_record = if status.is_success() {
        RecordStatus::Success
    } else {
        RecordStatus::Error {
            message: outcome
                .error_message
                .clone()
                .unwrap_or_else(|| status.to_string()),
        }
    };
    let response_body = if status.is_success() {
        outcome.response_body.clone()
    } else {
        None
    };

    save_request_record(
        state.request_history.clone(),
        RequestResponseRecord {
            id: record_id,
            timestamp,
            request_type,
            model: model.to_string(),
            node_id,
            agent_machine_name,
            agent_ip,
            client_ip: None,
            request_body,
            response_body,
            duration_ms: duration.as_millis() as u64,
            status: status_record,
            completed_at: Utc::now(),
        },
    );

    Ok(outcome.response)
}

async fn proxy_openai_post(
    state: &AppState,
    payload: Value,
    target_path: &str,
    model: String,
    stream: bool,
    request_type: RequestType,
) -> Result<Response, AppError> {
    // Cloud-prefixed model -> forward to provider API
    if parse_cloud_model(&model).is_some() {
        return proxy_openai_cloud_post(state, target_path, &model, stream, payload, request_type)
            .await;
    }

    let record_id = Uuid::new_v4();
    let timestamp = Utc::now();
    let request_body = payload.clone();

    let agent = select_available_node(state).await?;
    let node_id = agent.id;
    let agent_machine_name = agent.machine_name.clone();
    let agent_ip = agent.ip_address;

    state
        .load_manager
        .begin_request(node_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let agent_api_port = agent.ollama_port + 1;
    let ollama_url = format!(
        "http://{}:{}{}",
        agent.ip_address, agent_api_port, target_path
    );
    let start = Instant::now();

    let response = match client.post(&ollama_url).json(&payload).send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            state
                .load_manager
                .finish_request(node_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type,
                    model: model.clone(),
                    node_id,
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

            return Err(RouterError::Http(format!("Failed to proxy OpenAI request: {}", e)).into());
        }
    };

    // ストリームの場合はレスポンスをそのままパススルー
    if stream {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(node_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type,
                model: model.clone(),
                node_id,
                agent_machine_name: agent_machine_name.clone(),
                agent_ip,
                client_ip: None,
                request_body: request_body.clone(),
                response_body: None, // ストリームボディは記録しない
                duration_ms: duration.as_millis() as u64,
                status: RecordStatus::Success,
                completed_at: Utc::now(),
            },
        );

        return forward_streaming_response(response).map_err(AppError::from);
    }

    if !response.status().is_success() {
        let duration = start.elapsed();
        state
            .load_manager
            .finish_request(node_id, RequestOutcome::Error, duration)
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
                node_id,
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
            .finish_request(node_id, RequestOutcome::Success, duration)
            .await
            .map_err(AppError::from)?;

        save_request_record(
            state.request_history.clone(),
            RequestResponseRecord {
                id: record_id,
                timestamp,
                request_type,
                model,
                node_id,
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
                .finish_request(node_id, RequestOutcome::Success, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type,
                    model,
                    node_id,
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
                .finish_request(node_id, RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;

            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord {
                    id: record_id,
                    timestamp,
                    request_type,
                    model,
                    node_id,
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

            Err(RouterError::Http(format!("Failed to parse OpenAI response: {}", e)).into())
        }
    }
}

#[allow(dead_code)]
async fn proxy_openai_get(state: &AppState, target_path: &str) -> Result<Response, AppError> {
    let agent = select_available_node(state).await?;
    let node_id = agent.id;

    state
        .load_manager
        .begin_request(node_id)
        .await
        .map_err(AppError::from)?;

    let client = reqwest::Client::new();
    let ollama_url = format!(
        "http://{}:{}{}",
        agent.ip_address, agent.ollama_port, target_path
    );
    let start = Instant::now();

    let response = client.get(&ollama_url).send().await.map_err(|e| {
        AppError::from(RouterError::Http(format!(
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
        .finish_request(node_id, outcome, duration)
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
        AppError::from(RouterError::Http(format!(
            "Failed to parse OpenAI models response: {}",
            e
        )))
    })?;

    Ok((StatusCode::OK, Json(body)).into_response())
}

fn validation_error(message: impl Into<String>) -> AppError {
    let err = RouterError::Common(CommonError::Validation(message.into()));
    err.into()
}

#[cfg(test)]
mod tests {
    use super::{parse_cloud_model, proxy_openai_cloud_post, proxy_openai_post};
    use crate::{
        balancer::LoadManager, db::request_history::RequestHistoryStorage, registry::NodeRegistry,
        tasks::DownloadTaskManager, AppState,
    };
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use llm_router_common::protocol::{RecordStatus, RequestType};
    use serde_json::json;
    use serial_test::serial;
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::time::{sleep, Duration};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn create_local_state() -> AppState {
        let registry = NodeRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history =
            Arc::new(RequestHistoryStorage::new().expect("request history storage"));
        let task_manager = DownloadTaskManager::new();
        let db_pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory connect");
        sqlx::migrate!("./migrations")
            .run(&db_pool)
            .await
            .expect("migrations");
        AppState {
            registry,
            load_manager,
            request_history,
            task_manager,
            db_pool,
            jwt_secret: "test-secret".into(),
        }
    }

    async fn create_state_with_tempdir() -> (AppState, tempfile::TempDir) {
        let dir = tempdir().expect("temp dir");
        std::env::set_var("LLM_ROUTER_DATA_DIR", dir.path());
        let state = create_local_state().await;
        (state, dir)
    }

    #[test]
    fn parse_cloud_prefixes() {
        assert_eq!(
            parse_cloud_model("openai:gpt-4o"),
            Some(("openai".to_string(), "gpt-4o".to_string()))
        );
        assert_eq!(
            parse_cloud_model("google:gemini-pro"),
            Some(("google".to_string(), "gemini-pro".to_string()))
        );
        assert_eq!(
            parse_cloud_model("ahtnorpic:claude-3"),
            Some(("anthropic".to_string(), "claude-3".to_string()))
        );
        assert_eq!(parse_cloud_model("gpt-4"), None);
        assert_eq!(parse_cloud_model("openai:"), None);
    }

    #[tokio::test]
    #[serial]
    async fn openai_prefix_requires_api_key() {
        // Save and remove any existing API key to test error case
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            false,
            payload,
            RequestType::Chat,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("OPENAI_API_KEY"),
            "expected error mentioning OPENAI_API_KEY, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn google_prefix_requires_api_key() {
        // Save and remove any existing API key to test error case
        let saved = std::env::var("GOOGLE_API_KEY").ok();
        std::env::remove_var("GOOGLE_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"google:gemini-pro","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "google:gemini-pro",
            false,
            payload,
            RequestType::Chat,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("GOOGLE_API_KEY"),
            "expected GOOGLE_API_KEY error, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("GOOGLE_API_KEY", key);
        }
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn anthropic_prefix_requires_api_key() {
        // Save and remove any existing API key to test error case
        let saved = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"anthropic:claude-3","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "anthropic:claude-3",
            false,
            payload,
            RequestType::Chat,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("ANTHROPIC_API_KEY"),
            "expected ANTHROPIC_API_KEY error, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn openai_prefix_streams_via_cloud() {
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "text/event-stream",
            );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("OPENAI_API_KEY", "testkey");
        std::env::set_var("OPENAI_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[],"stream":true});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            true,
            payload,
            RequestType::Chat,
        )
        .await
        .expect("cloud stream response");
        let body = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("delta"));
        assert!(body_str.contains("hi"));

        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn google_prefix_proxies_and_maps_response() {
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "hello from gemini"}]}}]
        }));
        Mock::given(method("POST"))
            .and(path("/models/gemini-pro:generateContent"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("GOOGLE_API_KEY", "gkey");
        std::env::set_var("GOOGLE_API_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload =
            json!({"model":"google:gemini-pro","messages":[{"role":"user","content":"hi"}]});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "google:gemini-pro",
            false,
            payload,
            RequestType::Chat,
        )
        .await
        .expect("google mapped response");
        let bytes = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["model"].as_str().unwrap(), "google:gemini-pro");
        assert_eq!(
            v["choices"][0]["message"]["content"].as_str().unwrap(),
            "hello from gemini"
        );

        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("GOOGLE_API_BASE_URL");
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn anthropic_prefix_proxies_and_maps_response() {
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
            "id": "abc123",
            "model": "claude-3",
            "content": [{"text": "anthropic says hi"}]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("ANTHROPIC_API_KEY", "akey");
        std::env::set_var("ANTHROPIC_API_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload =
            json!({"model":"anthropic:claude-3","messages":[{"role":"user","content":"hi"}]});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "anthropic:claude-3",
            false,
            payload,
            RequestType::Chat,
        )
        .await
        .expect("anthropic mapped response");
        let bytes = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["model"].as_str().unwrap(), "anthropic:claude-3");
        assert_eq!(
            v["choices"][0]["message"]["content"].as_str().unwrap(),
            "anthropic says hi"
        );

        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn cloud_request_is_recorded_in_history() {
        let temp_dir = tempdir().expect("temp dir");
        std::env::set_var("LLM_ROUTER_DATA_DIR", temp_dir.path());

        let state = create_local_state().await;
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("OPENAI_API_KEY", "testkey");
        std::env::set_var("OPENAI_BASE_URL", server.uri());

        let payload = json!({"model":"openai:gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":false});
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "openai:gpt-4o".into(),
            false,
            RequestType::Chat,
        )
        .await
        .expect("cloud proxy succeeds");

        assert_eq!(response.status(), StatusCode::OK);
        sleep(Duration::from_millis(20)).await;

        let records = state.request_history.load_records().await.expect("records");
        assert_eq!(records.len(), 1, "cloud request should be recorded");

        let record = &records[0];
        assert_eq!(record.model, "openai:gpt-4o");
        assert!(matches!(record.status, RecordStatus::Success));
        assert_eq!(record.request_type, RequestType::Chat);
        assert!(
            record.response_body.is_some(),
            "response should be captured"
        );

        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn non_prefixed_model_stays_on_local_path() {
        let state = create_local_state().await;
        let payload = json!({"model":"gpt-oss:20b","messages":[]});
        let res = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "gpt-oss:20b".into(),
            false,
            RequestType::Chat,
        )
        .await;
        let err = res.unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("NoAgentsAvailable")
                || msg.contains("No available agents")
                || msg.contains("No agents available"),
            "expected local-path agent error, got {}",
            msg
        );
    }
    #[tokio::test]
    #[serial]
    async fn streaming_allowed_for_cloud_prefix() {
        // Save and remove any existing API key to test error case
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[],"stream":true});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            true,
            payload,
            RequestType::Chat,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("OPENAI_API_KEY"),
            "expected API key error (stream path), got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }
}
