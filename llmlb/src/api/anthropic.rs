//! Anthropic Messages API endpoint (`/v1/messages`).
//!
//! The public surface is Anthropic-compatible, while non-`anthropic:` models are
//! routed through the existing OpenAI-compatible local endpoint path.

use crate::api::error::AppError;
use crate::api::model_name::rewrite_payload_model_for_endpoint;
use crate::api::models::load_registered_model;
use crate::api::proxy::{
    forward_streaming_response, forward_to_endpoint, record_endpoint_request_stats,
    save_request_record, select_available_endpoint_with_queue_for_model, QueueSelection,
};
use crate::auth::middleware::ApiKeyAuthContext;
use crate::balancer::RequestOutcome;
use crate::cloud_metrics;
use crate::common::error::{CommonError, LbError};
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType, TpsApiKind};
use crate::token::{
    estimate_tokens, extract_or_estimate_tokens, extract_usage_from_response,
    StreamingTokenAccumulator, TokenUsage,
};
use crate::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::time::Instant;
use uuid::Uuid;

const UNSPECIFIED_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const ANTHROPIC_CLOUD_ENDPOINT_ID: &str = "00000000-0000-0000-0000-00000000c003";

#[derive(Debug)]
struct ConvertedAnthropicRequest {
    openai_payload: Value,
    request_text: String,
    stream: bool,
}

struct AnthropicStreamTracker {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    upstream_line_buffer: String,
    output_queue: VecDeque<Bytes>,
    accumulator: StreamingTokenAccumulator,
    endpoint_id: Uuid,
    model_id: String,
    endpoint_type: crate::types::endpoint::EndpointType,
    request_started_at: Instant,
    endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    load_manager: crate::balancer::LoadManager,
    event_bus: crate::events::SharedEventBus,
    sent_message_start: bool,
    sent_content_block_start: bool,
    sent_content_block_stop: bool,
    sent_message_stop: bool,
    response_id: String,
    public_model: String,
    stop_reason: Option<&'static str>,
    stop_sequence: Option<String>,
    stats_recorded: bool,
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

    let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &auth_ctx);
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

