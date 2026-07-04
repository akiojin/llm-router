//! Open Responses API エンドポイント (/v1/responses)
//!
//! SPEC-0f1de549: OpenAI互換API完全準拠 - Open Responses API対応
//!
//! このモジュールは /v1/responses エンドポイントへのリクエストを
//! Responses API対応バックエンド（Ollama、vLLM、xLLM等）にパススルーする。

use crate::common::error::LbError;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType, TpsApiKind};
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    api::{
        error::AppError,
        model_name::rewrite_payload_model_for_endpoint,
        models::load_registered_model,
        openai::extract_client_info,
        openai_util::{sanitize_openai_payload_for_history, upstream_error_message_from_bytes},
        proxy::{
            forward_streaming_response, forward_streaming_response_with_tps_tracking,
            forward_to_endpoint, record_endpoint_request_stats, save_request_record,
            select_available_endpoint_with_queue_for_model, QueueSelection,
        },
    },
    auth::middleware::ApiKeyAuthContext,
    balancer::RequestOutcome,
    token::extract_usage_from_response,
    AppState,
};

/// 履歴記録でエンドポイントIPが未特定な場合（ストリーミング等）のフォールバック。
const UNSPECIFIED_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// SPEC-f8e3a1b7: 推論リクエスト成功時にエンドポイントのレイテンシを更新（Fire-and-forget）
fn update_inference_latency(
    registry: &crate::registry::endpoints::EndpointRegistry,
    endpoint_id: Uuid,
    duration: std::time::Duration,
) {
    let registry = registry.clone();
    let latency_ms = duration.as_millis() as f64;
    tokio::spawn(async move {
        if let Err(e) = registry
            .update_inference_latency(endpoint_id, latency_ms)
            .await
        {
            tracing::debug!(
                endpoint_id = %endpoint_id,
                latency_ms = latency_ms,
                error = %e,
                "Failed to update inference latency"
            );
        }
    });
}

fn openai_error_response(message: impl Into<String>, status: StatusCode) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "invalid_request_error",
            "code": status.as_u16(),
        }
    });

    (status, Json(payload)).into_response()
}

fn model_unavailable_response(message: impl Into<String>) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "service_unavailable",
            "code": "no_capable_nodes",
        }
    });

    (StatusCode::SERVICE_UNAVAILABLE, Json(payload)).into_response()
}

fn add_queue_headers(response: &mut Response, wait_ms: u128) {
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-queue-status"),
        HeaderValue::from_static("queued"),
    );
    let wait_value = wait_ms.to_string();
    if let Ok(value) = HeaderValue::from_str(&wait_value) {
        headers.insert(HeaderName::from_static("x-queue-wait-ms"), value);
    }
}

/// リクエストからモデル名を抽出
fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload["model"].as_str().map(String::from).ok_or_else(|| {
        AppError::from(LbError::Common(
            crate::common::error::CommonError::Validation("Missing required field: model".into()),
        ))
    })
}

/// リクエストからstreamフラグを抽出
fn extract_stream(payload: &Value) -> bool {
    payload["stream"].as_bool().unwrap_or(false)
}

