//! エラー型定義
//!
//! 統一エラー型（thiserror使用）

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

/// Router error type
#[derive(Debug, Error)]
pub enum RouterError {
    /// Common layer error
    #[error(transparent)]
    Common(#[from] CommonError),

    /// Node not found
    #[error("Node not found: {0}")]
    AgentNotFound(Uuid),

    /// No available nodes
    #[error("No available nodes")]
    NoAgentsAvailable,

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

    /// Node is offline
    #[error("Node {0} is offline")]
    AgentOffline(Uuid),

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
}

/// Node error type
#[derive(Debug, Error)]
pub enum NodeError {
    /// Common layer error
    #[error(transparent)]
    Common(#[from] CommonError),

    /// Coordinator connection error
    #[error("Failed to connect to Coordinator: {0}")]
    CoordinatorConnection(String),

    /// LLM runtime connection error
    #[error("Failed to connect to LLM runtime: {0}")]
    RuntimeConnection(String),

    /// Registration error
    #[error("Node registration failed: {0}")]
    Registration(String),

    /// Health check send error
    #[error("Failed to send health check: {0}")]
    Heartbeat(String),

    /// Metrics collection error
    #[error("Failed to collect metrics: {0}")]
    Metrics(String),

    /// GUI error
    #[error("GUI error: {0}")]
    Gui(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias (Common)
pub type CommonResult<T> = Result<T, CommonError>;

/// Result type alias (Router)
pub type RouterResult<T> = Result<T, RouterError>;

/// Result type alias (Node)
pub type NodeResult<T> = Result<T, NodeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_error_display() {
        let error = CommonError::Config("test config error".to_string());
        assert_eq!(error.to_string(), "Configuration error: test config error");
    }

    #[test]
    fn test_coordinator_error_agent_not_found() {
        let node_id = Uuid::new_v4();
        let error = RouterError::AgentNotFound(node_id);
        assert!(error.to_string().contains(&node_id.to_string()));
    }

    #[test]
    fn test_coordinator_error_no_agents() {
        let error = RouterError::NoAgentsAvailable;
        assert_eq!(error.to_string(), "No available nodes");
    }

    #[test]
    fn test_agent_error_coordinator_connection() {
        let error = NodeError::CoordinatorConnection("timeout".to_string());
        assert_eq!(
            error.to_string(),
            "Failed to connect to Coordinator: timeout"
        );
    }

    #[test]
    fn test_error_from_conversion() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let common_error: CommonError = json_error.into();
        assert!(matches!(common_error, CommonError::Serialization(_)));
    }
}