#[allow(clippy::too_many_arguments)]
async fn proxy_anthropic_cloud_messages(
    state: &AppState,
    request_body: Value,
    public_model: String,
    cloud_model: String,
    anthropic_version: String,
    anthropic_beta: Option<String>,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(value) => value,
        Err(_) => {
            return Ok(anthropic_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "api_error",
                "Anthropic cloud integration is not configured",
            ));
        }
    };
    let base_url = std::env::var("ANTHROPIC_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".into());
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let endpoint_id = Uuid::parse_str(ANTHROPIC_CLOUD_ENDPOINT_ID)
        .expect("static anthropic cloud endpoint id must be valid");
    let mut upstream_body = request_body.clone();
    upstream_body["model"] = Value::String(cloud_model);

    let started = Instant::now();
    let mut builder = state
        .http_client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", anthropic_version)
        .json(&upstream_body);
    if let Some(beta) = anthropic_beta {
        builder = builder.header("anthropic-beta", beta);
    }

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            let duration = started.elapsed();
            let mut record = RequestResponseRecord::new(
                endpoint_id,
                "cloud:anthropic".to_string(),
                UNSPECIFIED_IP,
                public_model,
                RequestType::AnthropicMessages,
                request_body,
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: format!("Failed to proxy Anthropic cloud request: {}", err),
            };
            save_request_record(state.request_history.clone(), record);

            return Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                "Anthropic upstream request failed",
            ));
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    cloud_metrics::record("anthropic", status.as_u16(), started.elapsed().as_millis());

    if stream && status.is_success() {
        let response = forward_streaming_response(upstream).map_err(AppError::from)?;
        let record = RequestResponseRecord::new(
            endpoint_id,
            "cloud:anthropic".to_string(),
            UNSPECIFIED_IP,
            public_model,
            RequestType::AnthropicMessages,
            request_body,
            status,
            started.elapsed(),
            client_ip,
            api_key_id,
        );
        save_request_record(state.request_history.clone(), record);
        return Ok(response);
    }

    let headers = upstream.headers().clone();
    let bytes = upstream.bytes().await.map_err(|err| {
        AppError::from(LbError::Http(format!(
            "Failed to read Anthropic cloud response body: {}",
            err
        )))
    })?;
    let parsed_body = serde_json::from_slice::<Value>(&bytes).ok();

    let mut record = RequestResponseRecord::new(
        endpoint_id,
        "cloud:anthropic".to_string(),
        UNSPECIFIED_IP,
        public_model,
        RequestType::AnthropicMessages,
        request_body,
        status,
        started.elapsed(),
        client_ip,
        api_key_id,
    );
    if let Some(body) = parsed_body.clone() {
        if status.is_success() {
            record.response_body = Some(body.clone());
            if let Some(usage) = extract_usage_from_response(&body) {
                record.input_tokens = usage.input_tokens;
                record.output_tokens = usage.output_tokens;
                record.total_tokens = usage.total_tokens;
            }
        } else {
            record.status = RecordStatus::Error {
                message: body
                    .get("error")
                    .and_then(|v| v.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| status.as_str())
                    .to_string(),
            };
        }
    } else if !status.is_success() {
        record.status = RecordStatus::Error {
            message: String::from_utf8_lossy(&bytes).trim().to_string(),
        };
    }
    save_request_record(state.request_history.clone(), record);

    Ok(build_response_from_upstream(status, &headers, bytes))
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

    let queue_config = state.queue_config;
    let request_type = RequestType::AnthropicMessages;
    let tps_api_kind = Some(TpsApiKind::ChatCompletions);
    let mut queued_wait_ms = None;

    let endpoint = match select_available_endpoint_with_queue_for_model(
        state,
        queue_config,
        &resolved_model,
        tps_api_kind,
    )
    .await
    {
        Ok(QueueSelection::Ready {
            endpoint,
            queued_wait_ms: wait_ms,
        }) => {
            queued_wait_ms = wait_ms;
            *endpoint
        }
        Ok(QueueSelection::CapacityExceeded) => {
            let message = "Request queue is full".to_string();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    message.clone(),
                    0,
                    client_ip,
                    api_key_id,
                ),
            );
            let retry_after = queue_config.timeout.as_secs().max(1);
            return Ok(anthropic_error_response_with_retry_after(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                &message,
                Some(retry_after),
            ));
        }
        Ok(QueueSelection::Timeout { waited_ms }) => {
            let message = "Queue wait timeout".to_string();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    message.clone(),
                    waited_ms as u64,
                    client_ip,
                    api_key_id,
                ),
            );
            return Ok(anthropic_error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "api_error",
                message,
            ));
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
        stream,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
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
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: err.to_string(),
            };
            save_request_record(state.request_history.clone(), record);

            return Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                "OpenAI-compatible upstream request failed",
            ));
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

        let mut record = RequestResponseRecord::new(
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
        if !succeeded {
            record.status = RecordStatus::Error {
                message: format!("Upstream stream returned status {}", upstream_status),
            };
        }
        save_request_record(state.request_history.clone(), record);

        if !succeeded {
            let status = upstream.status();
            let body = upstream.bytes().await.unwrap_or_default();
            let message = if body.is_empty() {
                status.to_string()
            } else {
                String::from_utf8_lossy(&body).trim().to_string()
            };
            return Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                message,
            ));
        }

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
    let upstream_body = upstream.json::<Value>().await;
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

        let message = match &upstream_body {
            Ok(body) => body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or(&upstream_status.to_string())
                .to_string(),
            Err(_) => upstream_status.to_string(),
        };

        let mut record = RequestResponseRecord::new(
            endpoint_id,
            endpoint_name,
            UNSPECIFIED_IP,
            model,
            request_type,
            request_body,
            upstream_status,
            duration,
            client_ip,
            api_key_id,
        );
        record.status = RecordStatus::Error {
            message: format!("Upstream returned status {}", upstream_status),
        };
        save_request_record(state.request_history.clone(), record);

        let anthropic_status = match upstream_status.as_u16() {
            400 => StatusCode::BAD_REQUEST,
            401 => StatusCode::UNAUTHORIZED,
            403 => StatusCode::FORBIDDEN,
            404 => StatusCode::NOT_FOUND,
            429 => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_GATEWAY,
        };
        let error_type = match upstream_status.as_u16() {
            400 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            404 => "not_found_error",
            429 => "rate_limit_error",
            _ => "api_error",
        };

        return Ok(anthropic_error_response(
            anthropic_status,
            error_type,
            message,
        ));
    }

    match upstream_body {
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
                message: format!("Failed to parse OpenAI-compatible response: {}", err),
            };
            save_request_record(state.request_history.clone(), record);

            Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                "Failed to parse OpenAI-compatible upstream response",
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn transform_openai_streaming_response_to_anthropic(
    response: reqwest::Response,
    endpoint_id: Uuid,
    model_id: String,
    endpoint_type: crate::types::endpoint::EndpointType,
    request_started_at: Instant,
    input_tokens: Option<u32>,
    endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    load_manager: crate::balancer::LoadManager,
    event_bus: crate::events::SharedEventBus,
) -> Response {
    let headers = response.headers().clone();
    let mut accumulator = StreamingTokenAccumulator::new(&model_id);
    accumulator.set_input_tokens(input_tokens);

    let state = AnthropicStreamTracker {
        upstream: Box::pin(response.bytes_stream()),
        upstream_line_buffer: String::new(),
        output_queue: VecDeque::new(),
        accumulator,
        endpoint_id,
        model_id: model_id.clone(),
        endpoint_type,
        request_started_at,
        endpoint_registry,
        load_manager,
        event_bus,
        sent_message_start: false,
        sent_content_block_start: false,
        sent_content_block_stop: false,
        sent_message_stop: false,
        response_id: format!("msg_{}", Uuid::new_v4().simple()),
        public_model: model_id,
        stop_reason: None,
        stop_sequence: None,
        stats_recorded: false,
    };

    let transformed_stream = futures::stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.output_queue.pop_front() {
                return Ok(Some((chunk, state)));
            }

            match state.upstream.next().await {
                Some(Ok(chunk)) => {
                    let chunk_text = String::from_utf8_lossy(chunk.as_ref());
                    state.process_upstream_chunk(&chunk_text);
                }
                Some(Err(err)) => {
                    state.record_stats_once(false, TokenUsage::new(None, Some(0), Some(0)));
                    return Err(io::Error::other(err));
                }
                None => {
                    if !state.sent_message_stop {
                        state.finish_stream();
                        continue;
                    }
                    state.record_stats_once(true, state.accumulator.finalize());
                    return Ok(None);
                }
            }
        }
    });

    let mut response = Response::new(Body::from_stream(transformed_stream));
    *response.status_mut() = StatusCode::OK;
    for (name, value) in headers.iter() {
        if name == reqwest::header::CONTENT_LENGTH {
            continue;
        }
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response.headers_mut().insert(header_name, header_value);
        }
    }
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
}

