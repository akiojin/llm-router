//! LM Studio Model Metadata Retrieval
//!
//! SPEC-e8e9326e: Fetch model metadata from LM Studio endpoints via GET /api/v1/models/:model

use super::{MetadataError, ModelMetadata};
use crate::common::http::RequestBuilderBearerExt;
use reqwest::Client;
use std::time::Duration;

/// Fetch model metadata from LM Studio endpoint
///
/// Sends GET /api/v1/models/:model to the LM Studio endpoint
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - LM Studio endpoint base URL
/// * `api_key` - Optional API key for authentication
/// * `model` - Model name to query
///
/// # Returns
/// Model metadata or error
pub async fn get_lm_studio_model_metadata(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<ModelMetadata, MetadataError> {
    let encoded_model = model
        .replace(' ', "%20")
        .replace('/', "%2F")
        .replace(':', "%3A");

    let url = format!(
        "{}/api/v1/models/{}",
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

    let json: serde_json::Value = response.json().await.map_err(|e| {
        MetadataError::InvalidResponse(format!("Failed to parse LM Studio model info: {}", e))
    })?;

    Ok(parse_lm_studio_response(&json, model))
}

/// Parse LM Studio model info response into ModelMetadata
fn parse_lm_studio_response(json: &serde_json::Value, model: &str) -> ModelMetadata {
    let model_name = json["id"].as_str().unwrap_or(model).to_string();

    let context_length = json["max_context_length"].as_u64().map(|v| v as u32);

    let family = json["arch"].as_str().map(|s| s.to_string());

    // quantization can be a string or an object with "name" and "bits_per_weight"
    let (quantization, quantization_bits) = if let Some(q_str) = json["quantization"].as_str() {
        (Some(q_str.to_string()), None)
    } else if json["quantization"].is_object() {
        let name = json["quantization"]["name"].as_str().map(|s| s.to_string());
        let bits = json["quantization"]["bits_per_weight"]
            .as_f64()
            .map(|v| v as f32);
        (name, bits)
    } else {
        (None, None)
    };

    let format = json["compatibility_type"].as_str().map(|s| s.to_string());

    let parameter_size = json["params_string"].as_str().map(|s| s.to_string());

    let size_bytes = json["size_bytes"].as_u64();

    let supports_vision = json["capabilities"]["vision"].as_bool();

    let supports_tool_use = json["capabilities"]["trained_for_tool_use"].as_bool();

    // LM StudioのモデルIDは "publisher/model-name" 形式のため、HFリポ名として推測。
    // 判定ロジックは models::mapping::guess_hf_repo に集約済み。
    let hf_repo = crate::models::mapping::guess_hf_repo(
        model,
        &crate::types::endpoint::EndpointType::LmStudio,
    );

    ModelMetadata {
        model: model_name,
        context_length,
        size_bytes,
        quantization,
        family,
        parameter_size,
        format,
        supports_vision,
        supports_tool_use,
        quantization_bits,
        hf_repo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lm_studio_response_full() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "id": "llama-3.2-1b-instruct",
            "max_context_length": 8192,
            "arch": "llama",
            "quantization": "Q4_K_M",
            "compatibility_type": "gguf",
            "params_string": "1B",
            "size_bytes": 1500000000
        }"#,
        )
        .unwrap();

        let metadata = parse_lm_studio_response(&json, "fallback-model");
        assert_eq!(metadata.model, "llama-3.2-1b-instruct");
        assert_eq!(metadata.context_length, Some(8192));
        assert_eq!(metadata.family, Some("llama".to_string()));
        assert_eq!(metadata.quantization, Some("Q4_K_M".to_string()));
        assert_eq!(metadata.format, Some("gguf".to_string()));
        assert_eq!(metadata.parameter_size, Some("1B".to_string()));
        assert_eq!(metadata.size_bytes, Some(1500000000));
        assert!(metadata.supports_vision.is_none());
        assert!(metadata.supports_tool_use.is_none());
        assert!(metadata.quantization_bits.is_none());
    }

    #[test]
    fn test_parse_lm_studio_response_with_capabilities_and_quantization_object() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "id": "llava-1.5-7b",
            "max_context_length": 4096,
            "arch": "llava",
            "quantization": {
                "name": "Q4_K_M",
                "bits_per_weight": 4.5
            },
            "compatibility_type": "gguf",
            "params_string": "7B",
            "size_bytes": 5000000000,
            "capabilities": {
                "vision": true,
                "trained_for_tool_use": false
            }
        }"#,
        )
        .unwrap();

        let metadata = parse_lm_studio_response(&json, "fallback");
        assert_eq!(metadata.model, "llava-1.5-7b");
        assert_eq!(metadata.context_length, Some(4096));
        assert_eq!(metadata.family, Some("llava".to_string()));
        assert_eq!(metadata.quantization, Some("Q4_K_M".to_string()));
        assert_eq!(metadata.quantization_bits, Some(4.5));
        assert_eq!(metadata.format, Some("gguf".to_string()));
        assert_eq!(metadata.parameter_size, Some("7B".to_string()));
        assert_eq!(metadata.size_bytes, Some(5000000000));
        assert_eq!(metadata.supports_vision, Some(true));
        assert_eq!(metadata.supports_tool_use, Some(false));
    }

    #[test]
    fn test_parse_lm_studio_response_minimal() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
            "id": "tiny-model"
        }"#,
        )
        .unwrap();

        let metadata = parse_lm_studio_response(&json, "fallback");
        assert_eq!(metadata.model, "tiny-model");
        assert!(metadata.context_length.is_none());
        assert!(metadata.size_bytes.is_none());
        assert!(metadata.quantization.is_none());
        assert!(metadata.family.is_none());
        assert!(metadata.parameter_size.is_none());
        assert!(metadata.format.is_none());
        assert!(metadata.supports_vision.is_none());
        assert!(metadata.supports_tool_use.is_none());
        assert!(metadata.quantization_bits.is_none());
    }
}
