//! /v1/responses のリクエストフィールド抽出と HTTP レスポンス構築ヘルパー
//!
//! arch-review [H6] round2: api/responses.rs からステートレスなヘルパーを分離。

use crate::api::error::AppError;
use crate::common::error::LbError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

pub(crate) fn openai_error_response(message: impl Into<String>, status: StatusCode) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "invalid_request_error",
            "code": status.as_u16(),
        }
    });

    (status, Json(payload)).into_response()
}

pub(crate) fn model_unavailable_response(message: impl Into<String>) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "service_unavailable",
            "code": "no_capable_nodes",
        }
    });

    (StatusCode::SERVICE_UNAVAILABLE, Json(payload)).into_response()
}

/// リクエストからモデル名を抽出
pub(crate) fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload["model"].as_str().map(String::from).ok_or_else(|| {
        AppError::from(LbError::Common(
            crate::common::error::CommonError::Validation("Missing required field: model".into()),
        ))
    })
}

/// リクエストからstreamフラグを抽出
pub(crate) fn extract_stream(payload: &Value) -> bool {
    payload["stream"].as_bool().unwrap_or(false)
}
