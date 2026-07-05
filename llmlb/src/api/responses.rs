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
    response::Response,
    Json,
};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Instant;

mod helpers;
use helpers::{
    add_queue_headers, extract_model, extract_stream, model_unavailable_response,
    openai_error_response,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    api::{
        error::AppError,
        model_name::rewrite_payload_model_for_endpoint,
        models::load_registered_model,
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
    let (client_ip, api_key_id) =
        crate::common::http::extract_client_info(&addr, &headers, &auth_ctx);
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
mod tests;
