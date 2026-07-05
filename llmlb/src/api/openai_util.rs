//! OpenAI互換APIのユーティリティ関数
//!
//! ペイロードのサニタイズ、メッセージ変換、エラーレスポンス生成など。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
#[cfg(test)]
use serde_json::Value;

mod ollama_probe;
pub use ollama_probe::probe_ollama_model_loaded;
mod upstream_error;
pub use upstream_error::*;
mod messages;
pub use messages::{map_openai_messages_to_anthropic, map_openai_messages_to_google_contents};
mod sanitize;
pub use sanitize::sanitize_openai_payload_for_history;

/// OpenAI互換のエラーレスポンスを生成
pub fn openai_error_response_with_type(
    message: impl Into<String>,
    error_type: impl Into<String>,
    status: StatusCode,
) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": error_type.into(),
            "code": status.as_u16(),
        }
    });

    (status, Json(payload)).into_response()
}

/// Default OpenAI-compatible error response using `invalid_request_error`.
pub fn openai_error_response(message: impl Into<String>, status: StatusCode) -> Response {
    openai_error_response_with_type(message, "invalid_request_error", status)
}

/// モデル利用不可レスポンスを生成
pub fn model_unavailable_response(message: impl Into<String>, code: &str) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "service_unavailable",
            "code": code,
        }
    });

    (StatusCode::SERVICE_UNAVAILABLE, Json(payload)).into_response()
}

#[cfg(test)]
mod tests;
