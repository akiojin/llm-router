//! Anthropic Messages API endpoint (`/v1/messages`).
//!
//! The public surface is Anthropic-compatible, while non-`anthropic:` models are
//! routed through the existing OpenAI-compatible local endpoint path.

/// Anthropic → OpenAI リクエスト変換は translation submodule に分離（arch-review [M4]）
mod translation;
use translation::anthropic_request_to_openai;
mod cloud;
mod streaming;
#[cfg(test)]
use crate::token::{StreamingTokenAccumulator, TokenUsage};
#[cfg(test)]
use axum::body::Bytes;
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use std::collections::{BTreeMap, VecDeque};
use streaming::transform_openai_streaming_response_to_anthropic;
#[cfg(test)]
use streaming::AnthropicStreamTracker;
mod errors;
use cloud::{parse_anthropic_cloud_model, proxy_anthropic_cloud_messages};
mod response;
use errors::{
    anthropic_error_from_lb_error, anthropic_error_response, anthropic_upstream_error_details,
    map_upstream_status_to_anthropic_error,
};
use response::{extract_openai_response_text, openai_to_anthropic_message_response};

use crate::api::error::AppError;
use crate::api::model_name::rewrite_payload_model_for_endpoint;
use crate::api::models::load_registered_model;
use crate::api::openai_util::upstream_error_message_from_bytes;
use crate::api::proxy::{
    add_queue_headers, forward_to_endpoint, record_endpoint_request_stats, save_request_record,
    select_available_endpoint_with_queue_for_model, update_inference_latency, QueueSelection,
};
use crate::auth::middleware::ApiKeyAuthContext;
use crate::balancer::RequestOutcome;
use crate::common::error::LbError;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType, TpsApiKind};
use crate::token::{estimate_tokens, extract_or_estimate_tokens};
use crate::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
use uuid::Uuid;

const UNSPECIFIED_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

#[derive(Debug)]
struct ConvertedAnthropicRequest {
    openai_payload: Value,
    request_text: String,
    stream: bool,
}

