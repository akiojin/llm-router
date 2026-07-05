//! エラー型定義
//!
//! 統一エラー型（thiserror使用）
//!
//! # OpenAI互換エラーレスポンス
//!
//! `LbError`は`error_type()`と`status_code()`メソッドを提供し、
//! OpenAI互換のエラーレスポンスを生成できます。

use axum::http::StatusCode;
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

/// Common layer error type
#[derive(Debug, Error)]
pub enum CommonError {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// UUID parse error
    #[error("UUID parse error: {0}")]
    UuidParse(#[from] uuid::Error),

    /// IP address parse error
    #[error("IP address parse error: {0}")]
    IpAddrParse(#[from] std::net::AddrParseError),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),
}

/// load balancer error type
#[derive(Debug, Error)]
pub enum LbError {
    /// Common layer error
    #[error(transparent)]
    Common(#[from] CommonError),

    /// Endpoint not found
    #[error("Endpoint not found: {0}")]
    EndpointNotFound(Uuid),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// No available endpoints
    #[error("No available endpoints")]
    NoEndpointsAvailable,

    /// No capable endpoints for model
    #[error("No capable endpoints for model: {0}")]
    NoCapableEndpoints(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// HTTP client error
    #[error("HTTP client error: {0}")]
    Http(String),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Service unavailable (e.g., during initialization)
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Endpoint is offline
    #[error("Endpoint {0} is offline")]
    EndpointOffline(Uuid),

    /// Invalid model name
    #[error("Invalid model name: {0}")]
    InvalidModelName(String),

    /// Insufficient storage
    #[error("Insufficient storage: {0}")]
    InsufficientStorage(String),

    /// Password hash error
    #[error("Password hash error: {0}")]
    PasswordHash(String),

    /// JWT error
    #[error("JWT error: {0}")]
    Jwt(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Authorization error
    #[error("Authorization error: {0}")]
    Authorization(String),

    /// Conflict error (e.g., duplicate resource)
    #[error("Conflict: {0}")]
    Conflict(String),
}

impl LbError {
    /// Returns a safe error message for external clients.
    ///
    /// This method returns a generic error message that does not expose
    /// internal implementation details such as IP addresses, port numbers,
    /// or internal service names. Use this for HTTP responses to external clients.
    ///
    /// For debugging purposes, use the `Display` implementation (`to_string()`)
    /// which includes full error details - but only in server logs.
    pub fn external_message(&self) -> &'static str {
        match self {
            Self::Common(_) => "Request error",
            Self::EndpointNotFound(_) => "Endpoint not found",
            Self::NotFound(_) => "Not found",
            Self::NoEndpointsAvailable => "No available endpoints",
            Self::NoCapableEndpoints(_) => "No capable endpoints",
            Self::Database(_) => "Database error",
            Self::Http(_) => "Backend service unavailable",
            Self::Timeout(_) => "Request timeout",
            Self::ServiceUnavailable(_) => "Service temporarily unavailable",
            Self::Internal(_) => "Internal server error",
            Self::EndpointOffline(_) => "Endpoint offline",
            Self::InvalidModelName(_) => "Invalid model name",
            Self::InsufficientStorage(_) => "Insufficient storage",
            Self::PasswordHash(_) => "Authentication error",
            Self::Jwt(_) => "Authentication error",
            Self::Authentication(_) => "Authentication failed",
            Self::Authorization(_) => "Access denied",
            Self::Conflict(_) => "Resource conflict",
        }
    }

    /// Returns the OpenAI-compatible error type string.
    ///
    /// # Error Types
    ///
    /// - `invalid_request_error`: Bad request parameters
    /// - `authentication_error`: Auth failures
    /// - `permission_error`: Authorization failures
    /// - `not_found_error`: Resource not found
    /// - `rate_limit_error`: Too many requests
    /// - `server_error`: Internal server errors
    /// - `service_unavailable`: Backend unavailable
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::Common(CommonError::Validation(_)) => "invalid_request_error",
            Self::Common(_) => "invalid_request_error",
            Self::EndpointNotFound(_) => "not_found_error",
            Self::NotFound(_) => "not_found_error",
            Self::NoEndpointsAvailable => "service_unavailable",
            Self::NoCapableEndpoints(_) => "not_found_error",
            Self::Database(_) => "server_error",
            Self::Http(_) => "service_unavailable",
            Self::Timeout(_) => "server_error",
            Self::ServiceUnavailable(_) => "service_unavailable",
            Self::Internal(_) => "server_error",
            Self::EndpointOffline(_) => "service_unavailable",
            Self::InvalidModelName(_) => "invalid_request_error",
            Self::InsufficientStorage(_) => "server_error",
            Self::PasswordHash(_) => "authentication_error",
            Self::Jwt(_) => "authentication_error",
            Self::Authentication(_) => "authentication_error",
            Self::Authorization(_) => "permission_error",
            Self::Conflict(_) => "invalid_request_error",
        }
    }

    /// Returns the HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Common(CommonError::Validation(_)) => StatusCode::BAD_REQUEST,
            Self::Common(_) => StatusCode::BAD_REQUEST,
            Self::EndpointNotFound(_) => StatusCode::NOT_FOUND,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::NoEndpointsAvailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::NoCapableEndpoints(_) => StatusCode::NOT_FOUND,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Http(_) => StatusCode::BAD_GATEWAY,
            Self::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::EndpointOffline(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidModelName(_) => StatusCode::BAD_REQUEST,
            Self::InsufficientStorage(_) => StatusCode::INSUFFICIENT_STORAGE,
            Self::PasswordHash(_) => StatusCode::UNAUTHORIZED,
            Self::Jwt(_) => StatusCode::UNAUTHORIZED,
            Self::Authentication(_) => StatusCode::UNAUTHORIZED,
            Self::Authorization(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
        }
    }

    /// Converts this error to an OpenAI-compatible error response.
    pub fn to_openai_error(&self) -> OpenAIErrorResponse {
        OpenAIErrorResponse {
            error: OpenAIErrorDetail {
                message: self.external_message().to_string(),
                error_type: self.error_type().to_string(),
                code: Some(self.status_code().as_u16().to_string()),
            },
        }
    }
}

/// OpenAI互換エラーレスポンス
///
/// # Example
///
/// ```json
/// {
///   "error": {
///     "message": "No available endpoints",
///     "type": "service_unavailable",
///     "code": "503"
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIErrorResponse {
    /// The error details
    pub error: OpenAIErrorDetail,
}

/// OpenAIエラー詳細
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIErrorDetail {
    /// Human-readable error message
    pub message: String,
    /// Error type (e.g., "invalid_request_error", "server_error")
    #[serde(rename = "type")]
    pub error_type: String,
    /// Error code (optional, typically HTTP status as string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Result type alias (Common)
pub type CommonResult<T> = Result<T, CommonError>;

/// Result type alias (load balancer)
pub type RouterResult<T> = Result<T, LbError>;

#[cfg(test)]
mod tests;
