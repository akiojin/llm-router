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
mod tests {
    use super::*;

    #[test]
    fn test_common_error_display() {
        let error = CommonError::Config("test config error".to_string());
        assert_eq!(error.to_string(), "Configuration error: test config error");
    }

    #[test]
    fn test_lb_error_node_not_found() {
        let endpoint_id = Uuid::new_v4();
        let error = LbError::EndpointNotFound(endpoint_id);
        assert!(error.to_string().contains(&endpoint_id.to_string()));
    }

    #[test]
    fn test_lb_error_no_nodes() {
        let error = LbError::NoEndpointsAvailable;
        assert_eq!(error.to_string(), "No available endpoints");
    }

    #[test]
    fn test_error_from_conversion() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let common_error: CommonError = json_error.into();
        assert!(matches!(common_error, CommonError::Serialization(_)));
    }

    #[test]
    fn test_lb_error_type() {
        // Authentication errors
        assert_eq!(
            LbError::Authentication("test".to_string()).error_type(),
            "authentication_error"
        );
        assert_eq!(
            LbError::Jwt("test".to_string()).error_type(),
            "authentication_error"
        );

        // Authorization errors
        assert_eq!(
            LbError::Authorization("test".to_string()).error_type(),
            "permission_error"
        );

        // Not found errors
        assert_eq!(
            LbError::EndpointNotFound(Uuid::new_v4()).error_type(),
            "not_found_error"
        );
        assert_eq!(
            LbError::NotFound("test".to_string()).error_type(),
            "not_found_error"
        );
        assert_eq!(
            LbError::NoCapableEndpoints("test".to_string()).error_type(),
            "not_found_error"
        );

        // Service unavailable errors
        assert_eq!(
            LbError::NoEndpointsAvailable.error_type(),
            "service_unavailable"
        );
        assert_eq!(
            LbError::Http("test".to_string()).error_type(),
            "service_unavailable"
        );
        assert_eq!(
            LbError::EndpointOffline(Uuid::new_v4()).error_type(),
            "service_unavailable"
        );

        // Server errors
        assert_eq!(
            LbError::Database("test".to_string()).error_type(),
            "server_error"
        );
        assert_eq!(
            LbError::Internal("test".to_string()).error_type(),
            "server_error"
        );

        // Invalid request errors
        assert_eq!(
            LbError::InvalidModelName("test".to_string()).error_type(),
            "invalid_request_error"
        );

        // Conflict errors
        assert_eq!(
            LbError::Conflict("test".to_string()).error_type(),
            "invalid_request_error"
        );
    }

    #[test]
    fn test_lb_error_status_code() {
        assert_eq!(
            LbError::Authentication("test".to_string()).status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            LbError::Authorization("test".to_string()).status_code(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            LbError::EndpointNotFound(Uuid::new_v4()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            LbError::NotFound("test".to_string()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            LbError::NoEndpointsAvailable.status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            LbError::Http("test".to_string()).status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            LbError::Timeout("test".to_string()).status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            LbError::Internal("test".to_string()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            LbError::Conflict("test".to_string()).status_code(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn test_lb_error_to_openai_error() {
        let error = LbError::NoEndpointsAvailable;
        let response = error.to_openai_error();

        assert_eq!(response.error.message, "No available endpoints");
        assert_eq!(response.error.error_type, "service_unavailable");
        assert_eq!(response.error.code, Some("503".to_string()));
    }

    #[test]
    fn test_openai_error_response_serialization() {
        let response = OpenAIErrorResponse {
            error: OpenAIErrorDetail {
                message: "Test error".to_string(),
                error_type: "invalid_request_error".to_string(),
                code: Some("400".to_string()),
            },
        };

        let json = serde_json::to_string(&response).expect("Failed to serialize");
        assert!(json.contains("\"message\":\"Test error\""));
        assert!(json.contains("\"type\":\"invalid_request_error\""));
        assert!(json.contains("\"code\":\"400\""));
    }

    // --- 追加テスト ---

    #[test]
    fn test_common_error_validation_display() {
        let error = CommonError::Validation("name is required".to_string());
        assert_eq!(error.to_string(), "Validation error: name is required");
    }

    #[test]
    fn test_common_error_ip_addr_parse() {
        let ip_err: Result<std::net::IpAddr, _> = "not_an_ip".parse();
        let common_error: CommonError = ip_err.unwrap_err().into();
        assert!(matches!(common_error, CommonError::IpAddrParse(_)));
        assert!(common_error.to_string().contains("IP address parse error"));
    }

    #[test]
    fn test_common_error_uuid_parse() {
        let uuid_err: Result<Uuid, _> = "not-a-uuid".parse();
        let common_error: CommonError = uuid_err.unwrap_err().into();
        assert!(matches!(common_error, CommonError::UuidParse(_)));
        assert!(common_error.to_string().contains("UUID parse error"));
    }

    #[test]
    fn test_lb_error_external_message_all_variants() {
        assert_eq!(
            LbError::InvalidModelName("x".into()).external_message(),
            "Invalid model name"
        );
        assert_eq!(
            LbError::InsufficientStorage("x".into()).external_message(),
            "Insufficient storage"
        );
        assert_eq!(
            LbError::Authentication("x".into()).external_message(),
            "Authentication failed"
        );
        assert_eq!(
            LbError::ServiceUnavailable("x".into()).external_message(),
            "Service temporarily unavailable"
        );
    }

    #[test]
    fn test_lb_error_status_code_remaining_variants() {
        assert_eq!(
            LbError::ServiceUnavailable("x".into()).status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            LbError::EndpointOffline(Uuid::new_v4()).status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            LbError::PasswordHash("x".into()).status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            LbError::Database("x".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            LbError::InvalidModelName("x".into()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            LbError::InsufficientStorage("x".into()).status_code(),
            StatusCode::INSUFFICIENT_STORAGE
        );
        assert_eq!(
            LbError::NoCapableEndpoints("x".into()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            LbError::Common(CommonError::Validation("x".into())).status_code(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn test_lb_error_from_common_error() {
        let common = CommonError::Config("bad config".to_string());
        let lb: LbError = common.into();
        assert!(matches!(lb, LbError::Common(CommonError::Config(_))));
    }

    #[test]
    fn test_openai_error_response_code_none_skipped() {
        let response = OpenAIErrorResponse {
            error: OpenAIErrorDetail {
                message: "Test".to_string(),
                error_type: "server_error".to_string(),
                code: None,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("code"));
    }

    #[test]
    fn test_lb_error_error_type_password_hash() {
        assert_eq!(
            LbError::PasswordHash("err".into()).error_type(),
            "authentication_error"
        );
    }

    #[test]
    fn test_lb_error_error_type_timeout() {
        assert_eq!(LbError::Timeout("err".into()).error_type(), "server_error");
    }

    #[test]
    fn test_lb_error_error_type_insufficient_storage() {
        assert_eq!(
            LbError::InsufficientStorage("err".into()).error_type(),
            "server_error"
        );
    }

    #[test]
    fn test_lb_error_error_type_service_unavailable() {
        assert_eq!(
            LbError::ServiceUnavailable("err".into()).error_type(),
            "service_unavailable"
        );
    }

    #[test]
    fn test_lb_error_external_message_does_not_leak() {
        let err = LbError::Database("SELECT * FROM secrets WHERE id=1".into());
        let msg = err.external_message();
        assert!(!msg.contains("SELECT"));
        assert!(!msg.contains("secrets"));
    }

    #[test]
    fn test_lb_error_to_openai_error_all_fields() {
        let err = LbError::Authentication("bad token".into());
        let resp = err.to_openai_error();
        assert_eq!(resp.error.message, "Authentication failed");
        assert_eq!(resp.error.error_type, "authentication_error");
        assert_eq!(resp.error.code, Some("401".to_string()));
    }

    #[test]
    fn test_lb_error_external_message_common() {
        let err = LbError::Common(CommonError::Config("test".into()));
        assert_eq!(err.external_message(), "Request error");
    }

    #[test]
    fn test_lb_error_external_message_password_hash() {
        assert_eq!(
            LbError::PasswordHash("x".into()).external_message(),
            "Authentication error"
        );
    }

    #[test]
    fn test_lb_error_external_message_jwt() {
        assert_eq!(
            LbError::Jwt("x".into()).external_message(),
            "Authentication error"
        );
    }

    #[test]
    fn test_lb_error_external_message_endpoint_offline() {
        assert_eq!(
            LbError::EndpointOffline(Uuid::new_v4()).external_message(),
            "Endpoint offline"
        );
    }

    #[test]
    fn test_lb_error_external_message_http() {
        assert_eq!(
            LbError::Http("conn refused".into()).external_message(),
            "Backend service unavailable"
        );
    }

    #[test]
    fn test_lb_error_external_message_timeout() {
        assert_eq!(
            LbError::Timeout("30s".into()).external_message(),
            "Request timeout"
        );
    }

    #[test]
    fn test_lb_error_external_message_database() {
        assert_eq!(
            LbError::Database("connection pool".into()).external_message(),
            "Database error"
        );
    }

    #[test]
    fn test_lb_error_external_message_internal() {
        assert_eq!(
            LbError::Internal("panic".into()).external_message(),
            "Internal server error"
        );
    }

    #[test]
    fn test_lb_error_external_message_no_capable() {
        assert_eq!(
            LbError::NoCapableEndpoints("gpt-4".into()).external_message(),
            "No capable endpoints"
        );
    }

    #[test]
    fn test_lb_error_external_message_conflict() {
        assert_eq!(
            LbError::Conflict("duplicate".into()).external_message(),
            "Resource conflict"
        );
    }

    #[test]
    fn test_lb_error_jwt_status() {
        assert_eq!(
            LbError::Jwt("expired".into()).status_code(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn test_common_error_serialization_display() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{bad").unwrap_err();
        let common: CommonError = json_err.into();
        assert!(common.to_string().starts_with("Serialization error:"));
    }

    #[test]
    fn test_lb_error_debug_format() {
        let err = LbError::NotFound("model xyz".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("NotFound"));
    }

    #[test]
    fn test_openai_error_detail_debug() {
        let detail = OpenAIErrorDetail {
            message: "m".into(),
            error_type: "t".into(),
            code: None,
        };
        let debug = format!("{:?}", detail);
        assert!(debug.contains("OpenAIErrorDetail"));
    }
}
