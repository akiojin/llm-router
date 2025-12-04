//! モデル管理API
//!
//! モデル一覧取得、配布、進捗追跡のエンドポイント

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    registry::models::{
        ensure_router_model_cached, model_name_to_dir, router_model_path, router_models_dir,
        DownloadStatus, DownloadTask, InstalledModel, ModelInfo, ModelSource,
    },
    runtime::RuntimeClient,
    AppState,
};
use llm_router_common::error::RouterError;

/// モデル名の妥当性を検証
///
/// 有効なモデル名の形式: `name:tag` または `name`
/// - name: 小文字英数字、ハイフン、アンダースコア
/// - tag: 英数字、ピリオド、ハイフン
fn validate_model_name(model_name: &str) -> Result<(), RouterError> {
    if model_name.starts_with("hf/") {
        // HF形式はプレフィックスだけ確認
        if model_name.len() > 3 {
            return Ok(());
        }
        return Err(RouterError::InvalidModelName(
            "無効なHFモデル名".to_string(),
        ));
    }
    if model_name.is_empty() {
        return Err(RouterError::InvalidModelName(
            "モデル名が空です".to_string(),
        ));
    }

    // 基本的な形式チェック
    let parts: Vec<&str> = model_name.split(':').collect();
    if parts.len() > 2 {
        return Err(RouterError::InvalidModelName(format!(
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
        return Err(RouterError::InvalidModelName(format!(
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
            return Err(RouterError::InvalidModelName(format!(
                "無効なモデルタグ: {}",
                model_name
            )));
        }
    }

    Ok(())
}

/// 利用可能なモデル一覧のレスポンスDTO
#[derive(Debug, Serialize)]
pub struct AvailableModelView {
    /// モデルID（例: gpt-oss:20b）
    pub name: String,
    /// UI表示名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 説明文
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// タグの一覧
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// GB単位のサイズ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_gb: Option<f64>,
    /// 推奨GPUメモリ(GB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_memory_gb: Option<f64>,
}

/// 利用可能なモデル一覧レスポンス
#[derive(Debug, Serialize)]
pub struct AvailableModelsResponse {
    /// モデル一覧（UI表示用に整形済み）
    pub models: Vec<AvailableModelView>,
    /// ソース（"builtin" / "hf" など）
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// キャッシュヒットかどうか
    pub cached: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// ページネーション情報
    pub pagination: Option<Pagination>,
}

#[derive(Debug, Serialize)]
/// ページネーション情報
pub struct Pagination {
    /// 取得件数
    pub limit: u32,
    /// オフセット
    pub offset: u32,
    /// 総件数（未取得時は0）
    pub total: u32,
}

/// 複数ノードにまたがるロード済みモデルの集計
#[derive(Debug, Serialize)]
pub struct LoadedModelSummary {
    /// モデル名
    pub model_name: String,
    /// 該当モデルを報告したノード数
    pub total_nodes: usize,
    /// 待機中ノード数
    pub pending: usize,
    /// ダウンロード中ノード数
    pub downloading: usize,
    /// 完了ノード数
    pub completed: usize,
    /// 失敗ノード数
    pub failed: usize,
}

fn model_info_to_view(model: ModelInfo) -> AvailableModelView {
    let size_gb = if model.size > 0 {
        Some((model.size as f64) / (1024.0 * 1024.0 * 1024.0))
    } else {
        None
    };
    let required_memory_gb = if model.required_memory > 0 {
        Some((model.required_memory as f64) / (1024.0 * 1024.0 * 1024.0))
    } else {
        None
    };
    let display_name = if let Some((prefix, tag)) = model.name.split_once(':') {
        Some(format!("{} {}", prefix.to_uppercase(), tag.to_uppercase()))
    } else {
        Some(model.name.clone())
    };

    AvailableModelView {
        name: model.name,
        display_name,
        description: Some(model.description),
        tags: Some(model.tags),
        size_gb,
        required_memory_gb,
    }
}

// ===== Registered model store (in-memory) =====
static REGISTERED_MODELS: Lazy<RwLock<Vec<ModelInfo>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// 登録済みモデルをストレージからロード
pub async fn load_registered_models_from_storage() {
    if let Ok(models) = crate::db::models::load_models().await {
        let mut store = REGISTERED_MODELS.write().unwrap();
        *store = models;
    }
}

/// 登録済みモデル一覧を取得
pub fn list_registered_models() -> Vec<ModelInfo> {
    REGISTERED_MODELS.read().unwrap().clone()
}

fn find_model_by_name(name: &str) -> Option<ModelInfo> {
    let client = RuntimeClient::new().ok()?;
    let mut all = client.get_predefined_models();
    all.extend(list_registered_models());
    all.into_iter().find(|m| m.name == name)
}

/// 登録モデルを追加（重複チェックあり）
fn add_registered_model(model: ModelInfo) -> Result<(), RouterError> {
    let mut store = REGISTERED_MODELS.write().unwrap();
    if store.iter().any(|m| m.name == model.name) {
        return Err(RouterError::InvalidModelName("重複登録されています".into()));
    }
    store.push(model);
    Ok(())
}

/// 登録モデルを永続化（失敗はログのみ）
pub async fn persist_registered_models() {
    if let Ok(store) = std::panic::catch_unwind(|| REGISTERED_MODELS.read().unwrap().clone()) {
        let _ = crate::db::models::save_models(&store).await;
    }
}

// ===== HF available cache =====
#[derive(Clone)]
struct HfCache {
    fetched_at: Instant,
    models: Vec<ModelInfo>,
}

static HF_CACHE: Lazy<RwLock<Option<HfCache>>> = Lazy::new(|| RwLock::new(None));

const HF_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
/// HFカタログクエリ
pub struct AvailableQuery {
    /// 部分一致検索
    pub search: Option<String>,
    /// 取得件数
    pub limit: Option<u32>,
    /// オフセット
    pub offset: Option<u32>,
    /// ソース指定（hf/builtin）
    pub source: Option<String>,
}

#[derive(Deserialize)]
struct HfModel {
    #[serde(rename = "modelId")]
    model_id: String,
    tags: Option<Vec<String>>,
    #[serde(default)]
    siblings: Vec<HfSibling>,
    #[serde(rename = "lastModified")]
    last_modified: Option<String>,
}

#[derive(Deserialize)]
struct HfSibling {
    #[serde(rename = "rfilename")]
    rfilename: String,
    size: Option<u64>,
}

async fn fetch_hf_models(query: &AvailableQuery) -> Result<(Vec<ModelInfo>, bool), RouterError> {
    // cache hit
    if query.search.is_none() {
        if let Some(cache) = HF_CACHE.read().unwrap().as_ref() {
            if cache.fetched_at.elapsed() < HF_CACHE_TTL {
                return Ok((cache.models.clone(), true));
            }
        }
    }

    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);
    let search = query.search.clone().unwrap_or_default();

    let token = std::env::var("HF_TOKEN").ok();
    let url = format!(
        "https://huggingface.co/api/models?library=gguf&limit={limit}&offset={offset}&search={search}&fields=modelId,tags,lastModified,siblings"
    );

    let client = reqwest::Client::new();
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| RouterError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(RouterError::Http(resp.status().to_string()));
    }
    let models_raw: Vec<HfModel> = resp
        .json()
        .await
        .map_err(|e| RouterError::Http(e.to_string()))?;

    let mut out = Vec::new();
    for m in models_raw {
        // pick gguf siblings
        for sib in m.siblings {
            if !sib.rfilename.ends_with(".gguf") {
                continue;
            }
            let name = format!("hf/{}/{}", m.model_id, sib.rfilename);
            let size = sib.size.unwrap_or(0);
            let download_url = format!(
                "https://huggingface.co/{}/resolve/main/{}",
                m.model_id, sib.rfilename
            );
            let tags = m.tags.clone().unwrap_or_default();
            let last_modified = m
                .last_modified
                .as_ref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));

            out.push(ModelInfo {
                name: name.clone(),
                size,
                description: m.model_id.clone(),
                required_memory: size,
                tags,
                source: ModelSource::HfGguf,
                download_url: Some(download_url),
                path: None,
                chat_template: None,
                repo: Some(m.model_id.clone()),
                filename: Some(sib.rfilename.clone()),
                last_modified,
                status: Some("available".into()),
            });
        }
    }

    if query.search.is_none() {
        *HF_CACHE.write().unwrap() = Some(HfCache {
            fetched_at: Instant::now(),
            models: out.clone(),
        });
    }

    Ok((out, false))
}

/// モデル配布リクエスト
#[derive(Debug, Deserialize)]
pub struct DistributeModelsRequest {
    /// モデル名
    pub model_name: String,
    /// ターゲット（"all" または "specific"）
    pub target: String,
    /// ノードID一覧（targetが"specific"の場合）
    #[serde(default)]
    pub node_ids: Vec<Uuid>,
}

/// HFからモデルをpullするリクエスト
#[derive(Debug, Deserialize)]
pub struct PullFromHfRequest {
    /// HFリポジトリ名 (例: TheBloke/gpt-oss-GGUF)
    pub repo: String,
    /// ファイル名 (例: gpt-oss-20b.Q4_K_M.gguf)
    pub filename: String,
    /// オプションのchat_template
    #[serde(default)]
    pub chat_template: Option<String>,
}

/// HF pull APIのレスポンス
#[derive(Debug, Serialize)]
pub struct PullFromHfResponse {
    /// 登録名（hf/<repo>/<file>）
    pub name: String,
    /// ルーター側にキャッシュされたローカルパス
    pub path: String,
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
pub struct AppError(RouterError);

impl From<RouterError> for AppError {
    fn from(err: RouterError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            RouterError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            RouterError::NoAgentsAvailable => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            RouterError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            RouterError::AgentOffline(_) => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            RouterError::InvalidModelName(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
            RouterError::InsufficientStorage(_) => {
                (StatusCode::INSUFFICIENT_STORAGE, self.0.to_string())
            }
            RouterError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Http(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            RouterError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            RouterError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::PasswordHash(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Jwt(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Authentication(_) => (StatusCode::UNAUTHORIZED, self.0.to_string()),
            RouterError::Authorization(_) => (StatusCode::FORBIDDEN, self.0.to_string()),
            RouterError::Common(err) => (StatusCode::BAD_REQUEST, err.to_string()),
        };

        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// T027: GET /api/models/available - 利用可能なモデル一覧を取得
pub async fn get_available_models(
    State(_state): State<AppState>,
    Query(query): Query<AvailableQuery>,
) -> Result<Json<AvailableModelsResponse>, AppError> {
    // Default to built-in model list for backward compatibility.
    // Hugging Face catalog is available via `source=hf` query parameter.
    let source = query.source.clone().unwrap_or_else(|| "builtin".into());

    if source == "hf" {
        tracing::debug!("Fetching available models from Hugging Face");
        let (models, cached) = fetch_hf_models(&query).await?;
        let models_view = models.into_iter().map(model_info_to_view).collect();
        return Ok(Json(AvailableModelsResponse {
            models: models_view,
            source: "hf".to_string(),
            cached: Some(cached),
            pagination: Some(Pagination {
                limit: query.limit.unwrap_or(20),
                offset: query.offset.unwrap_or(0),
                total: 0,
            }),
        }));
    }

    tracing::debug!("Fetching available models from builtin list");
    let client = RuntimeClient::new()?;
    let models = client.get_predefined_models();
    let models_view = models.into_iter().map(model_info_to_view).collect();
    Ok(Json(AvailableModelsResponse {
        models: models_view,
        source: "builtin".to_string(),
        cached: None,
        pagination: None,
    }))
}

#[derive(Deserialize)]
/// HFモデル登録リクエスト
pub struct RegisterModelRequest {
    /// HFリポジトリ名 (e.g., TheBloke/Llama-2-7B-GGUF)
    pub repo: String,
    /// ファイル名 (e.g., llama-2-7b.Q4_K_M.gguf)
    pub filename: String,
    /// 表示名（任意）
    #[serde(default)]
    pub display_name: Option<String>,
    /// オプションのchat_template（GGUFに含まれない場合の補助）
    #[serde(default)]
    pub chat_template: Option<String>,
}

/// POST /api/models/register - HF GGUFを対応モデルに登録
pub async fn register_model(
    Json(req): Json<RegisterModelRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let name = format!("hf/{}/{}", req.repo, req.filename);
    let download_url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        req.repo, req.filename
    );

    let model = ModelInfo {
        name: name.clone(),
        size: 0,
        description: req.display_name.unwrap_or_else(|| req.repo.clone()),
        required_memory: 0,
        tags: vec!["gguf".into()],
        source: ModelSource::HfGguf,
        download_url: Some(download_url),
        path: None,
        chat_template: req.chat_template.clone(),
        repo: Some(req.repo.clone()),
        filename: Some(req.filename.clone()),
        last_modified: None,
        status: Some("registered".into()),
    };

    add_registered_model(model)?;
    persist_registered_models().await;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "name": name,
            "status": "registered"
        })),
    ))
}

/// POST /api/models/pull - HFからダウンロードしローカルキャッシュに保存して登録
pub async fn pull_model_from_hf(
    Json(req): Json<PullFromHfRequest>,
) -> Result<(StatusCode, Json<PullFromHfResponse>), AppError> {
    let name = format!("hf/{}/{}", req.repo, req.filename);
    validate_model_name(&name)?;

    let download_url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        req.repo, req.filename
    );

    let base = router_models_dir().ok_or_else(|| RouterError::Internal("HOME not set".into()))?;
    let dir = base.join(model_name_to_dir(&name));
    let target = dir.join("model.gguf");

    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| RouterError::Internal(e.to_string()))?;

    // ダウンロード
    let client = reqwest::Client::new();
    let resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| RouterError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(RouterError::Http(resp.status().to_string()).into());
    }

    let mut file = tokio::fs::File::create(&target)
        .await
        .map_err(|e| RouterError::Internal(e.to_string()))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| RouterError::Http(e.to_string()))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| RouterError::Internal(e.to_string()))?;
    }

    let mut model = ModelInfo::new(name.clone(), 0, req.repo.clone(), 0, vec!["gguf".into()]);
    model.download_url = Some(download_url.clone());
    model.path = Some(target.to_string_lossy().to_string());
    model.chat_template = req.chat_template.clone();
    model.repo = Some(req.repo.clone());
    model.filename = Some(req.filename.clone());
    model.status = Some("cached".into());

    if let Ok(meta) = tokio::fs::metadata(&target).await {
        model.size = meta.len();
    }

    add_registered_model(model)?;
    persist_registered_models().await;

    Ok((
        StatusCode::CREATED,
        Json(PullFromHfResponse {
            name,
            path: target.to_string_lossy().to_string(),
        }),
    ))
}