impl AnthropicStreamTracker {
    fn process_upstream_chunk(&mut self, chunk_text: &str) {
        self.upstream_line_buffer.push_str(chunk_text);

        while let Some(newline_idx) = self.upstream_line_buffer.find('\n') {
            let line = self.upstream_line_buffer[..newline_idx]
                .trim_end_matches('\r')
                .to_string();
            self.upstream_line_buffer.drain(..=newline_idx);
            self.process_upstream_line(&line);
        }
    }

    fn process_upstream_line(&mut self, line: &str) {
        self.accumulator.process_chunk(line);

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return;
        }
        let Some(data) = trimmed.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();

        if data == "[DONE]" {
            self.finish_stream();
            return;
        }

        let Ok(json) = serde_json::from_str::<Value>(data) else {
            return;
        };

        if let Some(id) = json.get("id").and_then(Value::as_str) {
            self.response_id = id.replace("chatcmpl-", "msg_").replace("chatcmpl", "msg");
        }

        self.ensure_message_start();

        if let Some(choice) = json
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        {
            let delta = choice.get("delta");

            if let Some(content) = delta
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
            {
                self.ensure_content_block_start();
                if !content.is_empty() {
                    self.emit_event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {
                                "type": "text_delta",
                                "text": content
                            }
                        }),
                    );
                }
            }

            if let Some(tool_calls) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(Value::as_array)
            {
                if !tool_calls.is_empty() {
                    // Close text content block if it was open
                    if self.sent_content_block_start && !self.sent_content_block_stop {
                        self.sent_content_block_stop = true;
                        self.emit_event(
                            "content_block_stop",
                            json!({
                                "type": "content_block_stop",
                                "index": 0
                            }),
                        );
                    }

                    // Emit tool_use content blocks
                    for (idx, tool_call) in tool_calls.iter().enumerate() {
                        if let Some(tool_use) =
                            convert_openai_tool_call_to_anthropic_tool_use(tool_call)
                        {
                            let tool_index = 1 + idx; // Start from index 1 (0 is for text)

                            // Start tool_use content block
                            self.emit_event(
                                "content_block_start",
                                json!({
                                    "type": "content_block_start",
                                    "index": tool_index,
                                    "content_block": tool_use
                                }),
                            );

                            // Immediately stop tool_use content block (tools are complete in delta)
                            self.emit_event(
                                "content_block_stop",
                                json!({
                                    "type": "content_block_stop",
                                    "index": tool_index
                                }),
                            );
                        }
                    }
                }
            }

            if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.stop_reason = Some(map_finish_reason_to_stop_reason(finish_reason));
            }
        }
    }

    fn ensure_message_start(&mut self) {
        if self.sent_message_start {
            return;
        }
        self.sent_message_start = true;
        self.emit_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.response_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.public_model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": self.accumulator.finalize().input_tokens.unwrap_or(0),
                        "output_tokens": 0
                    }
                }
            }),
        );
    }

    fn ensure_content_block_start(&mut self) {
        if self.sent_content_block_start {
            return;
        }
        self.sent_content_block_start = true;
        self.emit_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        );
    }

    fn finish_stream(&mut self) {
        if self.sent_message_stop {
            return;
        }

        self.ensure_message_start();
        self.ensure_content_block_start();

        if !self.sent_content_block_stop {
            self.sent_content_block_stop = true;
            self.emit_event(
                "content_block_stop",
                json!({
                    "type": "content_block_stop",
                    "index": 0
                }),
            );
        }

        let usage = self.accumulator.finalize();
        self.emit_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": self.stop_reason.unwrap_or("end_turn"),
                    "stop_sequence": self.stop_sequence
                },
                "usage": {
                    "output_tokens": usage.output_tokens.unwrap_or(0)
                }
            }),
        );
        self.emit_event("message_stop", json!({ "type": "message_stop" }));
        self.sent_message_stop = true;
    }

    fn emit_event(&mut self, event_name: &str, data: Value) {
        let payload = format!("event: {}\ndata: {}\n\n", event_name, data);
        self.output_queue.push_back(Bytes::from(payload));
    }

    fn record_stats_once(&mut self, success: bool, usage: TokenUsage) {
        if self.stats_recorded {
            return;
        }
        self.stats_recorded = true;

        let output_tokens = usage.output_tokens.unwrap_or(0) as u64;
        let duration_ms = if output_tokens > 0 {
            self.request_started_at.elapsed().as_millis().max(1) as u64
        } else {
            0
        };

        record_endpoint_request_stats(
            self.endpoint_registry.clone(),
            self.endpoint_id,
            self.model_id.clone(),
            success,
            output_tokens,
            duration_ms,
            Some(TpsApiKind::ChatCompletions),
            self.endpoint_type,
            self.load_manager.clone(),
            self.event_bus.clone(),
        );
    }
}

