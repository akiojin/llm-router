//! モデル管理APIハンドラー
//!
//! コーディネーターからのモデルプル要求を受信・処理

use crate::ollama::OllamaManager;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

/// アプリケーション状態（Ollamaマネージャー）
#[derive(Clone)]
pub struct AppState {
    /// Ollamaマネージャー（モデルプル処理用）
    pub ollama_manager: Arc<Mutex<OllamaManager>>,
    /// コーディネーターURL（進捗報告用）
    pub coordinator_url: String,
}

/// モデルプルリクエスト
#[derive(Debug, Deserialize)]
pub struct PullModelRequest {
    /// プルするモデル名
    pub model: String,
    /// タスクID（進捗報告用）
    pub task_id: Uuid,
}

/// モデルプルレスポンス
#[derive(Debug, Serialize)]
pub struct PullModelResponse {
    /// 成功メッセージ
    pub message: String,
    /// プルしたモデル名
    pub model: String,
}

/// 進捗報告用リクエスト
#[derive(Debug, Serialize)]
struct ProgressUpdate {
    /// 進捗（0.0-1.0）
    progress: f32,
    /// ダウンロード速度（bytes/sec、オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<u64>,
}

/// POST /pull - コーディネーターからのモデルプル要求
pub async fn pull_model(
    State(state): State<AppState>,
    Json(request): Json<PullModelRequest>,
) -> Result<Json<PullModelResponse>, AppError> {
    let model = request.model.clone();
    let task_id = request.task_id;

    info!(
        "Received model pull request: model={}, task_id={}",
        model, task_id
    );

    // 進捗0%を報告
    if let Err(e) = report_progress(&state.coordinator_url, task_id, 0.0, None).await {
        error!("Failed to report initial progress: {}", e);
    }

    // Ollamaマネージャーを取得
    let ollama_manager = state.ollama_manager.lock().await;

    // モデルをプル（既に存在する場合はスキップ）
    // TODO: Ollamaのストリーミングレスポンスから進捗を取得して報告
    ollama_manager
        .ensure_model(&model)
        .await
        .map_err(|e| AppError(format!("Failed to pull model {}: {}", model, e)))?;

    // 進捗100%を報告
    if let Err(e) = report_progress(&state.coordinator_url, task_id, 1.0, None).await {
        error!("Failed to report completion progress: {}", e);
    }

    info!("Model pull completed: model={}, task_id={}", model, task_id);

    Ok(Json(PullModelResponse {
        message: "Model pulled successfully".to_string(),
        model,
    }))
}

/// コーディネーターに進捗を報告
async fn report_progress(
    coordinator_url: &str,
    task_id: Uuid,
    progress: f32,
    speed: Option<u64>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/tasks/{}/progress", coordinator_url, task_id);

    let update = ProgressUpdate { progress, speed };

    client
        .post(&url)
        .json(&update)
        .send()
        .await
        .map_err(|e| format!("Failed to send progress: {}", e))?;

    Ok(())
}

/// Axum用のエラーレスポンス型
#[derive(Debug)]
pub struct AppError(
    /// エラーメッセージ
    String,
);

impl From<String> for AppError {
    fn from(value: String) -> Self {
        AppError(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        error!("API error: {}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": self.0
            })),
        )
            .into_response()
    }
}