/// GET /api/models/loaded - ルーター全体のロード済みモデル集計
pub async fn get_loaded_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<LoadedModelSummary>>, AppError> {
    // 現状はダウンロードタスクの状態を元に集計（ノード別ではなく全体）
    let tasks = state.task_manager.list_tasks().await;

    use std::collections::HashMap;
    let mut map: HashMap<String, LoadedModelSummary> = HashMap::new();

    for task in tasks {
        let entry = map
            .entry(task.model_name.clone())
            .or_insert(LoadedModelSummary {
                model_name: task.model_name.clone(),
                total_nodes: 0,
                pending: 0,
                downloading: 0,
                completed: 0,
                failed: 0,
            });

        entry.total_nodes += 1;
        match task.status {
            DownloadStatus::Pending => entry.pending += 1,
            DownloadStatus::InProgress => entry.downloading += 1,
            DownloadStatus::Completed => entry.completed += 1,
            DownloadStatus::Failed => entry.failed += 1,
        }
    }

    let mut list: Vec<LoadedModelSummary> = map.into_values().collect();
    list.sort_by(|a, b| a.model_name.cmp(&b.model_name));

    Ok(Json(list))
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

    // ターゲットノードを決定
    let node_ids = match request.target.as_str() {
        "all" => {
            // 全ノードを取得
            let nodes = state.registry.list().await;
            nodes.into_iter().map(|a| a.id).collect()
        }
        "specific" => request.node_ids.clone(),
        _ => {
            return Err(RouterError::Internal(
                "Invalid target. Must be 'all' or 'specific'".to_string(),
            )
            .into());
        }
    };

    // 各ノードID が存在することを確認し、タスクを作成
    let mut task_ids = Vec::new();
    for node_id in node_ids {
        // ノードが存在することを確認
        let node = state.registry.get(node_id).await?;

        // ノードがオンラインであることを確認
        if node.status != llm_router_common::types::NodeStatus::Online {
            tracing::error!(
                "Cannot distribute to offline node: node_id={}, status={:?}",
                node_id,
                node.status
            );
            return Err(RouterError::AgentOffline(node_id).into());
        }

        // タスクを作成
        let task = state
            .task_manager
            .create_task(node_id, request.model_name.clone())
            .await;
        let task_id = task.id;
        task_ids.push(task_id);

        tracing::info!(
            "Created distribution task {} for node {} with model {}",
            task_id,
            node_id,
            request.model_name
        );

        // ノードにモデルプル要求を送信（バックグラウンド）
        let node_api_port = node.runtime_port + 1;
        let node_url = format!("http://{}:{}/pull", node.ip_address, node_api_port);
        let model_name = request.model_name.clone();

        let model_info = find_model_by_name(&model_name);
        let cached = if let Some(mi) = &model_info {
            ensure_router_model_cached(mi).await
        } else {
            None
        };
        let shared_path = cached
            .or_else(|| model_info.as_ref().and_then(|m| router_model_path(&m.name)))
            .map(|p| p.to_string_lossy().to_string());
        let download_url = model_info.as_ref().and_then(|m| m.download_url.clone());
        let chat_template = model_info.as_ref().and_then(|m| m.chat_template.clone());

        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let pull_request = serde_json::json!({
                "model": model_name,
                "task_id": task_id,
                "path": shared_path,
                "download_url": download_url,
                "chat_template": chat_template,
            });

            match client.post(&node_url).json(&pull_request).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        tracing::info!("Successfully sent pull request to node {}", node_id);
                    } else {
                        tracing::error!(
                            "Node {} returned error status: {}",
                            node_id,
                            response.status()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to send pull request to node {}: {}", node_id, e);
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

/// T029: GET /api/nodes/{node_id}/models - ノードのインストール済みモデル一覧を取得
pub async fn get_node_models(
    State(state): State<AppState>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<InstalledModel>>, AppError> {
    // ノードが存在することを確認
    let node = state.registry.get(node_id).await?;

    // ノードからモデル一覧を取得（実装は後で）
    let node_url = format!("http://{}:{}", node.ip_address, node.runtime_port);
    tracing::info!("Fetching models from node at {}", node_url);

    // TODO: ノードのLLM runtime APIからモデル一覧を取得
    // 現在は空の配列を返す
    Ok(Json(Vec::new()))
}

/// T030: POST /api/nodes/{node_id}/models/pull - ノードにモデルプルを指示
pub async fn pull_model_to_node(
    State(state): State<AppState>,
    Path(node_id): Path<Uuid>,
    Json(request): Json<PullModelRequest>,
) -> Result<(StatusCode, Json<PullModelResponse>), AppError> {
    tracing::info!(
        "Model pull request: node_id={}, model={}",
        node_id,
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

    // ノードが存在することを確認
    let node = state.registry.get(node_id).await?;

    // ノードがオンラインであることを確認
    if node.status != llm_router_common::types::NodeStatus::Online {
        tracing::error!(
            "Cannot pull to offline node: node_id={}, status={:?}",
            node_id,
            node.status
        );
        return Err(RouterError::AgentOffline(node_id).into());
    }

    // タスクを作成
    let task = state
        .task_manager
        .create_task(node_id, request.model_name.clone())
        .await;
    let task_id = task.id;

    tracing::info!(
        "Created pull task {} for node {} with model {}",
        task_id,
        node_id,
        request.model_name
    );

    // ノードにモデルプル要求を送信（バックグラウンド）
    let node_api_port = node.runtime_port + 1;
    let node_url = format!("http://{}:{}/pull", node.ip_address, node_api_port);
    let model_name = request.model_name.clone();

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let pull_request = serde_json::json!({
            "model": model_name,
            "task_id": task_id,
        });

        match client.post(&node_url).json(&pull_request).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    tracing::info!("Successfully sent pull request to node {}", node_id);
                } else {
                    tracing::error!(
                        "Node {} returned error status: {}",
                        node_id,
                        response.status()
                    );
                }
            }
            Err(e) => {
                tracing::error!("Failed to send pull request to node {}: {}", node_id, e);
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
        RouterError::Internal(format!("Task {} not found", task_id))
    })?;

    Ok(Json(task))
}

/// GET /api/tasks - アクティブなタスク一覧を取得
pub async fn list_tasks(State(state): State<AppState>) -> Json<Vec<DownloadTask>> {
    let tasks = state.task_manager.list_active_tasks().await;
    Json(tasks)
}

/// T034: POST /api/tasks/{task_id}/progress - タスク進捗を更新（ノードから呼ばれる）
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
            RouterError::Internal(format!("Task {} not found", task_id))
        })?;

    // 進捗が完了に到達したら、ノードのloaded_modelsに反映
    if request.progress >= 1.0 {
        if let Some(task) = state.task_manager.get_task(task_id).await {
            if task.status == DownloadStatus::Completed {
                // モデルの完了を登録
                let _ = state
                    .registry
                    .mark_model_loaded(task.node_id, &task.model_name)
                    .await;
            }
        }
    }

    // 完了時に特別なログを出力
    if request.progress >= 1.0 {
        tracing::info!("Task completed: task_id={}", task_id);
    } else if request.progress == 0.0 {
        tracing::info!("Task started: task_id={}", task_id);
    }

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_model_name_valid() {
        assert!(validate_model_name("gpt-oss").is_ok());
        assert!(validate_model_name("gpt-oss:7b").is_ok());
        assert!(validate_model_name("llama3.2:latest").is_ok());
        assert!(validate_model_name("model_name:v1.0").is_ok());
    }

    #[test]
    fn test_validate_model_name_empty() {
        assert!(validate_model_name("").is_err());
    }

    #[test]
    fn test_validate_model_name_too_many_colons() {
        assert!(validate_model_name("a:b:c").is_err());
    }

    #[test]
    fn test_validate_model_name_invalid_characters() {
        assert!(validate_model_name("Model Name").is_err());
        assert!(validate_model_name("model@name").is_err());
    }

    #[test]
    fn test_available_model_view_serialize() {
        let view = AvailableModelView {
            name: "gpt-oss:7b".to_string(),
            display_name: Some("GPT-OSS 7B".to_string()),
            description: Some("Test model".to_string()),
            tags: Some(vec!["7b".to_string()]),
            size_gb: Some(4.0),
            required_memory_gb: Some(6.0),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("gpt-oss:7b"));
        assert!(json.contains("GPT-OSS 7B"));
    }

    #[test]
    fn test_available_model_view_optional_fields_skipped() {
        let view = AvailableModelView {
            name: "test".to_string(),
            display_name: None,
            description: None,
            tags: None,
            size_gb: None,
            required_memory_gb: None,
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("display_name"));
        assert!(!json.contains("description"));
    }

    #[test]
    fn test_available_models_response_serialize() {
        let response = AvailableModelsResponse {
            models: vec![],
            source: "builtin".to_string(),
            cached: None,
            pagination: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("builtin"));
    }

    #[test]
    fn test_loaded_model_summary_serialize() {
        let summary = LoadedModelSummary {
            model_name: "test:7b".to_string(),
            total_nodes: 3,
            pending: 1,
            downloading: 1,
            completed: 1,
            failed: 0,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("test:7b"));
        assert!(json.contains("\"total_nodes\":3"));
    }

    #[test]
    fn test_distribute_models_request_deserialize() {
        let json = r#"{"model_name": "test:7b", "target": "all"}"#;
        let request: DistributeModelsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model_name, "test:7b");
        assert_eq!(request.target, "all");
        assert!(request.node_ids.is_empty());
    }

    #[test]
    fn test_distribute_models_request_with_node_ids() {
        let json = r#"{"model_name": "test:7b", "target": "specific", "node_ids": ["550e8400-e29b-41d4-a716-446655440000"]}"#;
        let request: DistributeModelsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.target, "specific");
        assert_eq!(request.node_ids.len(), 1);
    }

    #[test]
    fn test_distribute_models_response_serialize() {
        let task_id = Uuid::new_v4();
        let response = DistributeModelsResponse {
            task_ids: vec![task_id],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(&task_id.to_string()));
    }

    #[test]
    fn test_pull_model_request_deserialize() {
        let json = r#"{"model_name": "gpt-oss:20b"}"#;
        let request: PullModelRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model_name, "gpt-oss:20b");
    }

    #[test]
    fn test_pull_model_response_serialize() {
        let task_id = Uuid::new_v4();
        let response = PullModelResponse { task_id };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(&task_id.to_string()));
    }

    #[test]
    fn test_update_progress_request_deserialize() {
        let json = r#"{"progress": 0.5, "speed": 1024}"#;
        let request: UpdateProgressRequest = serde_json::from_str(json).unwrap();
        assert!((request.progress - 0.5).abs() < f32::EPSILON);
        assert_eq!(request.speed, Some(1024));
    }

    #[test]
    fn test_update_progress_request_without_speed() {
        let json = r#"{"progress": 0.75}"#;
        let request: UpdateProgressRequest = serde_json::from_str(json).unwrap();
        assert!((request.progress - 0.75).abs() < f32::EPSILON);
        assert!(request.speed.is_none());
    }

    #[test]
    fn test_model_info_to_view_conversion() {
        let mut model = crate::registry::models::ModelInfo::new(
            "gpt-oss:7b".to_string(),
            4 * 1024 * 1024 * 1024,
            "Test model".to_string(),
            6 * 1024 * 1024 * 1024,
            vec!["7b".to_string()],
        );
        model.download_url = None;
        model.repo = None;
        model.filename = None;
        model.last_modified = None;
        model.status = None;
        let view = model_info_to_view(model);
        assert_eq!(view.name, "gpt-oss:7b");
        assert_eq!(view.display_name, Some("GPT-OSS 7B".to_string()));
        assert!((view.size_gb.unwrap() - 4.0).abs() < 0.001);
        assert!((view.required_memory_gb.unwrap() - 6.0).abs() < 0.001);
    }
}
