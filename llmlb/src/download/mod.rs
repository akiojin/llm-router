//! Model Download Dispatcher
//!
//! Routes download requests to the appropriate engine handler based on endpoint type.
//! Follows the dispatcher pattern used in `crate::metadata::mod.rs`.

pub mod lm_studio;
pub mod ollama;

use crate::db::download_tasks as tasks_db;
use crate::types::endpoint::{EndpointType, ModelDownloadTask};
use reqwest::Client;
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

/// Error types for model download operations
#[derive(Debug, Error)]
pub enum DownloadError {
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

    /// Invalid response format
    #[error("Invalid response format: {0}")]
    InvalidResponse(String),

    /// Download not supported for this endpoint type
    #[error("Download not supported for endpoint type: {0}")]
    UnsupportedType(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Download request parameters for the dispatcher
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Model name (canonical or engine-specific)
    pub model: String,
    /// HuggingFace repository (for LM Studio)
    pub hf_repo: Option<String>,
    /// Quantization type (for LM Studio, e.g., "Q4_K_M")
    pub quantization: Option<String>,
}

/// Dispatch a model download request to the appropriate engine handler
///
/// Creates a download task in the DB, then spawns a background task to perform
/// the actual download for engines that support it.
///
/// # Arguments
/// * `client` - HTTP client
/// * `base_url` - Endpoint base URL
/// * `api_key` - Optional API key
/// * `endpoint_type` - Type of the endpoint
/// * `request` - Download request parameters
/// * `pool` - Database connection pool
/// * `endpoint_id` - Endpoint UUID
///
/// # Returns
/// The created download task
pub async fn download_model(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    endpoint_type: &EndpointType,
    request: &DownloadRequest,
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<ModelDownloadTask, DownloadError> {
    // Check if this endpoint type supports downloads
    if !endpoint_type.supports_model_download() {
        return Err(DownloadError::UnsupportedType(
            endpoint_type.as_str().to_string(),
        ));
    }

    // Resolve engine-specific model name
    let engine_model = crate::models::mapping::resolve_engine_name(&request.model, endpoint_type)
        .map(|s| s.to_string())
        .unwrap_or_else(|| request.model.clone());

    // Create download task in DB
    let task = ModelDownloadTask::new(endpoint_id, request.model.clone());
    tasks_db::create_download_task(pool, &task).await?;

    let task_id = task.id.clone();
    let pool = pool.clone();

    match endpoint_type {
        EndpointType::Xllm => {
            // xLLM handles download internally; task is just a marker.
            // No background work needed from llmlb side.
        }
        EndpointType::Ollama => {
            let client = client.clone();
            let base_url = base_url.to_string();
            let model_name = engine_model;
            let task_id_bg = task_id.clone();
            let pool_bg = pool.clone();

            tokio::spawn(async move {
                // Update task to downloading
                let _ = tasks_db::update_download_progress(&pool_bg, &task_id_bg, 0.0, None, None)
                    .await;

                match ollama::pull_model(&client, &base_url, &model_name).await {
                    Ok(()) => {
                        let _ = tasks_db::complete_download_task(&pool_bg, &task_id_bg, None).await;
                        tracing::info!(
                            "Ollama model download completed: {} on endpoint",
                            model_name
                        );
                    }
                    Err(e) => {
                        let _ = tasks_db::fail_download_task(&pool_bg, &task_id_bg, &e.to_string())
                            .await;
                        tracing::error!("Ollama model download failed: {} - {}", model_name, e);
                    }
                }
            });
        }
        EndpointType::LmStudio => {
            let client = client.clone();
            let base_url = base_url.to_string();
            let api_key = api_key.map(|s| s.to_string());
            let hf_repo = request
                .hf_repo
                .clone()
                .unwrap_or_else(|| engine_model.clone());
            let quantization = request
                .quantization
                .clone()
                .unwrap_or_else(|| "Q4_K_M".to_string());
            let task_id_bg = task_id.clone();
            let pool_bg = pool.clone();

            tokio::spawn(async move {
                // Update task to downloading
                let _ = tasks_db::update_download_progress(&pool_bg, &task_id_bg, 0.0, None, None)
                    .await;

                match lm_studio::download_model(
                    &client,
                    &base_url,
                    api_key.as_deref(),
                    &hf_repo,
                    &quantization,
                )
                .await
                {
                    Ok(_resp) => {
                        // LM Studio download is fire-and-forget; mark as completed
                        // (actual completion should be verified by polling /v1/models)
                        let _ = tasks_db::complete_download_task(&pool_bg, &task_id_bg, None).await;
                        tracing::info!("LM Studio model download request accepted: {}", hf_repo);
                    }
                    Err(e) => {
                        let _ = tasks_db::fail_download_task(&pool_bg, &task_id_bg, &e.to_string())
                            .await;
                        tracing::error!("LM Studio model download failed: {} - {}", hf_repo, e);
                    }
                }
            });
        }
        EndpointType::Llamacpp | EndpointType::Vllm | EndpointType::OpenaiCompatible => {
            // Should not reach here due to supports_model_download() check above
            unreachable!()
        }
    }

    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_error_display() {
        let err = DownloadError::EndpointError {
            status: 404,
            message: "Model not found".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Endpoint returned error: 404 - Model not found"
        );
    }

    #[test]
    fn test_download_error_unsupported_type() {
        let err = DownloadError::UnsupportedType("vllm".to_string());
        assert_eq!(
            err.to_string(),
            "Download not supported for endpoint type: vllm"
        );
    }

    #[test]
    fn test_download_error_invalid_response() {
        let err = DownloadError::InvalidResponse("bad json".to_string());
        assert_eq!(err.to_string(), "Invalid response format: bad json");
    }

    #[test]
    fn test_download_request_construction() {
        let req = DownloadRequest {
            model: "llama-3.2-1b".to_string(),
            hf_repo: Some("meta-llama/Llama-3.2-1B-Instruct-GGUF".to_string()),
            quantization: Some("Q4_K_M".to_string()),
        };
        assert_eq!(req.model, "llama-3.2-1b");
        assert!(req.hf_repo.is_some());
        assert!(req.quantization.is_some());
    }

    #[test]
    fn test_download_request_minimal() {
        let req = DownloadRequest {
            model: "gpt-oss:20b".to_string(),
            hf_repo: None,
            quantization: None,
        };
        assert_eq!(req.model, "gpt-oss:20b");
        assert!(req.hf_repo.is_none());
        assert!(req.quantization.is_none());
    }

    #[test]
    fn test_unsupported_types_rejected() {
        assert!(!EndpointType::Vllm.supports_model_download());
        assert!(!EndpointType::Llamacpp.supports_model_download());
        assert!(!EndpointType::OpenaiCompatible.supports_model_download());
    }

    #[test]
    fn test_supported_types_accepted() {
        assert!(EndpointType::Xllm.supports_model_download());
        assert!(EndpointType::Ollama.supports_model_download());
        assert!(EndpointType::LmStudio.supports_model_download());
    }
}
