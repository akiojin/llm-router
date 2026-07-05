//! ダッシュボード Playground のチャットプロキシ
//!
//! arch-review [H6] round2: api/endpoints.rs から単一エンドポイントへの
//! /v1/chat/completions 転送・ストリーミング・状態更新・上流エラー分類を分離。

use crate::api::error::AppError;
use crate::api::openai_util::{
    classify_upstream_request_error, openai_error_response_with_type, probe_ollama_model_loaded,
};
use crate::common::error::LbError;
use crate::common::http::RequestBuilderBearerExt;
use crate::db::endpoints as db;
use crate::types::endpoint::EndpointStatus;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// POST /api/endpoints/:id/chat/completions - エンドポイントへのチャットプロキシ
///
/// ダッシュボードのPlayground用。JWT認証済みユーザーが直接エンドポイントと通信できる。
/// リクエストをそのままエンドポイントの`/v1/chat/completions`に転送する。
pub async fn proxy_chat_completions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // エンドポイントを取得
    let endpoint = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for proxy: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    // エンドポイントがオンラインか確認
    if endpoint.status != EndpointStatus::Online {
        return AppError(LbError::ServiceUnavailable(format!(
            "Endpoint is not online (status: {:?})",
            endpoint.status
        )))
        .into_response();
    }

    // リクエストを転送
    let url = format!(
        "{}/v1/chat/completions",
        endpoint.base_url.trim_end_matches('/')
    );
    let request_model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|payload| {
            payload
                .get("model")
                .and_then(|model| model.as_str())
                .map(str::to_string)
        });

    let mut request = state
        .http_client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_vec());
    let started_at = Instant::now();

    request = request.bearer_opt(endpoint.api_key.as_ref());

    let result = request
        .timeout(Duration::from_secs(endpoint.inference_timeout_secs as u64))
        .send()
        .await;

    match result {
        Ok(response) => {
            // reqwest::StatusCode -> axum::http::StatusCode
            let status_code = StatusCode::from_u16(response.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let latency_ms = u32::try_from(started_at.elapsed().as_millis()).unwrap_or(u32::MAX);
            if status_code.is_success() {
                let _ = state
                    .endpoint_registry
                    .update_status(endpoint.id, EndpointStatus::Online, Some(latency_ms), None)
                    .await;
            } else if status_code.is_server_error() {
                let error_msg = format!("HTTP {}", status_code);
                let _ = state
                    .endpoint_registry
                    .update_status(endpoint.id, EndpointStatus::Error, None, Some(&error_msg))
                    .await;
            } else {
                let _ = state
                    .endpoint_registry
                    .update_status(endpoint.id, EndpointStatus::Online, Some(latency_ms), None)
                    .await;
            }
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();

            // ストリーミングレスポンスの場合
            if content_type.contains("text/event-stream") {
                let stream = response.bytes_stream();
                let body = axum::body::Body::from_stream(stream);
                return axum::response::Response::builder()
                    .status(status_code)
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .body(body)
                    .unwrap()
                    .into_response();
            }

            // 通常のJSONレスポンス
            match response.bytes().await {
                Ok(bytes) => axum::response::Response::builder()
                    .status(status_code)
                    .header("Content-Type", content_type)
                    .body(axum::body::Body::from(bytes))
                    .unwrap()
                    .into_response(),
                Err(e) => AppError(LbError::Http(format!("Failed to read response: {}", e)))
                    .into_response(),
            }
        }
        Err(e) => {
            let ollama_loading_model = if e.is_timeout()
                && endpoint.endpoint_type == crate::types::endpoint::EndpointType::Ollama
            {
                match request_model.as_deref() {
                    Some(model) => {
                        match probe_ollama_model_loaded(
                            &state.http_client,
                            &endpoint.base_url,
                            endpoint.api_key.as_deref(),
                            model,
                        )
                        .await
                        {
                            Some(false) => Some(model.to_string()),
                            _ => None,
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
            let classified_error = classify_upstream_request_error(
                &e,
                &endpoint.base_url,
                endpoint.inference_timeout_secs,
                ollama_loading_model.as_deref(),
            );
            let _ = state
                .endpoint_registry
                .update_status(
                    endpoint.id,
                    EndpointStatus::Error,
                    None,
                    Some(&classified_error.record_message),
                )
                .await;
            openai_error_response_with_type(
                classified_error.client_message,
                classified_error.error_type,
                classified_error.status_code,
            )
            .into_response()
        }
    }
}
