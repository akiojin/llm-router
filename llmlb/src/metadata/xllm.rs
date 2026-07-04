//! xLLM Model Metadata Retrieval
//!
//! SPEC-e8e9326e: Fetch model metadata from xLLM endpoints via GET /api/models/:model/info

use super::{MetadataError, ModelMetadata};
use crate::common::http::RequestBuilderBearerExt;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

/// xLLM model info response structure
#[derive(Debug, Deserialize)]
struct XllmModelInfoResponse {
    /// Model identifier
    model: String,

    /// Context window size
    #[serde(alias = "context_length", alias = "n_ctx")]
    context_length: Option<u32>,

    /// Model file size in bytes
    #[serde(alias = "size", alias = "file_size")]
    size_bytes: Option<u64>,

    /// Quantization type
    #[serde(alias = "quant", alias = "quantization_type")]
    quantization: Option<String>,

    /// Model family
    family: Option<String>,

    /// Parameter count string
    #[serde(alias = "params", alias = "num_params")]
    parameter_size: Option<String>,
}

/// Fetch model metadata from xLLM endpoint
///
/// Sends GET /api/models/:model/info to the xLLM endpoint
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - xLLM endpoint base URL
/// * `api_key` - Optional API key for authentication
/// * `model` - Model name to query
///
/// # Returns
/// Model metadata or error
pub async fn get_xllm_model_metadata(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<ModelMetadata, MetadataError> {
    // Simple URL encoding for model names (replace spaces and special chars)
    let encoded_model = model
        .replace(' ', "%20")
        .replace('/', "%2F")
        .replace(':', "%3A");

    let url = format!(
        "{}/api/models/{}/info",
        base_url.trim_end_matches('/'),
        encoded_model
    );

    let mut req_builder = client.get(&url).timeout(Duration::from_secs(10));

    req_builder = req_builder.bearer_opt(api_key);

    let response = req_builder.send().await?;
    let status = response.status();

    if !status.is_success() {
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(MetadataError::EndpointError {
            status: status.as_u16(),
            message,
        });
    }

    let info: XllmModelInfoResponse = response.json().await.map_err(|e| {
        MetadataError::InvalidResponse(format!("Failed to parse xLLM model info: {}", e))
    })?;

    Ok(ModelMetadata {
        model: info.model,
        context_length: info.context_length,
        size_bytes: info.size_bytes,
        quantization: info.quantization,
        family: info.family,
        parameter_size: info.parameter_size,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xllm_model_info_deserialization() {
        let json = r#"{
            "model": "llama-3.2-1b-instruct-q4_k_m",
            "context_length": 8192,
            "size_bytes": 1500000000,
            "quantization": "Q4_K_M",
            "family": "llama",
            "parameter_size": "1B"
        }"#;

        let info: XllmModelInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.model, "llama-3.2-1b-instruct-q4_k_m");
        assert_eq!(info.context_length, Some(8192));
        assert_eq!(info.size_bytes, Some(1500000000));
        assert_eq!(info.quantization, Some("Q4_K_M".to_string()));
    }

    #[test]
    fn test_xllm_model_info_with_aliases() {
        // Test with alternative field names (n_ctx instead of context_length)
        let json = r#"{
            "model": "mistral-7b",
            "n_ctx": 4096,
            "file_size": 5000000000,
            "quant": "Q8_0"
        }"#;

        let info: XllmModelInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.model, "mistral-7b");
        assert_eq!(info.context_length, Some(4096));
        assert_eq!(info.size_bytes, Some(5000000000));
        assert_eq!(info.quantization, Some("Q8_0".to_string()));
    }

    #[test]
    fn test_xllm_model_info_minimal() {
        let json = r#"{
            "model": "tiny-model"
        }"#;

        let info: XllmModelInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.model, "tiny-model");
        assert!(info.context_length.is_none());
        assert!(info.size_bytes.is_none());
    }
}
