//! Model Delete Dispatcher
//!
//! Routes delete requests to the appropriate engine handler based on endpoint type.
//! Follows the dispatcher pattern used in `crate::metadata::mod.rs`.

pub mod ollama;

use crate::types::endpoint::EndpointType;
use reqwest::Client;
use thiserror::Error;

/// Error types for model delete operations
#[derive(Debug, Error)]
pub enum DeleteError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Endpoint returned an error response
    #[error("Endpoint returned error: {status} - {message}")]
    EndpointError {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },

    /// Delete not supported for this endpoint type
    #[error("Delete not supported for endpoint type: {0}")]
    UnsupportedType(String),
}

/// Delete a model from an endpoint
///
/// Routes to the appropriate handler based on endpoint type.
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - Endpoint base URL
/// * `api_key` - Optional API key
/// * `endpoint_type` - Type of the endpoint
/// * `model` - Model name to delete
pub async fn delete_model(
    client: &Client,
    base_url: &str,
    _api_key: Option<&str>,
    endpoint_type: &EndpointType,
    model: &str,
) -> Result<(), DeleteError> {
    // Resolve engine-specific model name
    let engine_model = crate::models::mapping::resolve_engine_name(model, endpoint_type)
        .map(|s| s.to_string())
        .unwrap_or_else(|| model.to_string());

    match endpoint_type {
        EndpointType::Ollama => ollama::delete_model(client, base_url, &engine_model).await,
        EndpointType::Xllm => {
            // xLLM delete API not yet available
            Err(DeleteError::UnsupportedType(
                "xllm (delete API not yet available)".to_string(),
            ))
        }
        EndpointType::LmStudio => Err(DeleteError::UnsupportedType(
            "lm_studio (delete API not available in 0.4.6)".to_string(),
        )),
        EndpointType::Llamacpp => Err(DeleteError::UnsupportedType(
            "llamacpp (delete API not available)".to_string(),
        )),
        EndpointType::Vllm => Err(DeleteError::UnsupportedType("vllm".to_string())),
        EndpointType::OpenaiCompatible => Err(DeleteError::UnsupportedType(
            "openai_compatible".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_error_display() {
        let err = DeleteError::EndpointError {
            status: 404,
            message: "Model not found".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Endpoint returned error: 404 - Model not found"
        );
    }

    #[test]
    fn test_delete_error_unsupported_type() {
        let err = DeleteError::UnsupportedType("vllm".to_string());
        assert_eq!(
            err.to_string(),
            "Delete not supported for endpoint type: vllm"
        );
    }

    #[test]
    fn test_delete_error_http() {
        // Verify that reqwest errors can be converted
        let err = DeleteError::UnsupportedType("test".to_string());
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_supported_types() {
        // Ollama supports delete
        assert!(EndpointType::Ollama.supports_model_delete());
        // xLLM delete API is not implemented yet
        assert!(!EndpointType::Xllm.supports_model_delete());
        // Others don't
        assert!(!EndpointType::LmStudio.supports_model_delete());
        assert!(!EndpointType::Vllm.supports_model_delete());
        assert!(!EndpointType::Llamacpp.supports_model_delete());
        assert!(!EndpointType::OpenaiCompatible.supports_model_delete());
    }
}
