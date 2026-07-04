//! LM Studio Model Download
//!
//! POST {base_url}/api/v1/models/download to download models on LM Studio endpoints
//!
//! NOTE: LM Studio (0.4.6) does not provide a progress API.
//! The download is fire-and-forget; model availability can be polled via /v1/models.

use crate::common::http::RequestBuilderBearerExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::DownloadError;

/// LM Studio download request body
#[derive(Debug, Serialize)]
struct LmStudioDownloadRequest {
    /// HuggingFace model URL (e.g., "https://huggingface.co/lmstudio-community/gemma-3-1b-it-GGUF")
    model: String,
    /// Quantization type (e.g., "Q4_K_M")
    quantization: String,
}

/// LM Studio download response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LmStudioDownloadResponse {
    /// Status or message from LM Studio
    #[serde(default)]
    pub status: Option<String>,
    /// Model identifier if available
    #[serde(default)]
    pub model: Option<String>,
}

/// Download a model on an LM Studio endpoint
///
/// Sends POST /api/v1/models/download. LM Studio requires the model to be
/// specified as a HuggingFace URL, not a catalog ID.
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - LM Studio endpoint base URL
/// * `api_key` - Optional API key for authentication
/// * `hf_repo` - HuggingFace repository (e.g., "lmstudio-community/gemma-3-1b-it-GGUF")
/// * `quantization` - Quantization type (e.g., "Q4_K_M")
pub async fn download_model(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    hf_repo: &str,
    quantization: &str,
) -> Result<LmStudioDownloadResponse, DownloadError> {
    let url = format!("{}/api/v1/models/download", base_url.trim_end_matches('/'));

    // LM Studio expects the model as a HuggingFace URL
    let hf_url = if hf_repo.starts_with("https://") {
        hf_repo.to_string()
    } else {
        format!("https://huggingface.co/{}", hf_repo)
    };

    let request = LmStudioDownloadRequest {
        model: hf_url,
        quantization: quantization.to_string(),
    };

    let mut req_builder = client
        .post(&url)
        .json(&request)
        .timeout(Duration::from_secs(30)); // Just the request timeout, not the download itself

    req_builder = req_builder.bearer_opt(api_key);

    let response = req_builder.send().await.map_err(DownloadError::Http)?;

    let status = response.status();

    if !status.is_success() {
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(DownloadError::EndpointError {
            status: status.as_u16(),
            message,
        });
    }

    // Try to parse response; LM Studio may return empty or minimal JSON
    let text = response.text().await.unwrap_or_else(|_| "{}".to_string());

    let resp: LmStudioDownloadResponse =
        serde_json::from_str(&text).unwrap_or(LmStudioDownloadResponse {
            status: Some("accepted".to_string()),
            model: None,
        });

    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lm_studio_download_request_serialization() {
        let req = LmStudioDownloadRequest {
            model: "https://huggingface.co/lmstudio-community/gemma-3-1b-it-GGUF".to_string(),
            quantization: "Q4_K_M".to_string(),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(
            "\"model\":\"https://huggingface.co/lmstudio-community/gemma-3-1b-it-GGUF\""
        ));
        assert!(json.contains("\"quantization\":\"Q4_K_M\""));
    }

    #[test]
    fn test_lm_studio_download_response_deserialization() {
        let json = r#"{"status": "downloading", "model": "gemma-3-1b"}"#;
        let resp: LmStudioDownloadResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, Some("downloading".to_string()));
        assert_eq!(resp.model, Some("gemma-3-1b".to_string()));
    }

    #[test]
    fn test_lm_studio_download_response_empty() {
        let json = r#"{}"#;
        let resp: LmStudioDownloadResponse = serde_json::from_str(json).unwrap();
        assert!(resp.status.is_none());
        assert!(resp.model.is_none());
    }

    #[test]
    fn test_hf_url_construction_from_repo() {
        // Simulate the URL construction logic
        let hf_repo = "lmstudio-community/gemma-3-1b-it-GGUF";
        let hf_url = if hf_repo.starts_with("https://") {
            hf_repo.to_string()
        } else {
            format!("https://huggingface.co/{}", hf_repo)
        };
        assert_eq!(
            hf_url,
            "https://huggingface.co/lmstudio-community/gemma-3-1b-it-GGUF"
        );
    }

    #[test]
    fn test_hf_url_passthrough_if_already_url() {
        let hf_repo = "https://huggingface.co/my-org/my-model";
        let hf_url = if hf_repo.starts_with("https://") {
            hf_repo.to_string()
        } else {
            format!("https://huggingface.co/{}", hf_repo)
        };
        assert_eq!(hf_url, "https://huggingface.co/my-org/my-model");
    }
}
