//! Ollama Model Metadata Retrieval
//!
//! SPEC-e8e9326e: Fetch model metadata from Ollama endpoints via POST /api/show

use super::{MetadataError, ModelMetadata};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Ollama show request
#[derive(Debug, Serialize)]
struct OllamaShowRequest {
    model: String,
}

/// Ollama show response
#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    /// Model name
    #[serde(alias = "name")]
    model: Option<String>,

    /// Model details
    details: Option<OllamaModelDetails>,

    /// Model parameters.
    /// Newer Ollama can return this as a plain text block.
    #[serde(default)]
    parameters: Option<Value>,

    /// Additional model info map.
    /// Some servers expose context length here with keys like `*.context_length`.
    #[serde(default)]
    model_info: Option<Value>,
}

/// Ollama model details
#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    /// Model family
    family: Option<String>,

    /// Parameter size (e.g., "7B")
    parameter_size: Option<String>,

    /// Quantization level
    quantization_level: Option<String>,
}

fn parse_positive_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Number(n) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0),
        Value::String(s) => s.trim().parse::<u32>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn is_context_length_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "num_ctx"
        || key == "context_length"
        || key == "max_context_length"
        || key == "n_ctx"
        || key.ends_with(".context_length")
        || key.ends_with(".max_context_length")
        || key.ends_with(".n_ctx")
        || key.ends_with("_context_length")
}

fn extract_first_u32(text: &str) -> Option<u32> {
    let mut digits = String::new();
    let mut started = false;

    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started {
            break;
        }
    }

    if digits.is_empty() {
        return None;
    }

    digits.parse::<u32>().ok().filter(|v| *v > 0)
}

fn extract_context_from_parameters_text(text: &str) -> Option<u32> {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        for prefix in ["num_ctx", "context_length", "max_context_length", "n_ctx"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                if let Some(parsed) = extract_first_u32(rest) {
                    return Some(parsed);
                }
            }
        }
    }

    None
}

fn extract_context_from_model_info(model_info: &Value) -> Option<u32> {
    let obj = model_info.as_object()?;
    for (key, value) in obj {
        if is_context_length_key(key) {
            if let Some(ctx) = parse_positive_u32(value) {
                return Some(ctx);
            }
        }
    }
    None
}

fn extract_context_from_parameters(parameters: &Value) -> Option<u32> {
    match parameters {
        Value::Object(obj) => {
            for (key, value) in obj {
                if is_context_length_key(key) {
                    if let Some(ctx) = parse_positive_u32(value) {
                        return Some(ctx);
                    }
                }
            }
            None
        }
        Value::String(text) => extract_context_from_parameters_text(text),
        _ => None,
    }
}

/// model_infoからHuggingFaceリポジトリ情報を抽出
///
/// Ollamaのmodel_infoには `general.base_model.repo_url` や類似のキーで
/// HFリポジトリURLが含まれることがある。
fn extract_hf_repo(info: &OllamaShowResponse) -> Option<String> {
    // model_infoからHF関連キーを探す
    if let Some(model_info) = &info.model_info {
        if let Some(obj) = model_info.as_object() {
            for (key, value) in obj {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.contains("repo_url")
                    || key_lower.contains("base_model.repo")
                    || key_lower == "general.base_model.repo_url"
                {
                    if let Some(url) = value.as_str() {
                        let repo = crate::registry::models::extract_repo_id(url);
                        if repo.contains('/') {
                            return Some(repo);
                        }
                    }
                }
            }
            // general.name + general.author パターン
            let author = obj
                .iter()
                .find(|(k, _)| {
                    let kl = k.to_ascii_lowercase();
                    kl == "general.author" || kl.ends_with(".author")
                })
                .and_then(|(_, v)| v.as_str());
            let name = obj
                .iter()
                .find(|(k, _)| {
                    let kl = k.to_ascii_lowercase();
                    kl == "general.name" || kl == "general.basename"
                })
                .and_then(|(_, v)| v.as_str());
            if let (Some(author), Some(name)) = (author, name) {
                if !author.is_empty() && !name.is_empty() {
                    return Some(format!("{}/{}", author, name));
                }
            }
        }
    }
    None
}

fn extract_context_length(info: &OllamaShowResponse) -> Option<u32> {
    if let Some(model_info) = &info.model_info {
        if let Some(ctx) = extract_context_from_model_info(model_info) {
            return Some(ctx);
        }
    }

    if let Some(parameters) = &info.parameters {
        if let Some(ctx) = extract_context_from_parameters(parameters) {
            return Some(ctx);
        }
    }

    None
}

