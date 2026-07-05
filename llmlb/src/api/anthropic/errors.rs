//! Anthropic エラーレスポンス構築と LbError/上流 HTTP ステータスの分類
//!
//! arch-review [H6]: api/anthropic.rs からエラー整形ロジックを分離。

use crate::common::error::{CommonError, LbError};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub(super) fn anthropic_error_response(
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

pub(super) fn anthropic_error_from_lb_error(err: &LbError) -> Response {
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

pub(super) fn anthropic_upstream_error_details(
    err: &LbError,
) -> (StatusCode, &'static str, String) {
    let message = lb_error_detail_message(err);
    match err {
        LbError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, "api_error", message),
        LbError::Http(_) => (StatusCode::BAD_GATEWAY, "api_error", message),
        _ => (
            err.status_code(),
            anthropic_error_type_for_lb_error(err),
            message,
        ),
    }
}

fn anthropic_error_type_for_lb_error(err: &LbError) -> &'static str {
    match err {
        LbError::Common(CommonError::Validation(_)) => "invalid_request_error",
        LbError::Authentication(_) => "authentication_error",
        LbError::Authorization(_) => "permission_error",
        LbError::NotFound(_) | LbError::InvalidModelName(_) | LbError::EndpointNotFound(_) => {
            "not_found_error"
        }
        _ => "api_error",
    }
}

fn lb_error_detail_message(err: &LbError) -> String {
    match err {
        LbError::Common(CommonError::Validation(message))
        | LbError::NotFound(message)
        | LbError::Database(message)
        | LbError::Http(message)
        | LbError::Timeout(message)
        | LbError::ServiceUnavailable(message)
        | LbError::Internal(message)
        | LbError::InvalidModelName(message)
        | LbError::InsufficientStorage(message)
        | LbError::PasswordHash(message)
        | LbError::Jwt(message)
        | LbError::Authentication(message)
        | LbError::Authorization(message)
        | LbError::Conflict(message)
        | LbError::NoCapableEndpoints(message) => message.clone(),
        LbError::EndpointNotFound(endpoint_id) => format!("Endpoint not found: {}", endpoint_id),
        LbError::EndpointOffline(endpoint_id) => format!("Endpoint {} is offline", endpoint_id),
        LbError::NoEndpointsAvailable => err.external_message().to_string(),
        LbError::Common(_) => err.external_message().to_string(),
    }
}

pub(super) fn map_upstream_status_to_anthropic_error(
    status: StatusCode,
) -> (StatusCode, &'static str) {
    match status.as_u16() {
        400 => (StatusCode::BAD_REQUEST, "invalid_request_error"),
        401 => (StatusCode::UNAUTHORIZED, "authentication_error"),
        403 => (StatusCode::FORBIDDEN, "permission_error"),
        404 => (StatusCode::NOT_FOUND, "not_found_error"),
        408 | 504 => (StatusCode::GATEWAY_TIMEOUT, "api_error"),
        429 => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error"),
        _ => (StatusCode::BAD_GATEWAY, "api_error"),
    }
}