#[allow(clippy::result_large_err)]
fn anthropic_request_to_openai(payload: &Value) -> Result<ConvertedAnthropicRequest, Response> {
    let model = extract_model(payload)?;
    let max_tokens = payload
        .get("max_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "max_tokens is required",
            )
        })?;
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message_values = payload
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "messages must be an array",
            )
        })?;

    let mut request_text_parts = Vec::new();
    let mut openai_messages = Vec::new();

    if let Some(system) = payload.get("system") {
        let system_text = flatten_anthropic_text_content(system, "system")?;
        if !system_text.is_empty() {
            openai_messages.push(json!({
                "role": "system",
                "content": system_text
            }));
            request_text_parts.push(format!("system: {}", system_text));
        }
    }

    for (index, message) in message_values.iter().enumerate() {
        let role = message.get("role").and_then(Value::as_str).ok_or_else(|| {
            anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("messages[{}].role is required", index),
            )
        })?;
        if !matches!(role, "user" | "assistant") {
            return Err(anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("messages[{}].role must be 'user' or 'assistant'", index),
            ));
        }

        let content = message.get("content").ok_or_else(|| {
            anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("messages[{}].content is required", index),
            )
        })?;

        // Handle assistant messages with tool_use content blocks (for future tool use handling)
        // For now, we skip processing tool_use blocks since OpenAI doesn't have them in messages
        if role == "assistant" {
            if let Some(content_array) = content.as_array() {
                let has_tool_use = content_array
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"));

                if has_tool_use {
                    // Skip processing assistant tool_use messages for now
                    // They will be handled through tool_calls in the response
                    continue;
                }
            }
        }

        // Handle tool_result content blocks in user messages
        if role == "user" {
            if let Some(content_array) = content.as_array() {
                let has_tool_result = content_array
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"));

                if has_tool_result {
                    // Convert Anthropic tool_result messages to OpenAI tool messages
                    for content_item in content_array {
                        if let Some("tool_result") =
                            content_item.get("type").and_then(Value::as_str)
                        {
                            let tool_use_id = content_item
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            let result_content = content_item
                                .get("content")
                                .and_then(Value::as_str)
                                .unwrap_or("");

                            openai_messages.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": result_content
                            }));
                            request_text_parts
                                .push(format!("tool_result[{}]: {}", tool_use_id, result_content));
                        }
                    }
                    continue; // Skip normal text processing for tool_result messages
                }
            }
        }

        let text =
            flatten_anthropic_text_content(content, &format!("messages[{}].content", index))?;
        openai_messages.push(json!({
            "role": role,
            "content": text
        }));
        request_text_parts.push(format!("{}: {}", role, text));
    }

    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model));
    body.insert("messages".to_string(), Value::Array(openai_messages));
    body.insert("max_tokens".to_string(), Value::Number(max_tokens.into()));
    body.insert("stream".to_string(), Value::Bool(stream));

    if let Some(temperature) = payload.get("temperature").and_then(Value::as_f64) {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = payload.get("top_p").and_then(Value::as_f64) {
        body.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(stop_sequences) = payload.get("stop_sequences") {
        body.insert(
            "stop".to_string(),
            normalize_stop_sequences(stop_sequences)?,
        );
    }

    // Convert Anthropic tools to OpenAI functions format
    if let Some(anthropic_tools) = payload.get("tools").and_then(Value::as_array) {
        let openai_tools: Result<Vec<_>, _> = anthropic_tools
            .iter()
            .map(convert_anthropic_tool_to_openai)
            .collect();
        body.insert("tools".to_string(), Value::Array(openai_tools?));
    }

    // Convert Anthropic tool_choice to OpenAI format
    if let Some(anthropic_tool_choice) = payload.get("tool_choice") {
        body.insert(
            "tool_choice".to_string(),
            convert_anthropic_tool_choice_to_openai(anthropic_tool_choice)?,
        );
    }

    Ok(ConvertedAnthropicRequest {
        openai_payload: Value::Object(body),
        request_text: request_text_parts.join("\n"),
        stream,
    })
}

#[allow(clippy::result_large_err)]
fn convert_anthropic_tool_to_openai(tool: &Value) -> Result<Value, Response> {
    let name = tool.get("name").and_then(Value::as_str).ok_or_else(|| {
        anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "tool.name is required",
        )
    })?;
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let input_schema = tool.get("input_schema").ok_or_else(|| {
        anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "tool.input_schema is required",
        )
    })?;

    // Convert input_schema (Anthropic format) to parameters (OpenAI format)
    let mut parameters = Map::new();
    if let Some(schema_type) = input_schema.get("type") {
        parameters.insert("type".to_string(), schema_type.clone());
    }
    if let Some(properties) = input_schema.get("properties") {
        parameters.insert("properties".to_string(), properties.clone());
    }
    if let Some(required) = input_schema.get("required") {
        parameters.insert("required".to_string(), required.clone());
    }

    Ok(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    }))
}