/// Fetch model metadata from Ollama endpoint
///
/// Sends POST /api/show to the Ollama endpoint
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - Ollama endpoint base URL
/// * `model` - Model name to query
///
/// # Returns
/// Model metadata or error
pub async fn get_ollama_model_metadata(
    client: &Client,
    base_url: &str,
    model: &str,
) -> Result<ModelMetadata, MetadataError> {
    let url = format!("{}/api/show", base_url.trim_end_matches('/'));

    let request = OllamaShowRequest {
        model: model.to_string(),
    };

    let response = client
        .post(&url)
        .json(&request)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

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

    let info: OllamaShowResponse = response.json().await.map_err(|e| {
        MetadataError::InvalidResponse(format!("Failed to parse Ollama show response: {}", e))
    })?;

    // Extract context length before moving fields out of `info`.
    let context_length = extract_context_length(&info);

    // HFリポ情報をmodel_infoから抽出
    let hf_repo = extract_hf_repo(&info);

    let mut metadata = ModelMetadata {
        model: info.model.unwrap_or_else(|| model.to_string()),
        context_length,
        hf_repo,
        ..Default::default()
    };

    // Extract details
    if let Some(details) = info.details {
        metadata.family = details.family;
        metadata.parameter_size = details.parameter_size;
        metadata.quantization = details.quantization_level;
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_show_response_deserialization() {
        let json = r#"{
            "model": "llama3.2:1b",
            "details": {
                "family": "llama",
                "parameter_size": "1B",
                "quantization_level": "Q4_K_M"
            },
            "parameters": {
                "num_ctx": 8192
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.model, Some("llama3.2:1b".to_string()));
        assert!(response.details.is_some());
        assert!(response.parameters.is_some());
        assert_eq!(extract_context_length(&response), Some(8192));

        let details = response.details.unwrap();
        assert_eq!(details.family, Some("llama".to_string()));
        assert_eq!(details.parameter_size, Some("1B".to_string()));
    }

    #[test]
    fn test_ollama_show_response_with_name_alias() {
        let json = r#"{
            "name": "mistral:7b",
            "details": {
                "family": "mistral"
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.model, Some("mistral:7b".to_string()));
    }

    #[test]
    fn test_ollama_show_response_minimal() {
        let json = r#"{}"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert!(response.model.is_none());
        assert!(response.details.is_none());
        assert!(response.parameters.is_none());
    }

    #[test]
    fn test_ollama_show_request_serialization() {
        let request = OllamaShowRequest {
            model: "llama3.2:1b".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(json, r#"{"model":"llama3.2:1b"}"#);
    }

    #[test]
    fn test_ollama_show_response_with_model_info_alias() {
        // model_info map may contain context length with normalized key.
        let json = r#"{
            "model": "phi3",
            "model_info": {
                "context_length": 4096
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.model, Some("phi3".to_string()));
        assert_eq!(extract_context_length(&response), Some(4096));
    }

    #[test]
    fn test_ollama_show_response_with_model_info_dotted_key() {
        let json = r#"{
            "model_info": {
                "qwen3moe.context_length": 262144
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_context_length(&response), Some(262144));
    }

    #[test]
    fn test_ollama_show_response_with_parameters_string() {
        let json = r#"{
            "model": "gpt-oss:20b",
            "parameters": "num_keep 0\nnum_ctx 131072\ntemperature 0.8"
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_context_length(&response), Some(131072));
    }

    #[test]
    fn test_extract_context_length_prefers_model_info() {
        let json = r#"{
            "model_info": {
                "context_length": 262144
            },
            "parameters": {
                "num_ctx": 8192
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_context_length(&response), Some(262144));
    }

    #[test]
    fn test_extract_hf_repo_from_model_info_repo_url() {
        let json = r#"{
            "model_info": {
                "general.base_model.repo_url": "https://huggingface.co/openai/gpt-oss-20b"
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_hf_repo(&response),
            Some("openai/gpt-oss-20b".to_string())
        );
    }

    #[test]
    fn test_extract_hf_repo_from_author_and_name() {
        let json = r#"{
            "model_info": {
                "general.author": "Qwen",
                "general.name": "Qwen3-30B"
            }
        }"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_hf_repo(&response),
            Some("Qwen/Qwen3-30B".to_string())
        );
    }

    #[test]
    fn test_extract_hf_repo_none_when_no_info() {
        let json = r#"{}"#;

        let response: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert!(extract_hf_repo(&response).is_none());
    }
}
