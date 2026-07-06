//! ローカルエンドポイントへの OpenAI 互換 POST 転送パイプライン。
//!
//! キュー選択・ペイロード書き換え・ストリーミング/非ストリーミング転送・
//! TPS 計測・リクエスト履歴記録・レイテンシ更新を担う中核処理。

use super::{
    parse_cloud_model, payload_requires_image_input, proxy_openai_cloud_post, UNSPECIFIED_IP,
};
use crate::api::error::AppError;
use crate::api::model_name::rewrite_payload_model_for_endpoint;
use crate::api::models::load_registered_model;
use crate::api::openai_util::{
    classify_upstream_request_error, model_unavailable_response, openai_error_response,
    openai_error_response_with_type, probe_ollama_model_loaded, read_capped_body,
    sanitize_openai_payload_for_history, upstream_error_message_from_bytes,
    UPSTREAM_ERROR_SUMMARY_MAX_BYTES,
};
use crate::api::proxy::{
    add_queue_headers, forward_streaming_response_with_tps_tracking, record_endpoint_request_stats,
    save_request_record, select_available_endpoint_with_queue_for_model,
    select_available_endpoint_with_queue_for_model_and_api, update_inference_latency,
    QueueSelection,
};
use crate::balancer::RequestOutcome;
use crate::common::error::LbError;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType, TpsApiKind};
use crate::token::extract_usage_from_response;
use crate::types::endpoint::SupportedAPI;
use crate::AppState;
use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::time::Instant;
use tracing::{error, warn};
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub(super) async fn proxy_openai_post(
    state: &AppState,
    payload: Value,
    target_path: &str,
    model: String,
    stream: bool,
    request_type: RequestType,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    // Cloud-prefixed model -> forward to provider API
    if parse_cloud_model(&model).is_some() {
        return proxy_openai_cloud_post(
            state,
            target_path,
            &model,
            stream,
            payload,
            request_type,
            client_ip,
            api_key_id,
        )
        .await;
    }

    // モデル名統一化: エイリアス名が渡された場合、正規名に変換して検索
    let resolved_model = {
        let found = state.endpoint_registry.find_by_model(&model).await;
        if found.is_empty() {
            // エイリアス名で見つからない場合、マッピングテーブルで正規名を解決
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

    // Check if any endpoint has this model
    if !state.endpoint_registry.has_model(&resolved_model).await {
        let is_registered = load_registered_model(&state.db_pool, &resolved_model).await?;
        if is_registered.is_none() {
            let message = format!("The model '{}' does not exist", model);
            return Ok(openai_error_response(message, StatusCode::NOT_FOUND));
        }
    }

    let request_body = sanitize_openai_payload_for_history(&payload);
    let tps_api_kind = TpsApiKind::from_request_type(request_type);
    let required_supported_api = if payload_requires_image_input(&payload) {
        Some(SupportedAPI::ImageInput)
    } else {
        None
    };
    let mut queued_wait_ms: Option<u128> = None;

    // FR-004: エンドポイント選択失敗時もリクエスト履歴に記録する
    let endpoint = match if let Some(required_api) = required_supported_api {
        select_available_endpoint_with_queue_for_model_and_api(
            state,
            &resolved_model,
            required_api,
            tps_api_kind,
        )
        .await
    } else {
        select_available_endpoint_with_queue_for_model(state, &resolved_model, tps_api_kind).await
    } {
        Ok(QueueSelection::Ready {
            endpoint,
            queued_wait_ms: wait_ms,
        }) => {
            queued_wait_ms = wait_ms;
            *endpoint
        }
        Err(e) => {
            let error_message = if matches!(e, LbError::NoCapableEndpoints(_)) {
                format!("No available nodes support model: {}", model)
            } else {
                format!("Node selection failed: {}", e)
            };
            error!(
                endpoint = %target_path,
                model = %model,
                error = %e,
                "Failed to select available node"
            );
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    error_message.clone(),
                    queued_wait_ms.unwrap_or(0) as u64,
                    client_ip,
                    api_key_id,
                ),
            );
            if matches!(e, LbError::NoCapableEndpoints(_)) {
                return Ok(model_unavailable_response(
                    error_message,
                    "no_capable_nodes",
                ));
            }
            return Err(e.into());
        }
    };
    let endpoint_id = endpoint.id;
    let endpoint_name = endpoint.name.clone();
    let endpoint_type = endpoint.endpoint_type;
    // RequestResponseRecordの互換性のため、デフォルトIP使用
    // (今後、RequestResponseRecordのフィールドをリネームすべき)
    let endpoint_host: std::net::IpAddr = UNSPECIFIED_IP;

    let request_lease = state
        .load_manager
        .begin_request(endpoint_id)
        .await
        .map_err(AppError::from)?;

    let client = state.http_client.clone();
    let runtime_url = format!("{}{}", endpoint.base_url.trim_end_matches('/'), target_path);
    let start = Instant::now();
    let endpoint_models = match state.endpoint_registry.list_models(endpoint_id).await {
        Ok(models) => models,
        Err(error) => {
            warn!(
                endpoint_id = %endpoint_id,
                model = %resolved_model,
                error = %error,
                "Failed to load endpoint models for request rewrite; falling back to static mapping"
            );
            Vec::new()
        }
    };
    let outbound_payload = rewrite_payload_model_for_endpoint(
        payload,
        &resolved_model,
        &endpoint_type,
        &endpoint_models,
    );

    // 上の list_models 結果を再利用する（1リクエストあたりの list_models 呼び出しを
    // 2回から1回に削減）。取得失敗時は endpoint_models が空 Vec のため find は None を
    // 返し、従来どおり resolve_engine_name のフォールバックに流れる。
    let upstream_model = endpoint_models
        .iter()
        .find(|endpoint_model| {
            endpoint_model.model_id == model
                || endpoint_model.model_id == resolved_model
                || endpoint_model.canonical_name.as_deref() == Some(model.as_str())
                || endpoint_model.canonical_name.as_deref() == Some(resolved_model.as_str())
        })
        .map(|endpoint_model| endpoint_model.model_id.clone())
        .or_else(|| {
            crate::models::mapping::resolve_engine_name(&model, &endpoint_type).map(str::to_string)
        })
        .or_else(|| {
            crate::models::mapping::resolve_engine_name(&resolved_model, &endpoint_type)
                .map(str::to_string)
        })
        .unwrap_or_else(|| resolved_model.clone());

    let mut upstream_payload = outbound_payload;
    if let Some(payload_object) = upstream_payload.as_object_mut() {
        payload_object.insert("model".to_string(), Value::String(upstream_model.clone()));

        // SPEC-8c32349f: ストリーミングリクエストに stream_options.include_usage を注入
        // Ollama 等のエンドポイントが最終チャンクに usage を含めるようにする
        if stream {
            if let Some(opts) = payload_object
                .entry("stream_options".to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()
            {
                opts.entry("include_usage".to_string())
                    .or_insert(json!(true));
            }
        }
    }

    let mut request_builder = client
        .post(&runtime_url)
        .timeout(std::time::Duration::from_secs(
            endpoint.inference_timeout_secs as u64,
        ))
        .json(&upstream_payload);
    if let Some(api_key) = &endpoint.api_key {
        request_builder = request_builder.bearer_auth(api_key);
    }

    let response = match request_builder.send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            let ollama_loading_model = if e.is_timeout()
                && endpoint_type == crate::types::endpoint::EndpointType::Ollama
            {
                match probe_ollama_model_loaded(
                    &client,
                    &endpoint.base_url,
                    endpoint.api_key.as_deref(),
                    &upstream_model,
                )
                .await
                {
                    Some(false) => Some(upstream_model.clone()),
                    _ => None,
                }
            } else {
                None
            };
            let classified_error = classify_upstream_request_error(
                &e,
                &runtime_url,
                endpoint.inference_timeout_secs,
                ollama_loading_model.as_deref(),
            );
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

            // Note: Model exclusion is handled by the health check system
            // which will mark the endpoint as offline/error if requests fail repeatedly

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name.clone(),
                    endpoint_host,
                    model.clone(),
                    request_type,
                    request_body.clone(),
                    classified_error.status_code,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: classified_error.record_message,
                };
                save_request_record(state.request_history.clone(), record);
            }

            let mut response = openai_error_response_with_type(
                classified_error.client_message,
                classified_error.error_type,
                classified_error.status_code,
            );
            if let Some(wait_ms) = queued_wait_ms {
                add_queue_headers(&mut response, wait_ms);
            }
            return Ok(response);
        }
    };

    // ストリームの場合はレスポンスをそのままパススルー
    if stream {
        let duration = start.elapsed();
        let succeeded = response.status().is_success();

        let mut axum_response = if succeeded {
            {
                let record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name.clone(),
                    endpoint_host,
                    model.clone(),
                    request_type,
                    request_body.clone(),
                    response.status(),
                    duration,
                    client_ip,
                    api_key_id,
                );
                save_request_record(state.request_history.clone(), record);
            }
            // lease と推論レイテンシ更新は forwarder に移譲し、ストリーム完走時に
            // 実時間で確定する（ヘッダー受信時点での早期解放を避ける: lease-ttfb）。
            forward_streaming_response_with_tps_tracking(
                response,
                endpoint_id,
                model.clone(),
                tps_api_kind,
                endpoint_type,
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

            let status = response.status();
            let headers = response.headers().clone();
            let body_bytes = response.bytes().await.unwrap_or_default();
            let message = upstream_error_message_from_bytes(status, &body_bytes);

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                endpoint_host,
                model.clone(),
                request_type,
                request_body.clone(),
                status,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);

            let mut response = Response::new(Body::from(body_bytes));
            *response.status_mut() = status;
            {
                let response_headers = response.headers_mut();
                for (name, value) in headers.iter() {
                    if let (Ok(header_name), Ok(header_value)) = (
                        HeaderName::from_bytes(name.as_str().as_bytes()),
                        HeaderValue::from_bytes(value.as_bytes()),
                    ) {
                        response_headers.insert(header_name, header_value);
                    }
                }
            }
            if !response
                .headers()
                .contains_key(axum::http::header::CONTENT_TYPE)
            {
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
            response
        };
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut axum_response, wait_ms);
        }
        return Ok(axum_response);
    }

    if !response.status().is_success() {
        let duration = start.elapsed();
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

        // Note: Model exclusion is handled by the health check system
        // which will mark the endpoint as offline/error if requests fail repeatedly

        let status = response.status();
        // OpenAI互換経路では upstream 非2xx は 502 に正規化して返す
        let status_code = StatusCode::BAD_GATEWAY;
        // ボディはエラー要約にしか使わず、クライアントには新規 JSON を返すため、
        // 巨大ボディによるメモリ枯渇を避けて先頭部分のみ読み取る。
        let body_bytes = read_capped_body(response, UPSTREAM_ERROR_SUMMARY_MAX_BYTES).await;
        let message = upstream_error_message_from_bytes(status, &body_bytes);

        {
            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                endpoint_host,
                model.clone(),
                request_type,
                request_body.clone(),
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);
        }

        let payload = json!({
            "error": {
                "message": message,
                "type": "endpoint_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        let mut response = (status_code, Json(payload)).into_response();
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut response, wait_ms);
        }
        return Ok(response);
    }

    let parsed = response.json::<Value>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(mut body) => {
            if let Some(body_object) = body.as_object_mut() {
                body_object.insert("model".to_string(), Value::String(model.clone()));
            }

            // レスポンスからトークン使用量を抽出
            let token_usage = extract_usage_from_response(&body);

            request_lease
                .complete_with_tokens(RequestOutcome::Success, duration, token_usage.clone())
                .await
                .map_err(AppError::from)?;
            // SPEC-f8e3a1b7: 成功時に推論レイテンシを更新
            update_inference_latency(&state.endpoint_registry, endpoint_id, duration);
            // SPEC-4bb5b55f: TPS計測用にoutput_tokensとdurationを渡す
            let tps_output_tokens = token_usage
                .as_ref()
                .and_then(|u| u.output_tokens)
                .unwrap_or(0) as u64;
            let tps_duration_ms = if tps_output_tokens > 0 {
                duration.as_millis().max(1) as u64
            } else {
                0
            };
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                true,
                tps_output_tokens,
                tps_duration_ms,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            // RequestResponseRecordにトークン情報を保存
            let (input_tokens, output_tokens, total_tokens) = token_usage
                .as_ref()
                .map(|u| (u.input_tokens, u.output_tokens, u.total_tokens))
                .unwrap_or((None, None, None));

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_host,
                    model,
                    request_type,
                    request_body,
                    StatusCode::OK,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.response_body = Some(body.clone());
                record.input_tokens = input_tokens;
                record.output_tokens = output_tokens;
                record.total_tokens = total_tokens;
                save_request_record(state.request_history.clone(), record);
            }

            let mut response = (StatusCode::OK, Json(body)).into_response();
            if let Some(wait_ms) = queued_wait_ms {
                add_queue_headers(&mut response, wait_ms);
            }
            Ok(response)
        }
        Err(e) => {
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

            // Note: Model exclusion is handled by the health check system
            // which will mark the endpoint as offline/error if requests fail repeatedly

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_host,
                    model,
                    request_type,
                    request_body,
                    StatusCode::BAD_GATEWAY,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: format!("Failed to parse OpenAI response: {}", e),
                };
                save_request_record(state.request_history.clone(), record);
            }

            Err(LbError::Http(format!("Failed to parse OpenAI response: {}", e)).into())
        }
    }
}