/// POST /v1/responses - Open Responses API
///
/// リクエストをバックエンドにパススルーする（判定/フラグは廃止）。
pub async fn post_responses(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let model = extract_model(&payload)?;
    let stream = extract_stream(&payload);
    let tps_api_kind = Some(TpsApiKind::Responses);
    let request_type = RequestType::Responses;
    let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &auth_ctx);
    // payload は rewrite で move されるため、履歴用ボディを先に確保する。
    let request_body = sanitize_openai_payload_for_history(&payload);

    info!(
        model = %model,
        stream = stream,
        "Processing Responses API request"
    );

    // モデル名統一化: エイリアス名が渡された場合、正規名に変換して検索（openai.rs と同一）。
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

    // モデルが未登録の場合は404、登録済みなら503（利用可能エンドポイントなし）
    if !state.endpoint_registry.has_model(&resolved_model).await {
        let is_registered = load_registered_model(&state.db_pool, &resolved_model).await?;
        if is_registered.is_none() {
            let message = format!("The model '{}' does not exist", model);
            return Ok(openai_error_response(message, StatusCode::NOT_FOUND));
        }
    }

    // モデル対応エンドポイントをキュー付きで選択（モデル集合内で分散）
    let (endpoint, queued_wait_ms) =
        match select_available_endpoint_with_queue_for_model(&state, &resolved_model, tps_api_kind)
            .await
        {
            Ok(QueueSelection::Ready {
                endpoint,
                queued_wait_ms,
            }) => (*endpoint, queued_wait_ms),
            Err(e) => {
                let error_message = if matches!(e, LbError::NoCapableEndpoints(_)) {
                    format!("No available endpoints support model: {}", model)
                } else {
                    format!("Endpoint selection failed: {}", e)
                };
                error!(
                    endpoint = "/v1/responses",
                    model = %model,
                    error = %e,
                    "Failed to select available endpoint for Responses request"
                );
                // FR-004: エンドポイント選択失敗時もリクエスト履歴に記録する（openai.rs 準拠）。
                save_request_record(
                    state.request_history.clone(),
                    RequestResponseRecord::error(
                        model.clone(),
                        request_type,
                        request_body.clone(),
                        error_message.clone(),
                        0,
                        client_ip,
                        api_key_id,
                    ),
                );
                if matches!(e, LbError::NoCapableEndpoints(_)) {
                    return Ok(model_unavailable_response(error_message));
                }
                return Err(AppError::from(e));
            }
        };

    info!(
        endpoint_id = %endpoint.id,
        endpoint_name = %endpoint.name,
        "Forwarding to Responses API endpoint"
    );

    // リクエストボディをそのままパススルー
    let endpoint_models = match state.endpoint_registry.list_models(endpoint.id).await {
        Ok(models) => models,
        Err(error) => {
            warn!(
                endpoint_id = %endpoint.id,
                model = %model,
                error = %error,
                "Failed to load endpoint models for Responses request rewrite; falling back to static mapping"
            );
            Vec::new()
        }
    };
    let outbound_payload = rewrite_payload_model_for_endpoint(
        payload,
        &resolved_model,
        &endpoint.endpoint_type,
        &endpoint_models,
    );
    let body = serde_json::to_vec(&outbound_payload).map_err(|e| {
        error!("Failed to serialize request: {}", e);
        AppError::from(LbError::Http(e.to_string()))
    })?;

    let request_lease = state
        .load_manager
        .begin_request(endpoint.id)
        .await
        .map_err(AppError::from)?;

    // SPEC-f8e3a1b7: レイテンシ計測開始
    let start = Instant::now();

    // エンドポイントにリクエストを転送
    //
    // NOTE: Responses APIはレスポンス本文（ステータス含む）をそのまま返したい。
    // forward_to_endpoint() は上流の非2xxもそのまま返すため、エラー本文も
    // レスポンスとして受け取り、ステータスを保持してクライアントへ転送する。
    let response =
        match forward_to_endpoint(&state.http_client, &endpoint, "/v1/responses", body).await {
            Ok(response) => response,
            Err(e) => {
                let duration = start.elapsed();
                request_lease
                    .complete(RequestOutcome::Error, duration)
                    .await
                    .map_err(AppError::from)?;
                record_endpoint_request_stats(
                    state.endpoint_registry.clone(),
                    endpoint.id,
                    model.clone(),
                    false,
                    0,
                    0,
                    tps_api_kind,
                    endpoint.endpoint_type,
                    state.load_manager.clone(),
                    state.event_bus.clone(),
                );
                let mut record = RequestResponseRecord::new(
                    endpoint.id,
                    endpoint.name.clone(),
                    UNSPECIFIED_IP,
                    model.clone(),
                    request_type,
                    request_body.clone(),
                    StatusCode::BAD_GATEWAY,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: format!("Failed to forward Responses request: {}", e),
                };
                save_request_record(state.request_history.clone(), record);
                return Err(AppError::from(e));
            }
        };

    let duration = start.elapsed();
    let response_status = response.status();

    // ストリーミングの場合はそのままパススルー
    if stream {
        let succeeded = response_status.is_success();

        let mut axum_response = if succeeded {
            // ストリーミング成功はレスポンス本文を保持しないため、response_body/tokens を
            // 付けずにリクエスト履歴を1件記録する（openai.rs 準拠。TPS/統計はストリーム
            // 完走時に tracker が別途確定する）。
            let record = RequestResponseRecord::new(
                endpoint.id,
                endpoint.name.clone(),
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body.clone(),
                StatusCode::from_u16(response_status.as_u16()).unwrap_or(StatusCode::OK),
                duration,
                client_ip,
                api_key_id,
            );
            save_request_record(state.request_history.clone(), record);
            // lease と推論レイテンシ更新は forwarder に移譲し、ストリーム完走時に
            // 実時間で確定する（ヘッダー受信時点での早期解放を避ける: lease-ttfb）。
            forward_streaming_response_with_tps_tracking(
                response,
                endpoint.id,
                model.clone(),
                tps_api_kind,
                endpoint.endpoint_type,
                start,
                state.endpoint_registry.clone(),
                state.load_manager.clone(),
                state.event_bus.clone(),
                Some(request_lease),
            )
            .map_err(AppError::from)?
        } else {
            // 失敗時はトークンストリームが無いため、ここで lease を完了し統計を記録する
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint.id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint.endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );
            // ストリーミング失敗も履歴を1件記録する。上流本文の passthrough を維持する
            // ため本文は消費せず、非成功ステータスから Error を自動導出する（openai.rs 準拠）。
            let record = RequestResponseRecord::new(
                endpoint.id,
                endpoint.name.clone(),
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body.clone(),
                StatusCode::from_u16(response_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                duration,
                client_ip,
                api_key_id,
            );
            save_request_record(state.request_history.clone(), record);
            forward_streaming_response(response).map_err(AppError::from)?
        };
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut axum_response, wait_ms);
        }
        return Ok(axum_response);
    }

    // 非ストリーミングの場合
    let status = response.status();
    let headers = response.headers().clone();
    let body_bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read response body: {}", e);
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint.id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint.endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );
            let mut record = RequestResponseRecord::new(
                endpoint.id,
                endpoint.name.clone(),
                UNSPECIFIED_IP,
                model.clone(),
                request_type,
                request_body.clone(),
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: format!("Failed to read response body: {}", e),
            };
            save_request_record(state.request_history.clone(), record);
            return Err(AppError::from(LbError::Http(e.to_string())));
        }
    };

    let outcome = if status.is_success() {
        RequestOutcome::Success
    } else {
        RequestOutcome::Error
    };
    let succeeded = status.is_success();
    request_lease
        .complete(outcome, duration)
        .await
        .map_err(AppError::from)?;
    let (tps_output_tokens, tps_duration_ms) = if succeeded {
        serde_json::from_slice::<Value>(&body_bytes)
            .ok()
            .and_then(|body| extract_usage_from_response(&body))
            .and_then(|usage| usage.output_tokens)
            .map(|tokens| tokens as u64)
            .map(|tokens| {
                let duration_ms = if tokens > 0 {
                    duration.as_millis().max(1) as u64
                } else {
                    0
                };
                (tokens, duration_ms)
            })
            .unwrap_or((0, 0))
    } else {
        (0, 0)
    };
    record_endpoint_request_stats(
        state.endpoint_registry.clone(),
        endpoint.id,
        model.clone(),
        succeeded,
        tps_output_tokens,
        tps_duration_ms,
        tps_api_kind,
        endpoint.endpoint_type,
        state.load_manager.clone(),
        state.event_bus.clone(),
    );

    // SPEC-f8e3a1b7: 成功時に推論レイテンシを更新
    if status.is_success() {
        update_inference_latency(&state.endpoint_registry, endpoint.id, duration);
    }

    // リクエスト履歴を1件記録する（openai.rs 準拠）。成功時は response_body と
    // usage トークンを付与し、非2xx は上流本文から Error メッセージを導出する。
    {
        let record_status =
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let mut record = RequestResponseRecord::new(
            endpoint.id,
            endpoint.name.clone(),
            UNSPECIFIED_IP,
            model.clone(),
            request_type,
            request_body.clone(),
            record_status,
            duration,
            client_ip,
            api_key_id,
        );
        if succeeded {
            if let Ok(body) = serde_json::from_slice::<Value>(&body_bytes) {
                if let Some(usage) = extract_usage_from_response(&body) {
                    record.input_tokens = usage.input_tokens;
                    record.output_tokens = usage.output_tokens;
                    record.total_tokens = usage.total_tokens;
                }
                record.response_body = Some(body);
            }
        } else {
            record.status = RecordStatus::Error {
                message: upstream_error_message_from_bytes(record_status, &body_bytes),
            };
        }
        save_request_record(state.request_history.clone(), record);
    }

    // バックエンドのレスポンス（ステータス/ヘッダ/本文）をパススルー
    let mut axum_response = Response::new(Body::from(body_bytes));
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

    if let Some(wait_ms) = queued_wait_ms {
        add_queue_headers(&mut axum_response, wait_ms);
    }

    Ok(axum_response)
}

