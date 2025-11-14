//! モデル管理API
//!
//! モデル一覧取得、配布、進捗追跡のエンドポイント

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ollama::OllamaClient,
    registry::models::{DownloadTask, InstalledModel, ModelInfo},
    AppState,
};
use ollama_coordinator_common::error::CoordinatorError;

/// モデル名の妥当性を検証
///
/// 有効なモデル名の形式: `name:tag` または `name`
/// - name: 小文字英数字、ハイフン、アンダースコア
/// - tag: 英数字、ピリオド、ハイフン
fn validate_model_name(model_name: &str) -> Result<(), CoordinatorError> {
    if model_name.is_empty() {
        return Err(CoordinatorError::InvalidModelName(
            "モデル名が空です".to_string(),
        ));
    }

    // 基本的な形式チェック
    let parts: Vec<&str> = model_name.split(':').collect();
    if parts.len() > 2 {
        return Err(CoordinatorError::InvalidModelName(format!(
            "無効なモデル名形式: {}",
            model_name
        )));
    }

    // 名前部分の検証
    let name = parts[0];
    if name.is_empty()
        || !name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.'
        })
    {
        return Err(CoordinatorError::InvalidModelName(format!(
            "無効なモデル名: {}",
            model_name
        )));
    }

    // タグ部分の検証（存在する場合）
    if parts.len() == 2 {
        let tag = parts[1];
        if tag.is_empty()
            || !tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == 'b')
        {
            return Err(CoordinatorError::InvalidModelName(format!(
                "無効なモデルタグ: {}",
                model_name
            )));
        }
    }

    Ok(())
}

/// 利用可能なモデル一覧のレスポンス
#[derive(Debug, Serialize)]
pub struct AvailableModelsResponse {
    /// モデル一覧
    pub models: Vec<ModelInfo>,
    /// ソース（"ollama_library" または "agents"）
    pub source: String,
}

/// モデル配布リクエスト
#[derive(Debug, Deserialize)]
pub struct DistributeModelsRequest {
    /// モデル名
    pub model_name: String,
    /// ターゲット（"all" または "specific"）
    pub target: String,
    /// エージェントID一覧（targetが"specific"の場合）
    #[serde(default)]
    pub agent_ids: Vec<Uuid>,
}

/// モデル配布レスポンス
#[derive(Debug, Serialize)]
pub struct DistributeModelsResponse {
    /// タスクID一覧
    pub task_ids: Vec<Uuid>,
}

/// モデルプルリクエスト
#[derive(Debug, Deserialize)]
pub struct PullModelRequest {
    /// モデル名
    pub model_name: String,
}

/// モデルプルレスポンス
#[derive(Debug, Serialize)]
pub struct PullModelResponse {
    /// タスクID
    pub task_id: Uuid,
}

/// タスク進捗更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateProgressRequest {
    /// 進捗（0.0-1.0）
    pub progress: f32,
    /// ダウンロード速度（bytes/sec、オプション）
    #[serde(default)]
    pub speed: Option<u64>,
}

/// Axum用のエラーレスポンス型
#[derive(Debug)]
pub struct AppError(CoordinatorError);

impl From<CoordinatorError> for AppError {
    fn from(err: CoordinatorError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            CoordinatorError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            CoordinatorError::NoAgentsAvailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string())
            }
            CoordinatorError::AgentOffline(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string())
            }
            CoordinatorError::InvalidModelName(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
            CoordinatorError::InsufficientStorage(_) => {
                (StatusCode::INSUFFICIENT_STORAGE, self.0.to_string())
            }
            CoordinatorError::Database(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string())
            }
            CoordinatorError::Http(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            CoordinatorError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            CoordinatorError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string())
            }
            CoordinatorError::Common(err) => (StatusCode::BAD_REQUEST, err.to_string()),
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// T027: GET /api/models/available - 利用可能なモデル一覧を取得
pub async fn get_available_models(
    State(_state): State<AppState>,
) -> Result<Json<AvailableModelsResponse>, AppError> {
    tracing::debug!("Fetching available models from Ollama library");

    let client = OllamaClient::new()?;

    // 事前定義モデルを取得（エージェントからの取得は後で実装）
    let models = client.get_predefined_models();

    tracing::info!("Available models retrieved: count={}", models.len());

    Ok(Json(AvailableModelsResponse {
        models,
        source: "ollama_library".to_string(),
    }))
}

