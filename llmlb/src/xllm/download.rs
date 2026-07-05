//! xLLM Model Download Client
//!
//! SPEC-e8e9326e: Model download request and progress tracking for xLLM endpoints

use crate::common::http::RequestBuilderBearerExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Error types for xLLM download operations
#[derive(Debug, Error)]
pub enum DownloadError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// xLLM returned an error response
    #[error("xLLM returned error: {status} - {message}")]
    XllmError {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },

    /// Invalid response format from xLLM
    #[error("Invalid response format: {0}")]
    InvalidResponse(String),
}

/// Request to download a model from HuggingFace
#[derive(Debug, Clone, Serialize)]
pub struct DownloadRequest {
    /// HuggingFace model repository (e.g., "bartowski/Llama-3.2-1B-Instruct-GGUF")
    pub repo: String,

    /// Optional filename to download (e.g., "Llama-3.2-1B-Instruct-Q4_K_M.gguf")
    /// If not specified, xLLM will choose the best quantization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// Response from xLLM download progress endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadProgressResponse {
    /// Download task ID
    pub task_id: String,

    /// Model being downloaded
    pub model: String,

    /// Current status: "pending", "downloading", "completed", "failed", "cancelled"
    pub status: String,

    /// Download progress (0.0 - 100.0)
    pub progress: f64,

    /// Download speed in MB/s (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<f64>,

    /// Estimated time remaining in seconds (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u32>,

    /// Error message if status is "failed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Downloaded filename (available when completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// xLLM download initiation response
#[derive(Debug, Clone, Deserialize)]
struct DownloadInitResponse {
    task_id: String,
    #[allow(dead_code)]
    model: String,
    #[allow(dead_code)]
    status: String,
}

/// Request model download from xLLM endpoint
///
/// Sends POST /api/models/download to the xLLM endpoint
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - xLLM endpoint base URL
/// * `api_key` - Optional API key for authentication
/// * `request` - Download request details
///
/// # Returns
/// Task ID for tracking download progress
pub async fn download_model(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    request: &DownloadRequest,
) -> Result<String, DownloadError> {
    let url = format!("{}/api/models/download", base_url.trim_end_matches('/'));

    let mut req_builder = client
        .post(&url)
        .json(request)
        .timeout(Duration::from_secs(30));

    req_builder = req_builder.bearer_opt(api_key);

    let response = req_builder.send().await?;
    let status = response.status();

    if !status.is_success() {
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(DownloadError::XllmError {
            status: status.as_u16(),
            message,
        });
    }

    let init_response: DownloadInitResponse = response.json().await.map_err(|e| {
        DownloadError::InvalidResponse(format!("Failed to parse download response: {}", e))
    })?;

    Ok(init_response.task_id)
}

/// Get download progress from xLLM endpoint
///
/// Sends GET /api/download/progress?task_id={task_id} to the xLLM endpoint
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - xLLM endpoint base URL
/// * `api_key` - Optional API key for authentication
/// * `task_id` - Download task ID
///
/// # Returns
/// Current download progress
pub async fn get_download_progress(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    task_id: &str,
) -> Result<DownloadProgressResponse, DownloadError> {
    let url = format!(
        "{}/api/download/progress?task_id={}",
        base_url.trim_end_matches('/'),
        task_id
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
        return Err(DownloadError::XllmError {
            status: status.as_u16(),
            message,
        });
    }

    let progress: DownloadProgressResponse = response.json().await.map_err(|e| {
        DownloadError::InvalidResponse(format!("Failed to parse progress response: {}", e))
    })?;

    Ok(progress)
}

#[cfg(test)]
mod tests;