#[allow(clippy::result_large_err)]
fn convert_anthropic_tool_choice_to_openai(tool_choice: &Value) -> Result<Value, Response> {
    if let Some(tool_choice_type) = tool_choice.get("type").and_then(Value::as_str) {
        match tool_choice_type {
            "auto" => Ok(Value::String("auto".to_string())),
            "any" => Ok(Value::String("required".to_string())),
            "tool" => {
                let name = tool_choice
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anthropic_error_response(
                            StatusCode::BAD_REQUEST,
                            "invalid_request_error",
                            "tool_choice.name is required when type is 'tool'",
                        )
                    })?;
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": name
                    }
                }))
            }
            _ => Err(anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                format!("unknown tool_choice type: {}", tool_choice_type),
            )),
        }
    } else {
        Err(anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "tool_choice.type is required",
        ))
    }
}

#[allow(clippy::result_large_err)]
fn normalize_stop_sequences(value: &Value) -> Result<Value, Response> {
    let sequences = value.as_array().ok_or_else(|| {
        anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "stop_sequences must be an array of strings",
        )
    })?;
    let mut normalized = Vec::with_capacity(sequences.len());
    for item in sequences {
        let Some(sequence) = item.as_str() else {
            return Err(anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "stop_sequences must be an array of strings",
            ));
        };
        normalized.push(Value::String(sequence.to_string()));
    }
    Ok(Value::Array(normalized))
}

#[allow(clippy::result_large_err)]
fn flatten_anthropic_text_content(value: &Value, field_name: &str) -> Result<String, Response> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(items) => {
            let mut text = String::new();
            for item in items {
                let Some(item_type) = item.get("type").and_then(Value::as_str) else {
                    return Err(anthropic_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        format!("{} content blocks must have a type", field_name),
                    ));
                };
                if item_type != "text" {
                    return Err(anthropic_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        format!(
                            "{} content block type '{}' is not supported",
                            field_name, item_type
                        ),
                    ));
                }
                let Some(block_text) = item.get("text").and_then(Value::as_str) else {
                    return Err(anthropic_error_response(
                        StatusCode::BAD_REQUEST,
                        "invalid_request_error",
                        format!("{} text content blocks must include text", field_name),
                    ));
                };
                text.push_str(block_text);
            }
            Ok(text)
        }
        _ => Err(anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            format!("{} must be a string or text content array", field_name),
        )),
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

fn parse_anthropic_cloud_model(model: &str) -> Option<String> {
    if let Some(stripped) = model.strip_prefix("anthropic:") {
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    if let Some(stripped) = model.strip_prefix("ahtnorpic:") {
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    None
}

/// Convert OpenAI tool_call format to Anthropic tool_use content block format
fn convert_openai_tool_call_to_anthropic_tool_use(tool_call: &Value) -> Option<Value> {
    let func = tool_call.get("function")?;
    let tool_name = func.get("name").and_then(Value::as_str)?;
    let tool_id = tool_call.get("id").and_then(Value::as_str)?;
    let arguments_str = func
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");

    // Parse arguments JSON - if parsing fails, use empty object as fallback
    let input = serde_json::from_str(arguments_str).unwrap_or_else(|_| Value::Object(Map::new()));

    Some(json!({
        "type": "tool_use",
        "id": tool_id,
        "name": tool_name,
        "input": input
    }))
}

fn openai_to_anthropic_message_response(body: &Value, model: &str, usage: &TokenUsage) -> Value {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());

    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);

    let message = choice.and_then(|choice| choice.get("message"));

    let mut content = Vec::new();

    // Add text content if present
    let text = extract_openai_response_text(body);
    if !text.is_empty() {
        content.push(json!({
            "type": "text",
            "text": text
        }));
    }

    // Add tool_use blocks if tool_calls are present
    if let Some(tool_calls) = message
        .and_then(|msg| msg.get("tool_calls"))
        .and_then(Value::as_array)
    {
        for tool_call in tool_calls {
            if let Some(tool_use_block) = convert_openai_tool_call_to_anthropic_tool_use(tool_call)
            {
                content.push(tool_use_block);
            }
        }
    }

    // If no content was added, add an empty text block
    if content.is_empty() {
        content.push(json!({
            "type": "text",
            "text": ""
        }));
    }

    let stop_reason = if finish_reason == Some("tool_calls") {
        "tool_use"
    } else {
        finish_reason
            .map(map_finish_reason_to_stop_reason)
            .unwrap_or("end_turn")
    };

    json!({
        "id": body
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple())),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.input_tokens.unwrap_or(0),
            "output_tokens": usage.output_tokens.unwrap_or(0)
        }
    })
}

