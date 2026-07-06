//! OpenAI互換APIエンドポイント (/v1/*)
//!
//! このモジュールはEndpointRegistry/Endpoint型を使用しています。

/// 未指定/仮想IPアドレス（クラウドプロバイダ等、実IPを持たない場合に使用）
const UNSPECIFIED_IP: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

mod catalog;
mod cloud;
mod proxy_post;
pub use catalog::{get_model, list_models};
#[cfg(test)]
use cloud::cloud_virtual_node;
use cloud::{parse_cloud_model, proxy_openai_cloud_post};
#[cfg(test)]
use proxy_post::add_queue_headers;
use proxy_post::proxy_openai_post;

use crate::common::{
    error::{CommonError, LbError},
    protocol::RequestType,
};
use crate::types::model::ModelCapability;
use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Response,
    Json,
};
use serde_json::Value;
use std::net::SocketAddr;

use crate::auth::middleware::ApiKeyAuthContext;

use crate::{
    api::{
        error::AppError,
        model_name::{parse_quantized_model_name, ParsedModelName},
        models::list_registered_models,
    },
    AppState,
};

/// POST /v1/chat/completions - OpenAI互換チャットAPI
pub async fn chat_completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) =
        crate::common::http::extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model(&payload)?;
    let parsed = if parse_cloud_model(&model).is_some() {
        ParsedModelName {
            raw: model.clone(),
            base: model.clone(),
            quantization: None,
        }
    } else {
        parse_quantized_model_name(&model).map_err(AppError::from)?
    };
    let requires_image_input = payload_requires_image_input(&payload);

    // モデルの TextGeneration capability を検証
    let models = list_registered_models(&state.db_pool).await?;
    if let Some(model_info) = models.iter().find(|m| m.name == model) {
        if !model_info.has_capability(ModelCapability::TextGeneration) {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("Model '{}' does not support text generation", parsed.raw),
            ))));
        }
        if requires_image_input && !model_info.has_capability(ModelCapability::ImageInput) {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("Model '{}' does not support image input", parsed.raw),
            ))));
        }
    }
    // 登録されていないモデルはエンドポイント側で処理（クラウドモデル等）

    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/chat/completions",
        parsed.raw,
        stream,
        RequestType::Chat,
        client_ip,
        api_key_id,
    )
    .await
}

/// POST /api/dashboard/playground/chat/completions - Dashboardセッション用チャットAPI
///
/// LB Playgroundからの試行リクエストを、APIキーではなくJWTセッションで受け付ける。
/// 外部クライアント向けの`/v1/chat/completions`は引き続きAPIキー必須。
pub async fn dashboard_playground_chat_completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    chat_completions(
        ConnectInfo(addr),
        headers,
        State(state),
        None,
        Json(payload),
    )
    .await
}

/// POST /v1/completions - OpenAI互換テキスト補完API
pub async fn completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) =
        crate::common::http::extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model(&payload)?;
    if parse_cloud_model(&model).is_none() {
        parse_quantized_model_name(&model).map_err(AppError::from)?;
    }
    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/completions",
        model,
        stream,
        RequestType::Generate,
        client_ip,
        api_key_id,
    )
    .await
}

/// POST /v1/embeddings - OpenAI互換Embeddings API
pub async fn embeddings(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) =
        crate::common::http::extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model_with_default(&payload, crate::config::get_default_embedding_model());
    if parse_cloud_model(&model).is_none() {
        parse_quantized_model_name(&model).map_err(AppError::from)?;
    }
    proxy_openai_post(
        &state,
        payload,
        "/v1/embeddings",
        model,
        false,
        RequestType::Embeddings,
        client_ip,
        api_key_id,
    )
    .await
}

fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| validation_error("`model` field is required for OpenAI-compatible requests"))
}

/// モデル名を抽出し、未指定または空の場合はデフォルト値を使用
fn extract_model_with_default(payload: &Value, default: String) -> String {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or(default)
}

fn extract_stream(payload: &Value) -> bool {
    payload
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn payload_requires_image_input(payload: &Value) -> bool {
    let Some(messages) = payload.get("messages").and_then(|v| v.as_array()) else {
        return false;
    };

    for message in messages {
        let Some(parts) = message.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str());
            if matches!(part_type, Some("image_url" | "input_image")) {
                return true;
            }
        }
    }

    false
}

fn validation_error(message: impl Into<String>) -> AppError {
    let err = LbError::Common(CommonError::Validation(message.into()));
    err.into()
}

#[cfg(test)]
mod tests;