/// Handle `POST /v1/messages` using the Anthropic-native request/response shape.
pub async fn messages(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Response {
    match handle_messages(addr, headers, state, auth_ctx, payload).await {
        Ok(response) => response,
        Err(err) => anthropic_error_from_lb_error(&err.0),
    }
}

async fn handle_messages(
    addr: SocketAddr,
    headers: HeaderMap,
    state: AppState,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    payload: Value,
) -> Result<Response, AppError> {
    let anthropic_version = match extract_required_header(&headers, "anthropic-version") {
        Ok(value) => value,
        Err(response) => return Ok(response),
    };
    let anthropic_beta = headers
        .get("anthropic-beta")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let (client_ip, api_key_id) =
        crate::common::http::extract_client_info(&addr, &headers, &auth_ctx);
    let request_body = payload.clone();
    let model = match extract_model(&payload) {
        Ok(model) => model,
        Err(response) => return Ok(response),
    };

    if let Some(cloud_model) = parse_anthropic_cloud_model(&model) {
        return proxy_anthropic_cloud_messages(
            &state,
            request_body,
            model,
            cloud_model,
            anthropic_version,
            anthropic_beta,
            client_ip,
            api_key_id,
        )
        .await;
    }

    let converted = match anthropic_request_to_openai(&payload) {
        Ok(converted) => converted,
        Err(response) => return Ok(response),
    };

    proxy_local_anthropic_messages(
        &state,
        request_body,
        model,
        converted,
        client_ip,
        api_key_id,
    )
    .await
}

async fn proxy_local_anthropic_messages(
    state: &AppState,
    request_body: Value,
    model: String,
    converted: ConvertedAnthropicRequest,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    let resolved_model = {
        let found = state.endpoint_registry.find_by_model(&model).await;
        if found.is_empty() {
            if let Some(canonical) = crate::models::mapping::resolve_canonical_any(&model) {
                let canonical_found = state.endpoint_registry.find_by_model(canonical).await;
                if !canonical_found.is_empty() {
                    canonical.to_string()
                } else {
                    model.clone()
                }
            } else {
                model.clone()
            }
        } else {
            model.clone()
        }
    };

    if state
        .endpoint_registry
        .find_by_model(&resolved_model)
        .await
        .is_empty()
    {
        let is_registered = load_registered_model(&state.db_pool, &resolved_model).await?;
        if is_registered.is_none() {
            return Ok(anthropic_error_response(
                StatusCode::NOT_FOUND,
                "not_found_error",
                format!("The model '{}' does not exist", model),
            ));
        }
    }

    let request_type = RequestType::AnthropicMessages;
    let tps_api_kind = Some(TpsApiKind::ChatCompletions);
    let mut queued_wait_ms = None;

    let endpoint =
        match select_available_endpoint_with_queue_for_model(state, &resolved_model, tps_api_kind)
            .await
        {
            Ok(QueueSelection::Ready {
                endpoint,
                queued_wait_ms: wait_ms,
            }) => {
                queued_wait_ms = wait_ms;
                *endpoint
            }
            Err(err) => {
                let message = if matches!(err, LbError::NoCapableEndpoints(_)) {
                    format!("No available endpoints support model: {}", model)
                } else {
                    format!("Endpoint selection failed: {}", err)
                };
                save_request_record(
                    state.request_history.clone(),
                    RequestResponseRecord::error(
                        model.clone(),
                        request_type,
                        request_body,
                        message.clone(),
                        queued_wait_ms.unwrap_or(0) as u64,
                        client_ip,
                        api_key_id,
                    ),
                );
                if matches!(err, LbError::NoCapableEndpoints(_)) {
                    return Ok(anthropic_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "api_error",
                        message,
                    ));
                }
                return Err(err.into());
            }
        };

    let endpoint_id = endpoint.id;
    let endpoint_name = endpoint.name.clone();
    let endpoint_type = endpoint.endpoint_type;
    let request_lease = state
        .load_manager
        .begin_request(endpoint_id)
        .await
        .map_err(AppError::from)?;
    let endpoint_models = match state.endpoint_registry.list_models(endpoint_id).await {
        Ok(models) => models,
        Err(error) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                model = %resolved_model,
                error = %error,
                "Failed to load endpoint models for Anthropic request rewrite; falling back to static mapping"
            );
            Vec::new()
        }
    };
    let ConvertedAnthropicRequest {
        openai_payload,
        request_text,
        stream,
    } = converted;
    let outbound_payload = rewrite_payload_model_for_endpoint(
        openai_payload,
        &resolved_model,
        &endpoint_type,
        &endpoint_models,
    );
    let body_bytes = serde_json::to_vec(&outbound_payload).map_err(|err| {
        AppError::from(LbError::Http(format!(
            "Failed to serialize translated OpenAI payload: {}",
            err
        )))
    })?;
    let started = Instant::now();

    let upstream = match forward_to_endpoint(
        &state.http_client,
        &endpoint,
        "/v1/chat/completions",
        body_bytes,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            let (error_status, error_type, message) = anthropic_upstream_error_details(&err);
            let duration = started.elapsed();
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body,
                error_status,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);

            return Ok(anthropic_error_response(error_status, error_type, message));
        }
    };

    if stream {
        let duration = started.elapsed();
        let upstream_status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let succeeded = upstream_status.is_success();
        let outcome = if succeeded {
            RequestOutcome::Success
        } else {
            RequestOutcome::Error
        };
        request_lease
            .complete(outcome, duration)
            .await
            .map_err(AppError::from)?;

        if succeeded {
            update_inference_latency(&state.endpoint_registry, endpoint_id, duration);
        } else {
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );
        }

        if !succeeded {
            let body = upstream.bytes().await.unwrap_or_default();
            let message = upstream_error_message_from_bytes(upstream_status, &body);
            let (anthropic_status, error_type) =
                map_upstream_status_to_anthropic_error(upstream_status);
            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name,
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body,
                anthropic_status,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);
            return Ok(anthropic_error_response(
                anthropic_status,
                error_type,
                message,
            ));
        }

        let record = RequestResponseRecord::new(
            endpoint_id,
            endpoint_name,
            UNSPECIFIED_IP,
            model.clone(),
            request_type,
            request_body,
            upstream_status,
            duration,
            client_ip,
            api_key_id,
        );
        save_request_record(state.request_history.clone(), record);

        let mut response = transform_openai_streaming_response_to_anthropic(
            upstream,
            endpoint_id,
            model.clone(),
            endpoint_type,
            started,
            estimate_tokens(&request_text, &model),
            state.endpoint_registry.clone(),
            state.load_manager.clone(),
            state.event_bus.clone(),
        );
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut response, wait_ms);
        }
        return Ok(response);
    }

    let upstream_status = upstream.status();
    let upstream_body = match upstream.bytes().await {
        Ok(body) => body,
        Err(err) => {
            let duration = started.elapsed();
            let read_error = LbError::Http(format!(
                "Failed to read OpenAI-compatible upstream response: {}",
                err
            ));
            let (error_status, error_type, message) = anthropic_upstream_error_details(&read_error);

            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body.clone(),
                error_status,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);

            return Ok(anthropic_error_response(error_status, error_type, message));
        }
    };
    let duration = started.elapsed();

    if !upstream_status.is_success() {
        request_lease
            .complete(RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;
        record_endpoint_request_stats(
            state.endpoint_registry.clone(),
            endpoint_id,
            model.clone(),
            false,
            0,
            0,
            tps_api_kind,
            endpoint_type,
            state.load_manager.clone(),
            state.event_bus.clone(),
        );

        let message = upstream_error_message_from_bytes(upstream_status, &upstream_body);
        let (anthropic_status, error_type) =
            map_upstream_status_to_anthropic_error(upstream_status);

        let mut record = RequestResponseRecord::new(
            endpoint_id,
            endpoint_name,
            UNSPECIFIED_IP,
            model,
            request_type,
            request_body,
            anthropic_status,
            duration,
            client_ip,
            api_key_id,
        );
        record.status = RecordStatus::Error {
            message: message.clone(),
        };
        save_request_record(state.request_history.clone(), record);

        return Ok(anthropic_error_response(
            anthropic_status,
            error_type,
            message,
        ));
    }

    match serde_json::from_slice::<Value>(&upstream_body) {
        Ok(body) => {
            let response_text = extract_openai_response_text(&body);
            let token_usage = extract_or_estimate_tokens(
                &body,
                Some(&request_text),
                Some(&response_text),
                &model,
            );

            request_lease
                .complete_with_tokens(RequestOutcome::Success, duration, Some(token_usage.clone()))
                .await
                .map_err(AppError::from)?;
            update_inference_latency(&state.endpoint_registry, endpoint_id, duration);

            let output_tokens = token_usage.output_tokens.unwrap_or(0) as u64;
            let duration_ms = if output_tokens > 0 {
                duration.as_millis().max(1) as u64
            } else {
                0
            };
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                true,
                output_tokens,
                duration_ms,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            let anthropic_body = openai_to_anthropic_message_response(&body, &model, &token_usage);

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name,
                UNSPECIFIED_IP,
                model,
                request_type,
                request_body,
                StatusCode::OK,
                duration,
                client_ip,
                api_key_id,
            );
            record.response_body = Some(anthropic_body.clone());
            record.input_tokens = token_usage.input_tokens;
            record.output_tokens = token_usage.output_tokens;
            record.total_tokens = token_usage.total_tokens;
            save_request_record(state.request_history.clone(), record);

            let mut response = (StatusCode::OK, Json(anthropic_body)).into_response();
            if let Some(wait_ms) = queued_wait_ms {
                add_queue_headers(&mut response, wait_ms);
            }
            Ok(response)
        }
        Err(err) => {
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name,
                UNSPECIFIED_IP,
                model,
                request_type,
                request_body,
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: format!(
                    "Failed to parse OpenAI-compatible upstream response: {}",
                    err
                ),
            };
            save_request_record(state.request_history.clone(), record);

            let message = format!(
                "Failed to parse OpenAI-compatible upstream response: {}",
                err
            );
            Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                message,
            ))
        }
    }
}

/// Anthropic の tool_result.content（文字列または content ブロック配列）を
/// OpenAI tool メッセージ用の文字列へ変換する。配列形式（Claude Code が常用）でも
/// 中身を欠落させない。
fn anthropic_tool_result_content_to_string(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            parts.push(t.to_string());
                        }
                    }
                    // text 以外（image 等）は JSON で温存して情報欠落を防ぐ
                    _ => parts.push(item.to_string()),
                }
            }
            parts.join("\n")
        }
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

#[allow(clippy::result_large_err)]
fn extract_model(payload: &Value) -> Result<String, Response> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "model is required",
            )
        })?;
    if model.trim().is_empty() {
        return Err(anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "model must not be empty",
        ));
    }
    Ok(model.to_string())
}

#[allow(clippy::result_large_err)]
fn extract_required_header(headers: &HeaderMap, name: &'static str) -> Result<String, Response> {
    let value = headers.get(name).and_then(|header| header.to_str().ok());
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value.to_string()),
        _ => Err(anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            format!("Missing required header: {}", name),
        )),
    }
}

#[cfg(test)]
mod tests;