fn extract_openai_response_text(body: &Value) -> String {
    body.get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    choice
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .unwrap_or_default()
}

fn map_finish_reason_to_stop_reason(finish_reason: &str) -> &'static str {
    match finish_reason {
        "length" => "max_tokens",
        "stop" => "end_turn",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

fn build_response_from_upstream(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: Bytes,
) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    for (name, value) in headers.iter() {
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response.headers_mut().insert(header_name, header_value);
        }
    }
    if !response.headers().contains_key(header::CONTENT_TYPE) {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    response
}

fn anthropic_error_response(
    status: StatusCode,
    error_type: impl Into<String>,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type.into(),
                "message": message.into()
            }
        })),
    )
        .into_response()
}

fn anthropic_error_response_with_retry_after(
    status: StatusCode,
    error_type: &str,
    message: &str,
    retry_after: Option<u64>,
) -> Response {
    let mut response = anthropic_error_response(status, error_type, message);
    if let Some(retry_after) = retry_after {
        if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
            response
                .headers_mut()
                .insert(HeaderName::from_static("retry-after"), value);
        }
    }
    response
}

fn anthropic_error_from_lb_error(err: &LbError) -> Response {
    let status = err.status_code();
    match err {
        LbError::Common(CommonError::Validation(message)) => {
            anthropic_error_response(status, "invalid_request_error", message.clone())
        }
        LbError::Authentication(message) => {
            anthropic_error_response(status, "authentication_error", message.clone())
        }
        LbError::Authorization(message) => {
            anthropic_error_response(status, "permission_error", message.clone())
        }
        LbError::NotFound(message) | LbError::InvalidModelName(message) => {
            anthropic_error_response(status, "not_found_error", message.clone())
        }
        _ => anthropic_error_response(status, "api_error", err.external_message()),
    }
}

fn add_queue_headers(response: &mut Response, wait_ms: u128) {
    response.headers_mut().insert(
        HeaderName::from_static("x-queue-status"),
        HeaderValue::from_static("queued"),
    );
    if let Ok(value) = HeaderValue::from_str(&wait_ms.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-queue-wait-ms"), value);
    }
}

fn extract_client_info(
    addr: &SocketAddr,
    headers: &HeaderMap,
    auth_ctx: &Option<axum::Extension<ApiKeyAuthContext>>,
) -> (Option<IpAddr>, Option<Uuid>) {
    let client_ip = Some(
        extract_client_ip_from_headers(headers)
            .unwrap_or_else(|| crate::common::ip::normalize_socket_ip(addr)),
    );
    let api_key_id = auth_ctx.as_ref().map(|ext| ext.0.id);
    (client_ip, api_key_id)
}

fn extract_client_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    extract_x_forwarded_for(headers).or_else(|| extract_forwarded_for(headers))
}

fn extract_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value
        .split(',')
        .map(str::trim)
        .find_map(parse_client_ip_from_forwarded_value)
}

fn extract_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("forwarded")?.to_str().ok()?;
    value.split(',').find_map(|entry| {
        entry
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                if key.trim().eq_ignore_ascii_case("for") {
                    parse_client_ip_from_forwarded_value(value.trim())
                } else {
                    None
                }
            })
    })
}

fn parse_client_ip_from_forwarded_value(value: &str) -> Option<IpAddr> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") || trimmed.starts_with('_') {
        return None;
    }

    let host = if let Some(stripped) = trimmed.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default().trim()
    } else {
        trimmed
    };

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(crate::common::ip::normalize_ip(ip));
    }

    if let Some((ip_candidate, _port)) = host.rsplit_once(':') {
        if !ip_candidate.contains(':') {
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                return Some(crate::common::ip::normalize_ip(ip));
            }
        }
    }

    None
}