#[cfg(test)]
mod tests {
    use super::post_responses;
    use crate::{
        common::protocol::{RecordStatus, RequestType},
        db::test_utils::TestAppStateBuilder,
        types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI},
        AppState,
    };
    use axum::{body::to_bytes, extract::State, http::StatusCode, Json};
    use serde_json::json;
    use tokio::time::{sleep, Duration};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn create_local_state() -> AppState {
        TestAppStateBuilder::new().await.build().await
    }

    async fn register_vllm_endpoint(
        state: &AppState,
        base_url: String,
        model_id: &str,
    ) -> uuid::Uuid {
        let mut endpoint = Endpoint::new(
            "responses-test-endpoint".to_string(),
            base_url,
            EndpointType::Vllm,
        );
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
                supported_apis: vec![SupportedAPI::Responses],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");
        endpoint_id
    }

    #[tokio::test]
    async fn responses_non_stream_success_updates_model_tps() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        let response_body = json!({
            "id": "resp_123",
            "object": "response",
            "usage": {
                "input_tokens": 8,
                "output_tokens": 12,
                "total_tokens": 20
            }
        });
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let endpoint_id = register_vllm_endpoint(&state, server.uri(), "responses-tps-model").await;

        let response = post_responses(
            axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({
                "model": "responses-tps-model",
                "input": "hello"
            })),
        )
        .await
        .expect("responses request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("response body should be readable");

        sleep(Duration::from_millis(100)).await;

        let tps = state.load_manager.get_model_tps(endpoint_id).await;
        let entry = tps
            .iter()
            .find(|info| info.model_id == "responses-tps-model")
            .expect("responses model should have TPS entry");
        assert!(entry.tps.is_some(), "TPS should be updated");
        assert!(
            entry.total_output_tokens >= 12,
            "usage.output_tokens should be accumulated"
        );
    }

    #[tokio::test]
    async fn responses_stream_success_updates_model_tps_after_completion() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(stream_body, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let endpoint_id =
            register_vllm_endpoint(&state, server.uri(), "responses-stream-model").await;

        let response = post_responses(
            axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({
                "model": "responses-stream-model",
                "input": "hello",
                "stream": true
            })),
        )
        .await
        .expect("streaming responses request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("stream body should be readable");

        sleep(Duration::from_millis(120)).await;

        let tps = state.load_manager.get_model_tps(endpoint_id).await;
        let entry = tps
            .iter()
            .find(|info| info.model_id == "responses-stream-model")
            .expect("streaming model should have TPS entry");
        assert!(
            entry.tps.is_some(),
            "TPS should be updated after stream completion"
        );
        assert!(
            entry.total_output_tokens > 0,
            "streaming output tokens should be accumulated"
        );
    }

    #[tokio::test]
    async fn responses_interrupted_stream_still_records_success_stats() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(stream_body, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let endpoint_id =
            register_vllm_endpoint(&state, server.uri(), "responses-stream-interrupted").await;

        let response = post_responses(
            axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({
                "model": "responses-stream-interrupted",
                "input": "hello",
                "stream": true
            })),
        )
        .await
        .expect("streaming responses request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        // Simulate client disconnect before consuming the full stream.
        drop(response);

        sleep(Duration::from_millis(120)).await;

        let endpoint = crate::db::endpoints::get_endpoint(&state.db_pool, endpoint_id)
            .await
            .expect("get endpoint should succeed")
            .expect("endpoint should exist");
        assert_eq!(endpoint.total_requests, 1);
        assert_eq!(endpoint.successful_requests, 1);
        assert_eq!(endpoint.failed_requests, 0);

        let model_stats =
            crate::db::endpoint_daily_stats::get_model_stats(&state.db_pool, endpoint_id)
                .await
                .expect("get model stats");
        let stat = model_stats
            .iter()
            .find(|s| s.model_id == "responses-stream-interrupted")
            .expect("model stats should exist for interrupted stream");
        assert_eq!(stat.total_requests, 1);
        assert_eq!(stat.successful_requests, 1);
        assert_eq!(stat.failed_requests, 0);
    }

    // --- request history recording tests (finding [C1]) ---

    fn test_connect_info() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
    }

    #[tokio::test]
    async fn responses_non_stream_success_records_history() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        let response_body = json!({
            "id": "resp_1",
            "object": "response",
            "usage": {"input_tokens": 5, "output_tokens": 9, "total_tokens": 14}
        });
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;
        register_vllm_endpoint(&state, server.uri(), "resp-hist-model").await;

        let response = post_responses(
            test_connect_info(),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({"model": "resp-hist-model", "input": "hello"})),
        )
        .await
        .expect("responses request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

        sleep(Duration::from_millis(150)).await;
        let records = state
            .request_history
            .load_records()
            .await
            .expect("load records");
        assert_eq!(records.len(), 1, "one history record expected");
        let rec = &records[0];
        assert_eq!(rec.model, "resp-hist-model");
        assert_eq!(rec.request_type, RequestType::Responses);
        assert!(matches!(rec.status, RecordStatus::Success));
        assert!(
            rec.response_body.is_some(),
            "success should keep response_body"
        );
        assert_eq!(rec.output_tokens, Some(9));
    }

    #[tokio::test]
    async fn responses_non_stream_error_records_error_history() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(500).set_body_string("upstream boom"))
            .mount(&server)
            .await;
        register_vllm_endpoint(&state, server.uri(), "resp-err-model").await;

        let response = post_responses(
            test_connect_info(),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({"model": "resp-err-model", "input": "x"})),
        )
        .await
        .expect("passthrough should return a response");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

        sleep(Duration::from_millis(150)).await;
        let records = state.request_history.load_records().await.unwrap();
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].status, RecordStatus::Error { .. }));
        assert!(records[0].response_body.is_none());
    }

    #[tokio::test]
    async fn responses_stream_success_records_history_without_body() {
        let state = create_local_state().await;
        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(stream_body, "text/event-stream"),
            )
            .mount(&server)
            .await;
        register_vllm_endpoint(&state, server.uri(), "resp-stream-hist").await;

        let response = post_responses(
            test_connect_info(),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({"model": "resp-stream-hist", "input": "hi", "stream": true})),
        )
        .await
        .expect("streaming responses request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

        sleep(Duration::from_millis(150)).await;
        let records = state.request_history.load_records().await.unwrap();
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].status, RecordStatus::Success));
        assert_eq!(records[0].request_type, RequestType::Responses);
        assert!(
            records[0].response_body.is_none(),
            "streaming success should not carry response_body"
        );
    }

    #[tokio::test]
    async fn responses_unregistered_model_records_nothing() {
        let state = create_local_state().await;
        let response = post_responses(
            test_connect_info(),
            axum::http::HeaderMap::new(),
            State(state.clone()),
            None,
            Json(json!({"model": "totally-unknown-model", "input": "x"})),
        )
        .await
        .expect("should return a 404 response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        sleep(Duration::from_millis(100)).await;
        let records = state.request_history.load_records().await.unwrap();
        assert!(
            records.is_empty(),
            "404 (unregistered model) must not record history"
        );
    }

    // --- extract_model tests ---

    #[test]
    fn extract_model_returns_model_string() {
        let payload = json!({"model": "gpt-4", "input": "hello"});
        let model = super::extract_model(&payload).expect("should extract model");
        assert_eq!(model, "gpt-4");
    }

    #[test]
    fn extract_model_returns_error_when_missing() {
        let payload = json!({"input": "hello"});
        let result = super::extract_model(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn extract_model_returns_error_for_non_string() {
        let payload = json!({"model": 42, "input": "hello"});
        let result = super::extract_model(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn extract_model_returns_error_for_null() {
        let payload = json!({"model": null});
        let result = super::extract_model(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn extract_model_returns_error_for_array_model() {
        let payload = json!({"model": ["gpt-4"]});
        let result = super::extract_model(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn extract_model_returns_error_for_empty_object() {
        let payload = json!({});
        let result = super::extract_model(&payload);
        assert!(result.is_err());
    }

    // --- extract_stream tests ---

    #[test]
    fn extract_stream_returns_true_when_set() {
        let payload = json!({"model": "test", "stream": true});
        assert!(super::extract_stream(&payload));
    }

    #[test]
    fn extract_stream_returns_false_when_set_false() {
        let payload = json!({"model": "test", "stream": false});
        assert!(!super::extract_stream(&payload));
    }

    #[test]
    fn extract_stream_defaults_to_false_when_missing() {
        let payload = json!({"model": "test"});
        assert!(!super::extract_stream(&payload));
    }

    #[test]
    fn extract_stream_defaults_to_false_for_non_boolean() {
        let payload = json!({"model": "test", "stream": "yes"});
        assert!(!super::extract_stream(&payload));
    }

    #[test]
    fn extract_stream_defaults_to_false_for_integer() {
        let payload = json!({"model": "test", "stream": 1});
        assert!(!super::extract_stream(&payload));
    }

    #[test]
    fn extract_stream_defaults_to_false_for_null() {
        let payload = json!({"model": "test", "stream": null});
        assert!(!super::extract_stream(&payload));
    }

    // --- openai_error_response tests ---

    #[test]
    fn openai_error_response_returns_requested_status() {
        let resp = super::openai_error_response("bad request", StatusCode::BAD_REQUEST);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn openai_error_response_returns_404() {
        let resp = super::openai_error_response("not found", StatusCode::NOT_FOUND);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn openai_error_response_returns_500() {
        let resp =
            super::openai_error_response("internal error", StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // --- model_unavailable_response tests ---

    #[test]
    fn model_unavailable_response_returns_503() {
        let resp = super::model_unavailable_response("no endpoints for model X");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn model_unavailable_response_accepts_owned_string() {
        let msg = format!("No available endpoints support model: {}", "llama3");
        let resp = super::model_unavailable_response(msg);
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // --- add_queue_headers tests ---

    #[test]
    fn add_queue_headers_sets_status_and_wait() {
        use axum::http::HeaderName;

        let mut resp = axum::response::Response::new(axum::body::Body::empty());
        super::add_queue_headers(&mut resp, 150);

        assert_eq!(
            resp.headers()
                .get(HeaderName::from_static("x-queue-status"))
                .map(|v| v.to_str().unwrap()),
            Some("queued")
        );
        assert_eq!(
            resp.headers()
                .get(HeaderName::from_static("x-queue-wait-ms"))
                .map(|v| v.to_str().unwrap()),
            Some("150")
        );
    }

    #[test]
    fn add_queue_headers_zero_wait() {
        use axum::http::HeaderName;

        let mut resp = axum::response::Response::new(axum::body::Body::empty());
        super::add_queue_headers(&mut resp, 0);

        assert_eq!(
            resp.headers()
                .get(HeaderName::from_static("x-queue-wait-ms"))
                .map(|v| v.to_str().unwrap()),
            Some("0")
        );
    }

    #[test]
    fn add_queue_headers_large_wait_value() {
        use axum::http::HeaderName;

        let mut resp = axum::response::Response::new(axum::body::Body::empty());
        super::add_queue_headers(&mut resp, 999_999);

        assert_eq!(
            resp.headers()
                .get(HeaderName::from_static("x-queue-wait-ms"))
                .map(|v| v.to_str().unwrap()),
            Some("999999")
        );
    }
}