/// T028: POST /api/models/distribute - モデルを配布
pub async fn distribute_models(
    State(state): State<AppState>,
    Json(request): Json<DistributeModelsRequest>,
) -> Result<(StatusCode, Json<DistributeModelsResponse>), AppError> {
    tracing::info!(
        "Model distribution request: model={}, target={}",
        request.model_name,
        request.target
    );

    // モデル名のバリデーション
    if let Err(e) = validate_model_name(&request.model_name) {
        tracing::error!(
            "Model name validation failed: model={}, error={}",
            request.model_name,
            e
        );
        return Err(e.into());
    }

    // ターゲットエージェントを決定
    let agent_ids = match request.target.as_str() {
        "all" => {
            // 全エージェントを取得
            let agents = state.registry.list().await;
            agents.into_iter().map(|a| a.id).collect()
        }
        "specific" => request.agent_ids.clone(),
        _ => {
            return Err(CoordinatorError::Internal(
                "Invalid target. Must be 'all' or 'specific'".to_string(),
            )
            .into());
        }
    };

    // 各エージェントID が存在することを確認し、タスクを作成
    let mut task_ids = Vec::new();
    for agent_id in agent_ids {
        // エージェントが存在することを確認
        let agent = state.registry.get(agent_id).await?;

        // エージェントがオンラインであることを確認
        if agent.status != ollama_coordinator_common::types::AgentStatus::Online {
            tracing::error!(
                "Cannot distribute to offline agent: agent_id={}, status={:?}",
                agent_id,
                agent.status
            );
            return Err(CoordinatorError::AgentOffline(agent_id).into());
        }

        // タスクを作成
        let task = state
            .task_manager
            .create_task(agent_id, request.model_name.clone())
            .await;
        let task_id = task.id;
        task_ids.push(task_id);

        tracing::info!(
            "Created distribution task {} for agent {} with model {}",
            task_id,
            agent_id,
            request.model_name
        );

        // エージェントにモデルプル要求を送信（バックグラウンド）
        let agent_api_port = agent.ollama_port + 1;
        let agent_url = format!("http://{}:{}/pull", agent.ip_address, agent_api_port);
        let model_name = request.model_name.clone();

        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let pull_request = serde_json::json!({
                "model": model_name,
                "task_id": task_id,
            });

            match client.post(&agent_url).json(&pull_request).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::info!("Successfully sent pull request to agent {}", agent_id);
                    } else {
                        tracing::error!(
                            "Agent {} returned error status: {}",
                            agent_id,
                            response.status()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to send pull request to agent {}: {}", agent_id, e);
                }
            }
        });
    }

    tracing::info!(
        "Model distribution initiated: model={}, tasks_created={}, task_ids={:?}",
        request.model_name,
        task_ids.len(),
        task_ids
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(DistributeModelsResponse { task_ids }),
    ))
}

/// T029: GET /api/agents/{agent_id}/models - エージェントのインストール済みモデル一覧を取得
pub async fn get_agent_models(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<InstalledModel>>, AppError> {
    // エージェントが存在することを確認
    let agent = state.registry.get(agent_id).await?;

    // エージェントからモデル一覧を取得（実装は後で）
    let agent_url = format!("http://{}:{}", agent.ip_address, agent.ollama_port);
    tracing::info!("Fetching models from agent at {}", agent_url);

    // TODO: エージェントのOllama APIからモデル一覧を取得
    // 現在は空の配列を返す
    Ok(Json(Vec::new()))
}

/// T030: POST /api/agents/{agent_id}/models/pull - エージェントにモデルプルを指示
pub async fn pull_model_to_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    Json(request): Json<PullModelRequest>,
) -> Result<(StatusCode, Json<PullModelResponse>), AppError> {
    tracing::info!(
        "Model pull request: agent_id={}, model={}",
        agent_id,
        request.model_name
    );

    // モデル名のバリデーション
    if let Err(e) = validate_model_name(&request.model_name) {
        tracing::error!(
            "Model name validation failed: model={}, error={}",
            request.model_name,
            e
        );
        return Err(e.into());
    }

    // エージェントが存在することを確認
    let agent = state.registry.get(agent_id).await?;

    // エージェントがオンラインであることを確認
    if agent.status != ollama_coordinator_common::types::AgentStatus::Online {
        tracing::error!(
            "Cannot pull to offline agent: agent_id={}, status={:?}",
            agent_id,
            agent.status
        );
        return Err(CoordinatorError::AgentOffline(agent_id).into());
    }

    // タスクを作成
    let task = state
        .task_manager
        .create_task(agent_id, request.model_name.clone())
        .await;
    let task_id = task.id;

    tracing::info!(
        "Created pull task {} for agent {} with model {}",
        task_id,
        agent_id,
        request.model_name
    );

    // エージェントにモデルプル要求を送信（バックグラウンド）
    let agent_api_port = agent.ollama_port + 1;
    let agent_url = format!("http://{}:{}/pull", agent.ip_address, agent_api_port);
    let model_name = request.model_name.clone();

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let pull_request = serde_json::json!({
            "model": model_name,
            "task_id": task_id,
        });

        match client.post(&agent_url).json(&pull_request).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    tracing::info!("Successfully sent pull request to agent {}", agent_id);
                } else {
                    tracing::error!(
                        "Agent {} returned error status: {}",
                        agent_id,
                        response.status()
                    );
                }
            }
            Err(e) => {
                tracing::error!("Failed to send pull request to agent {}: {}", agent_id, e);
            }
        }
    });

    Ok((StatusCode::ACCEPTED, Json(PullModelResponse { task_id })))
}

/// T031: GET /api/tasks/{task_id} - タスク進捗を取得
pub async fn get_task_progress(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<DownloadTask>, AppError> {
    tracing::debug!("Task progress query: task_id={}", task_id);

    // タスクマネージャーからタスクを取得
    let task = state.task_manager.get_task(task_id).await.ok_or_else(|| {
        tracing::error!("Task not found: task_id={}", task_id);
        CoordinatorError::Internal(format!("Task {} not found", task_id))
    })?;

    Ok(Json(task))
}

/// T034: POST /api/tasks/{task_id}/progress - タスク進捗を更新（エージェントから呼ばれる）
pub async fn update_progress(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(request): Json<UpdateProgressRequest>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(
        "Updating progress for task {}: progress={}, speed={:?}",
        task_id,
        request.progress,
        request.speed
    );

    // タスクの進捗を更新
    state
        .task_manager
        .update_progress(task_id, request.progress, request.speed)
        .await
        .ok_or_else(|| {
            tracing::error!(
                "Failed to update progress, task not found: task_id={}",
                task_id
            );
            CoordinatorError::Internal(format!("Task {} not found", task_id))
        })?;

    // 完了時に特別なログを出力
    if request.progress >= 1.0 {
        tracing::info!("Task completed: task_id={}", task_id);
    } else if request.progress == 0.0 {
        tracing::info!("Task started: task_id={}", task_id);
    }

    Ok(StatusCode::OK)
}