fn update_inference_latency(
    registry: &crate::registry::endpoints::EndpointRegistry,
    endpoint_id: Uuid,
    duration: std::time::Duration,
) {
    let registry = registry.clone();
    let latency_ms = duration.as_millis() as f64;
    tokio::spawn(async move {
        if let Err(err) = registry
            .update_inference_latency(endpoint_id, latency_ms)
            .await
        {
            tracing::debug!(
                endpoint_id = %endpoint_id,
                latency_ms = latency_ms,
                error = %err,
                "Failed to update inference latency"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::{TestAppStateBuilder, TEST_LOCK};
    use crate::types::endpoint::{
        Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
    };
    use crate::AppState;
    use axum::body::to_bytes;
    use axum::http::{HeaderMap, HeaderValue};
    use serial_test::serial;
    use tempfile::tempdir;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn create_local_state() -> AppState {
        TestAppStateBuilder::new().await.build().await
    }

    async fn create_state_with_tempdir() -> (AppState, tempfile::TempDir) {
        let dir = tempdir().expect("temp dir");
        std::env::set_var("LLMLB_DATA_DIR", dir.path());
        let state = create_local_state().await;
        (state, dir)
    }

    async fn add_online_chat_endpoint_with_model(
        state: &AppState,
        endpoint_name: &str,
        base_url: String,
        endpoint_type: EndpointType,
        model_id: &str,
        canonical_name: Option<&str>,
    ) {
        let mut endpoint = Endpoint::new(endpoint_name.to_string(), base_url, endpoint_type);
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: model_id.to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: canonical_name.map(str::to_string),
            })
            .await
            .expect("add endpoint model");
    }

    #[test]
    fn anthropic_request_to_openai_maps_system_and_messages() {
        let converted = anthropic_request_to_openai(&json!({
            "model": "test-model",
            "system": "You are helpful",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"}
            ],
            "max_tokens": 128,
            "temperature": 0.2,
            "top_p": 0.9,
            "stop_sequences": ["END"],
            "stream": true
        }))
        .expect("conversion should succeed");

        assert_eq!(converted.openai_payload["model"], "test-model");
        assert_eq!(converted.openai_payload["messages"][0]["role"], "system");
        assert_eq!(converted.openai_payload["messages"][1]["role"], "user");
        assert_eq!(converted.openai_payload["messages"][2]["role"], "assistant");
        assert_eq!(converted.openai_payload["max_tokens"], 128);
        assert_eq!(converted.openai_payload["stream"], true);
        assert_eq!(converted.openai_payload["stop"][0], "END");
        assert!(converted.request_text.contains("system: You are helpful"));
    }

    #[test]
    fn anthropic_request_to_openai_rejects_non_text_blocks() {
        let response = anthropic_request_to_openai(&json!({
            "model": "test-model",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "image", "source": {}}
                    ]
                }
            ],
            "max_tokens": 32
        }))
        .expect_err("non-text content must be rejected");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn openai_response_maps_to_anthropic_message_shape() {
        let usage = TokenUsage::new(Some(10), Some(6), Some(16));
        let response = openai_to_anthropic_message_response(
            &json!({
                "id": "chatcmpl-123",
                "choices": [
                    {
                        "message": {"role": "assistant", "content": "Hello from upstream"},
                        "finish_reason": "stop"
                    }
                ]
            }),
            "local-model",
            &usage,
        );

        assert_eq!(response["type"], "message");
        assert_eq!(response["role"], "assistant");
        assert_eq!(response["content"][0]["type"], "text");
        assert_eq!(response["content"][0]["text"], "Hello from upstream");
        assert_eq!(response["stop_reason"], "end_turn");
        assert_eq!(response["usage"]["input_tokens"], 10);
        assert_eq!(response["usage"]["output_tokens"], 6);
    }

    #[test]
    fn parse_anthropic_cloud_model_accepts_alias() {
        assert_eq!(
            parse_anthropic_cloud_model("anthropic:claude-3-7-sonnet"),
            Some("claude-3-7-sonnet".to_string())
        );
        assert_eq!(
            parse_anthropic_cloud_model("ahtnorpic:claude-3-7-sonnet"),
            Some("claude-3-7-sonnet".to_string())
        );
        assert_eq!(parse_anthropic_cloud_model("local-model"), None);
    }

    #[test]
    fn test_tools_request_conversion() {
        // Test that tools and tool_choice are accepted and converted
        let converted = anthropic_request_to_openai(&json!({
            "model": "test-model",
            "messages": [
                {"role": "user", "content": "Call the bash tool"}
            ],
            "max_tokens": 128,
            "tools": [
                {
                    "name": "bash",
                    "description": "Execute bash commands",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "description": "Command to execute"
                            }
                        },
                        "required": ["command"]
                    }
                }
            ],
            "tool_choice": {"type": "auto"}
        }))
        .expect("tools should be accepted and converted");

        // Verify OpenAI format
        let functions = converted
            .openai_payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools should be present in OpenAI payload");

        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0]["type"], "function");
        assert_eq!(functions[0]["function"]["name"], "bash");
        assert_eq!(
            functions[0]["function"]["description"],
            "Execute bash commands"
        );

        // Verify tool_choice conversion
        let tool_choice = converted
            .openai_payload
            .get("tool_choice")
            .expect("tool_choice should be converted");
        assert_eq!(tool_choice, "auto");
    }

    #[test]
    fn test_tool_result_message_conversion() {
        // Test that tool_result content blocks are converted properly
        let converted = anthropic_request_to_openai(&json!({
            "model": "test-model",
            "messages": [
                {"role": "user", "content": "Call the bash tool"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "toolu_123", "name": "bash", "input": {"command": "ls"}}
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_123", "content": "file1.txt\nfile2.txt"}
                    ]
                }
            ],
            "max_tokens": 128,
            "tools": [
                {
                    "name": "bash",
                    "description": "Execute bash commands",
                    "input_schema": {
                        "type": "object",
                        "properties": {"command": {"type": "string"}},
                        "required": ["command"]
                    }
                }
            ]
        }))
        .expect("tool_result conversion should succeed");

        // Verify that tool_result message is converted to tool role
        let messages = converted
            .openai_payload
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages should be array");

        // Find the tool_result message (should be converted to tool role)
        let tool_message = messages
            .iter()
            .find(|m| m.get("role").and_then(Value::as_str) == Some("tool"))
            .expect("tool_result should be converted to tool role message");

        assert_eq!(tool_message["tool_call_id"], "toolu_123");
        assert_eq!(tool_message["content"], "file1.txt\nfile2.txt");
    }

    #[test]
    fn test_tool_use_response_conversion() {
        // Test that OpenAI tool_calls are converted to Anthropic tool_use content blocks
        let usage = TokenUsage::new(Some(10), Some(20), Some(30));
        let response = openai_to_anthropic_message_response(
            &json!({
                "id": "chatcmpl-123",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "I'll execute the command for you.",
                            "tool_calls": [
                                {
                                    "id": "call_abc123",
                                    "type": "function",
                                    "function": {
                                        "name": "bash",
                                        "arguments": "{\"command\": \"ls -la\"}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ]
            }),
            "local-model",
            &usage,
        );

        // Verify response structure
        assert_eq!(response["type"], "message");
        assert_eq!(response["role"], "assistant");

        // Verify content blocks include both text and tool_use
        let content = response
            .get("content")
            .and_then(Value::as_array)
            .expect("content should be array");

        assert!(content.len() >= 2, "should have text and tool_use blocks");

        // Find tool_use block
        let tool_use = content
            .iter()
            .find(|c| c.get("type").and_then(Value::as_str) == Some("tool_use"))
            .expect("should have tool_use block");

        assert_eq!(tool_use["name"], "bash");
        assert_eq!(tool_use["id"], "call_abc123");
        assert_eq!(tool_use["input"]["command"], "ls -la");

        // Verify stop_reason
        assert_eq!(response["stop_reason"], "tool_use");
    }

    #[test]
    fn test_tool_use_streaming() {
        // Test that streaming tool call events are properly transformed
        // This test is a placeholder for streaming transformation logic
        // TODO: Implement streaming transformation and associated tests
    }

    #[tokio::test]
    #[serial]
    async fn canonical_model_routes_to_ollama_alias_backed_endpoint() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"model": "gpt-oss:20b"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-alias",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            })))
            .mount(&server)
            .await;

        add_online_chat_endpoint_with_model(
            &state,
            "ollama-alias",
            server.uri(),
            EndpointType::Ollama,
            "gpt-oss:20b",
            Some("openai/gpt-oss-20b"),
        )
        .await;

        let mut headers = HeaderMap::new();
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        let payload = json!({
            "model": "openai/gpt-oss-20b",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hello"}]
        });

        let response = handle_messages(
            std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
            headers,
            state.clone(),
            None,
            payload,
        )
        .await
        .expect("Anthropic request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("response body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json["model"], "openai/gpt-oss-20b");
        assert_eq!(json["content"][0]["text"], "ok");
    }

    #[tokio::test]
    #[serial]
    async fn canonical_model_routes_to_lmstudio_endpoint_without_alias_rewrite() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"model": "openai/gpt-oss-20b"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-canonical",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            })))
            .mount(&server)
            .await;

        add_online_chat_endpoint_with_model(
            &state,
            "lmstudio-canonical",
            server.uri(),
            EndpointType::LmStudio,
            "openai/gpt-oss-20b",
            Some("openai/gpt-oss-20b"),
        )
        .await;

        let mut headers = HeaderMap::new();
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        let payload = json!({
            "model": "openai/gpt-oss-20b",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hello"}]
        });

        let response = handle_messages(
            std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
            headers,
            state.clone(),
            None,
            payload,
        )
        .await
        .expect("Anthropic request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("response body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json["model"], "openai/gpt-oss-20b");
        assert_eq!(json["content"][0]["text"], "ok");
    }
}
