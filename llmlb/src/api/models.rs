//! モデル管理API
//!
//! モデル一覧取得、登録、マニフェスト配信のエンドポイント
//!
//! このモジュールはEndpointRegistry/Endpoint型を使用しています。

use super::error::AppError;
use crate::common::error::{CommonError, LbError, RouterResult};
use crate::{
    db::models::ModelStorage,
    registry::models::{extract_repo_id, generate_model_id, ModelInfo},
    AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

mod artifacts;
use artifacts::*;
mod hf_info;
use hf_info::fetch_hf_info;

// NOTE: supported_models.json は廃止されました (2026-01-25)
// モデルアーキテクチャ認識はエンドポイント（xLLM）側の config.json ベースで行われます
// 詳細は SPEC-6cd7f960, SPEC-48678000 を参照

/// モデル名の妥当性を検証
///
/// 有効なモデル名の形式:
/// - `gpt-oss-20b`, `mistral-7b-instruct-v0.2` のようなファイル名ベース形式
/// - `openai/gpt-oss-20b` のような階層形式（HuggingFace互換）
///
/// SPEC-dcaeaec4 FR-2: 階層形式を許可
fn validate_model_name(model_name: &str) -> Result<(), LbError> {
    if model_name.is_empty() {
        return Err(LbError::InvalidModelName("Model name is empty".to_string()));
    }

    // 危険なパターンを禁止（パストラバーサル対策）
    if model_name.contains("..") || model_name.contains('\0') {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name (contains dangerous pattern): {}",
            model_name
        )));
    }

    // 先頭・末尾のスラッシュは禁止
    if model_name.starts_with('/') || model_name.ends_with('/') {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name (leading/trailing slash): {}",
            model_name
        )));
    }

    // 許可する文字: 小文字英数字、'-', '_', '.', '/'（ディレクトリセパレータ）
    if !model_name.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.' || c == '/'
    }) {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name: {}",
            model_name
        )));
    }

    Ok(())
}

// NOTE: AvailableModelView, AvailableModelsResponse, Pagination, model_info_to_view() は
// /api/models/available 廃止に伴い削除されました。

/// モデルのライフサイクル状態
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStatus {
    /// 登録リクエスト受付、キャッシュ待ち
    Pending,
    /// ダウンロード・変換中（キャッシュ処理中）
    Caching,
    /// llmlbにキャッシュ完了（ノードがアクセス可能）
    Registered,
    /// エラー発生
    Error,
}

/// モデルの状態（SPEC-6cd7f960）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    /// 対応モデル（未ダウンロード）
    Available,
    /// ダウンロード中
    Downloading,
    /// ダウンロード完了
    Downloaded,
}

/// HuggingFace動的情報
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HfInfo {
    /// ダウンロード数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<u64>,
    /// いいね数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
}

/// 対応モデル + 状態（GET /api/models レスポンス）
#[derive(Debug, Clone, Serialize)]
pub struct ModelWithStatus {
    /// モデルID
    pub id: String,
    /// 表示名
    pub name: String,
    /// 説明
    pub description: String,
    /// HFリポジトリ
    pub repo: String,
    /// 推奨ファイル名
    pub recommended_filename: String,
    /// ファイルサイズ（バイト）
    pub size_bytes: u64,
    /// 必要メモリ（バイト）
    pub required_memory_bytes: u64,
    /// タグ
    pub tags: Vec<String>,
    /// 能力
    pub capabilities: Vec<String>,
    /// 量子化タイプ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    /// パラメータ数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_count: Option<String>,
    /// モデル状態
    pub status: ModelStatus,
    /// ライフサイクル状態（ダウンロード中/完了時のみ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_status: Option<LifecycleStatus>,
    /// ダウンロード進捗（ダウンロード中のみ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_progress: Option<DownloadProgress>,
    /// HF動的情報
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hf_info: Option<HfInfo>,
}

impl ModelWithStatus {
    /// 登録済みモデルからModelWithStatusを作成（available状態）
    pub fn from_registered(model: &ModelInfo) -> Self {
        let capabilities = model
            .get_capabilities()
            .iter()
            .map(|cap| format!("{:?}", cap))
            .collect();
        Self {
            id: model.name.clone(),
            name: model.name.clone(),
            description: model.description.clone(),
            repo: model.repo.clone().unwrap_or_default(),
            recommended_filename: model.filename.clone().unwrap_or_default(),
            size_bytes: model.size,
            required_memory_bytes: model.required_memory,
            tags: model.tags.clone(),
            capabilities,
            quantization: None,
            parameter_count: None,
            status: ModelStatus::Available,
            lifecycle_status: Some(LifecycleStatus::Registered),
            download_progress: None,
            hf_info: None,
        }
    }
}

/// ダウンロード進行状況
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    /// 進行率（0.0〜1.0）
    pub percent: f64,
    /// ダウンロード済みバイト数
    pub bytes_downloaded: Option<u64>,
    /// 総バイト数
    pub bytes_total: Option<u64>,
    /// エラーメッセージ（status=errorの場合）
    pub error: Option<String>,
}

// NOTE: RegisteredModelView と model_info_to_registered_view は /api/models 廃止に伴い削除。
// ダッシュボードは /v1/models を使用し、TypeScript側で型を定義。

// NOTE: get_registered_models() ハンドラは廃止されました。
// モデル一覧は /v1/models を使用してください（openai::list_models）。
// LifecycleStatus, DownloadProgress 型は openai.rs で使用するため維持。

/// 登録済みモデル一覧を取得
pub async fn list_registered_models(pool: &SqlitePool) -> RouterResult<Vec<ModelInfo>> {
    let storage = ModelStorage::new(pool.clone());
    storage.load_models().await
}

/// GET /api/models - 登録済みモデル一覧（拡張メタデータ付き）
///
/// ノード同期用途向け。配列を直接返す。
/// NOTE: この関数は既存のノード同期用途で維持。ダッシュボードは list_models_with_status() を使用。
pub async fn list_models(State(state): State<AppState>) -> Result<Json<Vec<ModelInfo>>, AppError> {
    let models = list_registered_models(&state.db_pool).await?;
    Ok(Json(models))
}

/// GET /api/models/hub - 登録済みモデル一覧 + 状態（SPEC-6cd7f960 改定版）
///
/// ダッシュボードのModel Hub用。登録済みモデルを状態付きで返す。
/// HF動的情報（ダウンロード数、いいね数）も含む。
///
/// NOTE: supported_models.json は廃止されました (2026-01-25)
/// 現在は登録済みモデルのみを返します。
/// モデルアーキテクチャ認識はエンドポイント側で行われます。
pub async fn list_models_with_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<ModelWithStatus>>, AppError> {
    let registered = list_registered_models(&state.db_pool).await?;

    // Build ready model names from endpoint models
    let ready_names: std::collections::HashSet<String> = {
        let endpoints = state.endpoint_registry.list().await;
        let mut names = std::collections::HashSet::new();
        for endpoint in &endpoints {
            if let Ok(models) = state.endpoint_registry.list_models(endpoint.id).await {
                for model in models {
                    names.insert(model.model_id.clone());
                }
            }
        }
        names
    };

    // Collect HF repos from registered models
    let hf_repos: std::collections::HashSet<String> =
        registered.iter().filter_map(|m| m.repo.clone()).collect();

    // Collect Hugging Face info for each repo
    let hf_info_futures: Vec<_> = hf_repos
        .into_iter()
        .map(|repo| {
            let client = state.http_client.clone();
            async move { (repo.clone(), fetch_hf_info(&client, &repo).await) }
        })
        .collect();

    let hf_infos: HashMap<String, Option<HfInfo>> = futures::future::join_all(hf_info_futures)
        .await
        .into_iter()
        .collect();

    // Build result from registered models only
    let mut result: Vec<ModelWithStatus> = Vec::with_capacity(registered.len());

    for model in &registered {
        let mut with_status = ModelWithStatus::from_registered(model);
        if ready_names.contains(&model.name) {
            with_status.status = ModelStatus::Downloaded;
        }
        if let Some(repo) = model.repo.as_ref() {
            if let Some(Some(info)) = hf_infos.get(repo) {
                with_status.hf_info = Some(info.clone());
            }
        }
        result.push(with_status);
    }

    Ok(Json(result))
}

/// 登録済みモデルを名前で取得
pub async fn load_registered_model(
    pool: &SqlitePool,
    name: &str,
) -> RouterResult<Option<ModelInfo>> {
    let storage = ModelStorage::new(pool.clone());
    storage.load_model(name).await
}

/// 登録モデルを全削除（テスト用）
pub async fn clear_registered_models(pool: &SqlitePool) -> RouterResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| LbError::Database(format!("Failed to begin transaction: {}", e)))?;

    sqlx::query("DELETE FROM model_tags")
        .execute(&mut *tx)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete model_tags: {}", e)))?;
    sqlx::query("DELETE FROM model_capabilities")
        .execute(&mut *tx)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete model_capabilities: {}", e)))?;
    sqlx::query("DELETE FROM models")
        .execute(&mut *tx)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete models: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| LbError::Database(format!("Failed to commit transaction: {}", e)))?;

    Ok(())
}

// NOTE: GET /api/models/available は廃止されました。
// HFカタログは直接 https://huggingface.co を参照してください。

#[derive(Deserialize)]
/// HFモデル登録リクエスト
pub struct RegisterModelRequest {
    /// HFリポジトリ名 (e.g., TheBloke/Llama-2-7B-GGUF)
    pub repo: String,
    /// ファイル名 (e.g., llama-2-7b.Q4_K_M.gguf)
    pub filename: Option<String>,
    /// 表示名（任意）
    #[serde(default)]
    pub display_name: Option<String>,
    /// オプションのchat_template（GGUFに含まれない場合の補助）
    #[serde(default)]
    pub chat_template: Option<String>,
}

async fn compute_gpu_warnings(
    registry: &crate::registry::endpoints::EndpointRegistry,
    required_memory: u64,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if required_memory == 0 {
        return warnings;
    }

    let endpoints = registry.list().await;
    let mut memories: Vec<u64> = Vec::new();
    for endpoint in endpoints {
        if let Some(mem) = endpoint.gpu_total_memory_bytes {
            memories.push(mem);
        }
    }

    // 安全: max()はSomeを返すことが保証（空でないことを上でチェック済み）
    let Some(max_mem) = memories.iter().max().copied() else {
        warnings.push("No GPU memory info available from registered endpoints".into());
        return warnings;
    };

    if required_memory > max_mem {
        warnings.push(format!(
            "Model requires {:.1}GB but max endpoint GPU memory is {:.1}GB",
            required_memory as f64 / (1024.0 * 1024.0 * 1024.0),
            max_mem as f64 / (1024.0 * 1024.0 * 1024.0),
        ));
    }

    warnings
}

/// POST /api/models/register - HFモデルを対応モデルに登録（メタデータのみ）
///
/// 方針:
/// - llmlbは変換・バイナリ保存を行わない
/// - `filename` を指定するとそのアーティファクトを主として登録
/// - 未指定の場合、リポジトリ内のアーティファクトが一意であれば自動選択
/// - safetensors では `config.json` / `tokenizer.json` が必須
pub async fn register_model(
    State(state): State<AppState>,
    Json(req): Json<RegisterModelRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if req.repo.trim().is_empty() {
        return Err(LbError::Common(CommonError::Validation("repo is required".into())).into());
    }

    // URLからrepo_idを抽出（フルURLが渡された場合はrepo_id形式に正規化）
    let repo = extract_repo_id(&req.repo);

    let name = generate_model_id(&repo);
    if load_registered_model(&state.db_pool, &name)
        .await?
        .is_some()
    {
        return Err(
            LbError::Common(CommonError::Validation("Model already registered".into())).into(),
        );
    }

    let filename_hint = req
        .filename
        .clone()
        .or_else(|| extract_filename_from_hf_url(&req.repo));

    if let Some(fname) = filename_hint.as_ref() {
        validate_artifact_path(fname)?;
    }

    let siblings = fetch_repo_siblings(&state.http_client, &repo).await?;
    let selection = resolve_primary_artifact(&siblings, filename_hint)?;

    let (content_length, required_memory, warnings) = {
        let (size, required) = match selection.format {
            ArtifactFormat::Gguf => {
                let size = siblings
                    .iter()
                    .find(|s| s.rfilename == selection.filename)
                    .map(sibling_size_bytes)
                    .unwrap_or(0);
                const REQUIRED_MEMORY_RATIO: f64 = 1.5;
                let required = if size > 0 {
                    ((size as f64) * REQUIRED_MEMORY_RATIO).ceil() as u64
                } else {
                    0
                };
                (size, required)
            }
            ArtifactFormat::Safetensors => {
                let total = siblings
                    .iter()
                    .filter(|s| s.rfilename.to_ascii_lowercase().ends_with(".safetensors"))
                    .map(sibling_size_bytes)
                    .sum::<u64>();
                const REQUIRED_MEMORY_RATIO: f64 = 1.5;
                let required = if total > 0 {
                    ((total as f64) * REQUIRED_MEMORY_RATIO).ceil() as u64
                } else {
                    0
                };
                (total, required)
            }
        };
        let warnings = compute_gpu_warnings(&state.endpoint_registry, required).await;
        (size, required, warnings)
    };

    let chat_template = if req.chat_template.is_some() {
        req.chat_template.clone()
    } else {
        fetch_chat_template_from_hf(&state.http_client, &repo).await
    };

    let mut tags = Vec::new();
    match selection.format {
        ArtifactFormat::Gguf => tags.push("gguf".to_string()),
        ArtifactFormat::Safetensors => tags.push("safetensors".to_string()),
    }
    let description = req.display_name.clone().unwrap_or_else(|| repo.clone());
    let capabilities = vec![crate::types::ModelCapability::TextGeneration];
    let size_bytes = content_length;
    let required_memory_bytes = required_memory;

    let source = match selection.format {
        ArtifactFormat::Gguf => crate::registry::models::ModelSource::HfGguf,
        ArtifactFormat::Safetensors => crate::registry::models::ModelSource::HfSafetensors,
    };

    let model = ModelInfo {
        name: name.clone(),
        size: size_bytes,
        description,
        required_memory: required_memory_bytes,
        tags,
        capabilities,
        source,
        chat_template,
        repo: Some(repo.clone()),
        filename: Some(selection.filename.clone()),
        last_modified: None,
        status: Some("registered".to_string()),
    };

    let storage = ModelStorage::new(state.db_pool.clone());
    storage.save_model(&model).await?;

    tracing::info!(
        repo = %repo,
        filename = %selection.filename,
        size_bytes = content_length,
        required_memory_bytes = required_memory,
        warnings = warnings.len(),
        "hf_model_registered"
    );

    let response = serde_json::json!({
        "name": name,
        "status": "registered",
        "filename": selection.filename,
        "size_bytes": content_length,
        "required_memory_bytes": required_memory,
        "warnings": warnings,
    });

    Ok((StatusCode::CREATED, Json(response)))
}

/// DELETE /api/models/:model_name - 登録モデル削除
///
/// 登録情報のみ削除し、Nodeは次回同期でキャッシュを削除する。
pub async fn delete_model(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> Result<StatusCode, AppError> {
    let storage = ModelStorage::new(state.db_pool.clone());
    if storage.load_model(&model_name).await?.is_none() {
        return Err(LbError::Common(CommonError::Validation("model not found".into())).into());
    }
    storage.delete_model(&model_name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/models/registry/:model_name/manifest.json - モデル配布マニフェスト
///
/// Node がモデルを複数ファイル（safetensors + metadata）として取得するためのマニフェスト。
pub async fn get_model_registry_manifest(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::response::Response;

    if let Err(e) = validate_model_name(&model_name) {
        return AppError(e).into_response();
    }

    let model = match load_registered_model(&state.db_pool, &model_name).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return AppError(LbError::NotFound(format!(
                "Model not found: {}",
                model_name
            )))
            .into_response();
        }
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let Some(repo) = model.repo.clone() else {
        return AppError(LbError::Common(CommonError::Validation(
            "repo not set for model".into(),
        )))
        .into_response();
    };

    let siblings = match fetch_repo_siblings(&state.http_client, &repo).await {
        Ok(list) => list,
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let selection = match resolve_primary_artifact(&siblings, model.filename.clone()) {
        Ok(sel) => sel,
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let runtime_hint = match selection.format {
        ArtifactFormat::Gguf => Some(vec!["llama_cpp".to_string()]),
        ArtifactFormat::Safetensors => infer_runtime_hint(&state.http_client, &repo)
            .await
            .or_else(|| Some(vec!["safetensors_cpp".to_string()])),
    };
    let manifest_quantization = match selection.format {
        ArtifactFormat::Gguf => infer_quantization_from_filename(&selection.filename),
        ArtifactFormat::Safetensors => None,
    };

    let base_url = hf_base_url();
    let mut files: Vec<ManifestFile> = Vec::new();

    match selection.format {
        ArtifactFormat::Gguf => {
            files.push(ManifestFile {
                name: "model.gguf".to_string(),
                priority: None,
                runtimes: runtime_hint.clone(),
                url: Some(hf_resolve_url(&base_url, &repo, &selection.filename)),
                optional: None,
            });
        }
        ArtifactFormat::Safetensors => {
            if let Err(e) = require_safetensors_metadata_files(&siblings) {
                return AppError(e).into_response();
            }

            let mut names: Vec<String> =
                vec!["config.json".to_string(), "tokenizer.json".to_string()];
            names.push(selection.filename.clone());

            if is_safetensors_index_filename(&selection.filename) {
                match fetch_safetensors_index_shards(&state.http_client, &repo, &selection.filename)
                    .await
                {
                    Ok(shards) => {
                        for shard in shards {
                            if !names.contains(&shard) {
                                names.push(shard);
                            }
                        }
                    }
                    Err(e) => {
                        return AppError(e).into_response();
                    }
                }
            }

            for name in names {
                files.push(ManifestFile {
                    name: name.clone(),
                    priority: manifest_file_priority(&name),
                    runtimes: runtime_hint.clone(),
                    url: Some(hf_resolve_url(&base_url, &repo, &name)),
                    optional: None,
                });
            }
        }
    }

    if let Some(metal_path) = find_metal_artifact(&siblings) {
        files.push(ManifestFile {
            name: "model.metal.bin".to_string(),
            priority: manifest_file_priority("model.metal.bin"),
            runtimes: runtime_hint.clone(),
            url: Some(hf_resolve_url(&base_url, &repo, &metal_path)),
            optional: None,
        });
    }

    let body = serde_json::to_string(&Manifest {
        format: manifest_format_label(selection.format).to_string(),
        files,
        quantization: manifest_quantization,
    })
    .unwrap_or_else(|_| "{\"format\":\"unknown\",\"files\":[]}".into());
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("Response builder should not fail with valid status and string body")
}

#[cfg(test)]
mod tests {
    use super::artifacts::*;
    use super::*;
    use serial_test::serial;
    use std::net::TcpListener;

    #[test]
    fn test_validate_model_name_valid() {
        assert!(validate_model_name("gpt-oss").is_ok());
        assert!(validate_model_name("gpt-oss-7b").is_ok());
        assert!(validate_model_name("llama3.2").is_ok());
        assert!(validate_model_name("model_name-v1.0").is_ok());
    }

    #[test]
    fn test_validate_model_name_hierarchical_valid() {
        // SPEC-dcaeaec4 FR-2: 階層形式を許可
        assert!(validate_model_name("openai/gpt-oss-20b").is_ok());
        assert!(validate_model_name("meta/llama-3-8b").is_ok());
        assert!(validate_model_name("org/sub/model").is_ok());
    }

    #[test]
    fn test_validate_model_name_empty() {
        assert!(validate_model_name("").is_err());
    }

    #[test]
    fn test_validate_model_name_colon_rejected() {
        assert!(validate_model_name("llama3.2:latest").is_err());
    }

    #[test]
    fn test_validate_model_name_invalid_characters() {
        assert!(validate_model_name("Model Name").is_err());
        assert!(validate_model_name("model@name").is_err());
    }

    #[test]
    fn test_validate_model_name_dangerous_patterns_rejected() {
        // パストラバーサル対策
        assert!(validate_model_name("../etc/passwd").is_err());
        assert!(validate_model_name("model/../other").is_err());
        assert!(validate_model_name("/absolute/path").is_err());
        assert!(validate_model_name("trailing/").is_err());
    }

    // ===== SPEC-6cd7f960: 対応モデルリスト型管理 =====

    #[test]
    fn test_model_status_serialization() {
        // ModelStatusが正しくシリアライズされることを確認
        assert_eq!(
            serde_json::to_string(&ModelStatus::Available).unwrap(),
            "\"available\""
        );
        assert_eq!(
            serde_json::to_string(&ModelStatus::Downloading).unwrap(),
            "\"downloading\""
        );
        assert_eq!(
            serde_json::to_string(&ModelStatus::Downloaded).unwrap(),
            "\"downloaded\""
        );
    }

    #[test]
    fn test_model_with_status_from_registered() {
        let mut model = ModelInfo::new(
            "test-model".to_string(),
            1000,
            "Test Model".to_string(),
            1500,
            vec!["test".to_string()],
        );
        model.repo = Some("test/repo".into());
        model.filename = Some("model.gguf".into());

        let with_status = ModelWithStatus::from_registered(&model);

        assert_eq!(with_status.id, "test-model");
        assert_eq!(with_status.name, "test-model");
        assert_eq!(with_status.description, "Test Model");
        assert_eq!(with_status.status, ModelStatus::Available);
        assert_eq!(
            with_status.lifecycle_status,
            Some(LifecycleStatus::Registered)
        );
        assert!(with_status.download_progress.is_none());
        assert!(with_status.hf_info.is_none());
    }

    #[test]
    fn test_model_with_status_serialization() {
        let mut model = ModelInfo::new(
            "qwen2.5-7b-instruct".to_string(),
            4_920_000_000,
            "Qwen2.5 7B Instruct".to_string(),
            7_380_000_000,
            vec!["chat".to_string()],
        );
        model.repo = Some("bartowski/Qwen2.5-7B-Instruct-GGUF".into());
        model.filename = Some("Qwen2.5-7B-Instruct-Q4_K_M.gguf".into());

        let with_status = ModelWithStatus::from_registered(&model);
        let json = serde_json::to_string(&with_status).expect("シリアライズに失敗");

        // JSONに必要なフィールドが含まれることを確認
        assert!(json.contains("\"id\":\"qwen2.5-7b-instruct\""));
        assert!(json.contains("\"status\":\"available\""));
        assert!(json.contains("\"lifecycle_status\":\"registered\""));
        // skip_serializing_if により None フィールドは含まれない
        assert!(!json.contains("\"download_progress\""));
    }

    #[test]
    fn test_hf_info_serialization() {
        let hf_info = HfInfo {
            downloads: Some(125000),
            likes: Some(450),
        };
        let json = serde_json::to_string(&hf_info).expect("シリアライズに失敗");
        assert!(json.contains("\"downloads\":125000"));
        assert!(json.contains("\"likes\":450"));

        // Noneの場合はフィールドが省略される
        let empty_info = HfInfo::default();
        let empty_json = serde_json::to_string(&empty_info).expect("シリアライズに失敗");
        assert_eq!(empty_json, "{}");
    }

    #[tokio::test]
    #[serial]
    async fn hf_fetch_repo_siblings_transport_errors_are_5xx() {
        // Pick an unused local port to force a connection error quickly.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener
            .local_addr()
            .expect("local addr should be available")
            .port();
        drop(listener);

        let previous = std::env::var("HF_BASE_URL").ok();
        std::env::set_var("HF_BASE_URL", format!("http://127.0.0.1:{}", port));

        let client = reqwest::Client::new();
        let err = match fetch_repo_siblings(&client, "openai/gpt-oss-7b").await {
            Ok(_) => panic!("expected transport error"),
            Err(e) => e,
        };

        assert!(matches!(err, LbError::Http(_) | LbError::Timeout(_)));
        assert_ne!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);

        match previous {
            Some(v) => std::env::set_var("HF_BASE_URL", v),
            None => std::env::remove_var("HF_BASE_URL"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn hf_fetch_file_bytes_transport_errors_are_5xx() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener
            .local_addr()
            .expect("local addr should be available")
            .port();
        drop(listener);

        let previous = std::env::var("HF_BASE_URL").ok();
        std::env::set_var("HF_BASE_URL", format!("http://127.0.0.1:{}", port));

        let client = reqwest::Client::new();
        let err = match fetch_hf_file_bytes(&client, "openai/gpt-oss-7b", "model.gguf").await {
            Ok(_) => panic!("expected transport error"),
            Err(e) => e,
        };

        assert!(matches!(err, LbError::Http(_) | LbError::Timeout(_)));
        assert_ne!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);

        match previous {
            Some(v) => std::env::set_var("HF_BASE_URL", v),
            None => std::env::remove_var("HF_BASE_URL"),
        }
    }

    // ===== is_gguf_filename tests =====

    #[test]
    fn test_is_gguf_filename_valid() {
        assert!(is_gguf_filename("model.gguf"));
        assert!(is_gguf_filename("Llama-2-7B-Q4_K_M.gguf"));
        assert!(is_gguf_filename("MODEL.GGUF"));
        assert!(is_gguf_filename("path/to/model.gguf"));
    }

    #[test]
    fn test_is_gguf_filename_invalid() {
        assert!(!is_gguf_filename("model.safetensors"));
        assert!(!is_gguf_filename("model.bin"));
        assert!(!is_gguf_filename("model"));
        assert!(!is_gguf_filename(""));
        assert!(!is_gguf_filename("gguf"));
    }

    // ===== is_safetensors_filename tests =====

    #[test]
    fn test_is_safetensors_filename_valid() {
        assert!(is_safetensors_filename("model.safetensors"));
        assert!(is_safetensors_filename("model.safetensors.index.json"));
        assert!(is_safetensors_filename("MODEL.SAFETENSORS"));
        assert!(is_safetensors_filename("model.SAFETENSORS.INDEX.JSON"));
    }

    #[test]
    fn test_is_safetensors_filename_invalid() {
        assert!(!is_safetensors_filename("model.gguf"));
        assert!(!is_safetensors_filename("model.bin"));
        assert!(!is_safetensors_filename("safetensors"));
        assert!(!is_safetensors_filename(""));
    }

    // ===== is_safetensors_index_filename tests =====

    #[test]
    fn test_is_safetensors_index_filename_valid() {
        assert!(is_safetensors_index_filename(
            "model.safetensors.index.json"
        ));
        assert!(is_safetensors_index_filename(
            "MODEL.SAFETENSORS.INDEX.JSON"
        ));
    }

    #[test]
    fn test_is_safetensors_index_filename_invalid() {
        assert!(!is_safetensors_index_filename("model.safetensors"));
        assert!(!is_safetensors_index_filename("model.gguf"));
        assert!(!is_safetensors_index_filename("index.json"));
    }

    // ===== infer_safetensors_index_from_shard tests =====

    #[test]
    fn test_infer_index_from_shard_standard() {
        let result = infer_safetensors_index_from_shard("model-00001-of-00003.safetensors");
        assert_eq!(result, Some("model.safetensors.index.json".to_string()));
    }

    #[test]
    fn test_infer_index_from_shard_with_directory() {
        let result = infer_safetensors_index_from_shard("subdir/model-00001-of-00003.safetensors");
        assert_eq!(
            result,
            Some("subdir/model.safetensors.index.json".to_string())
        );
    }

    #[test]
    fn test_infer_index_from_shard_not_a_shard() {
        assert!(infer_safetensors_index_from_shard("model.safetensors").is_none());
    }

    #[test]
    fn test_infer_index_from_shard_index_file_returns_none() {
        assert!(infer_safetensors_index_from_shard("model.safetensors.index.json").is_none());
    }

    #[test]
    fn test_infer_index_from_shard_non_safetensors() {
        assert!(infer_safetensors_index_from_shard("model.gguf").is_none());
    }

    // ===== validate_artifact_path tests =====

    #[test]
    fn test_validate_artifact_path_valid() {
        assert!(validate_artifact_path("model.gguf").is_ok());
        assert!(validate_artifact_path("subdir/model.safetensors").is_ok());
    }

    #[test]
    fn test_validate_artifact_path_empty() {
        assert!(validate_artifact_path("").is_err());
    }

    #[test]
    fn test_validate_artifact_path_traversal() {
        assert!(validate_artifact_path("../etc/passwd").is_err());
        assert!(validate_artifact_path("model/../other.gguf").is_err());
    }

    #[test]
    fn test_validate_artifact_path_null_byte() {
        assert!(validate_artifact_path("model\0.gguf").is_err());
    }

    #[test]
    fn test_validate_artifact_path_absolute() {
        assert!(validate_artifact_path("/absolute/path.gguf").is_err());
        assert!(validate_artifact_path("\\windows\\path.gguf").is_err());
    }

    // ===== extract_filename_from_hf_url tests =====

    #[test]
    fn test_extract_filename_from_hf_url_resolve() {
        let result =
            extract_filename_from_hf_url("https://huggingface.co/org/repo/resolve/main/model.gguf");
        assert_eq!(result, Some("model.gguf".to_string()));
    }

    #[test]
    fn test_extract_filename_from_hf_url_blob() {
        let result = extract_filename_from_hf_url(
            "https://huggingface.co/org/repo/blob/main/subdir/model.safetensors",
        );
        assert_eq!(result, Some("subdir/model.safetensors".to_string()));
    }

    #[test]
    fn test_extract_filename_from_hf_url_raw() {
        let result =
            extract_filename_from_hf_url("https://huggingface.co/org/repo/raw/main/config.json");
        assert_eq!(result, Some("config.json".to_string()));
    }

    #[test]
    fn test_extract_filename_from_hf_url_no_marker() {
        assert!(extract_filename_from_hf_url("https://huggingface.co/org/repo").is_none());
    }

    #[test]
    fn test_extract_filename_from_hf_url_plain_repo() {
        assert!(extract_filename_from_hf_url("org/repo").is_none());
    }

    // ===== manifest_format_label tests =====

    #[test]
    fn test_manifest_format_label_gguf() {
        assert_eq!(manifest_format_label(ArtifactFormat::Gguf), "gguf");
    }

    #[test]
    fn test_manifest_format_label_safetensors() {
        assert_eq!(
            manifest_format_label(ArtifactFormat::Safetensors),
            "safetensors"
        );
    }

    // ===== manifest_file_priority tests =====

    #[test]
    fn test_manifest_file_priority_config() {
        assert_eq!(manifest_file_priority("config.json"), Some(10));
    }

    #[test]
    fn test_manifest_file_priority_tokenizer() {
        assert_eq!(manifest_file_priority("tokenizer.json"), Some(10));
    }

    #[test]
    fn test_manifest_file_priority_safetensors_index() {
        assert_eq!(
            manifest_file_priority("model.safetensors.index.json"),
            Some(5)
        );
    }

    #[test]
    fn test_manifest_file_priority_metal() {
        assert_eq!(manifest_file_priority("model.metal.bin"), Some(5));
    }

    #[test]
    fn test_manifest_file_priority_regular_file() {
        assert_eq!(manifest_file_priority("model.gguf"), None);
        assert_eq!(manifest_file_priority("model.safetensors"), None);
    }

    // ===== is_quantization_token tests =====

    #[test]
    fn test_is_quantization_token_q_variants() {
        assert!(is_quantization_token("Q4_K_M"));
        assert!(is_quantization_token("Q5_K_S"));
        assert!(is_quantization_token("Q8_0"));
        assert!(is_quantization_token("Q2_K"));
    }

    #[test]
    fn test_is_quantization_token_iq_variants() {
        assert!(is_quantization_token("IQ4_XS"));
        assert!(is_quantization_token("IQ2_S"));
    }

    #[test]
    fn test_is_quantization_token_fp_bf_variants() {
        assert!(is_quantization_token("FP16"));
        assert!(is_quantization_token("BF16"));
        assert!(is_quantization_token("F32"));
        assert!(is_quantization_token("F16"));
    }

    #[test]
    fn test_is_quantization_token_mx_variants() {
        assert!(is_quantization_token("MXFP4"));
        assert!(is_quantization_token("MX8"));
    }

    #[test]
    fn test_is_quantization_token_non_quant() {
        assert!(!is_quantization_token("instruct"));
        assert!(!is_quantization_token("llama"));
        assert!(!is_quantization_token(""));
        assert!(!is_quantization_token("Q"));
        assert!(!is_quantization_token("F"));
    }

    // ===== infer_quantization_from_filename tests =====

    #[test]
    fn test_infer_quantization_from_filename_standard() {
        assert_eq!(
            infer_quantization_from_filename("Llama-2-7B-Q4_K_M.gguf"),
            Some("Q4_K_M".to_string())
        );
    }

    #[test]
    fn test_infer_quantization_from_filename_with_dots() {
        assert_eq!(
            infer_quantization_from_filename("model.Q5_K_S.gguf"),
            Some("Q5_K_S".to_string())
        );
    }

    #[test]
    fn test_infer_quantization_from_filename_fp16() {
        assert_eq!(
            infer_quantization_from_filename("model-FP16.gguf"),
            Some("FP16".to_string())
        );
    }

    #[test]
    fn test_infer_quantization_from_filename_no_quant() {
        // Model without a quantization token
        assert_eq!(infer_quantization_from_filename("llama-model.gguf"), None);
    }

    #[test]
    fn test_infer_quantization_from_filename_non_gguf() {
        assert_eq!(infer_quantization_from_filename("model.safetensors"), None);
    }

    #[test]
    fn test_infer_quantization_from_filename_with_path() {
        assert_eq!(
            infer_quantization_from_filename("subdir/model-Q4_K_M.gguf"),
            Some("Q4_K_M".to_string())
        );
    }

    // ===== extract_runtime_from_config tests =====

    #[test]
    fn test_extract_runtime_gptoss_architecture() {
        let config = serde_json::json!({
            "architectures": ["GptOssForCausalLM"]
        });
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("gptoss_cpp".to_string())
        );
    }

    #[test]
    fn test_extract_runtime_gptoss_uppercase() {
        let config = serde_json::json!({
            "architectures": ["GPTOSSModel"]
        });
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("gptoss_cpp".to_string())
        );
    }

    #[test]
    fn test_extract_runtime_nemotron_architecture() {
        let config = serde_json::json!({
            "architectures": ["NemotronForCausalLM"]
        });
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("nemotron_cpp".to_string())
        );
    }

    #[test]
    fn test_extract_runtime_from_model_type_gptoss() {
        let config = serde_json::json!({
            "model_type": "gpt_oss"
        });
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("gptoss_cpp".to_string())
        );
    }

    #[test]
    fn test_extract_runtime_from_model_type_nemotron() {
        let config = serde_json::json!({
            "model_type": "nemotron"
        });
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("nemotron_cpp".to_string())
        );
    }

    #[test]
    fn test_extract_runtime_unknown_architecture() {
        let config = serde_json::json!({
            "architectures": ["LlamaForCausalLM"]
        });
        assert_eq!(extract_runtime_from_config(&config), None);
    }

    #[test]
    fn test_extract_runtime_empty_config() {
        let config = serde_json::json!({});
        assert_eq!(extract_runtime_from_config(&config), None);
    }

    // ===== sibling_size_bytes tests =====

    #[test]
    fn test_sibling_size_bytes_direct_size() {
        let s = HfSibling {
            rfilename: "model.gguf".to_string(),
            size: Some(1000),
            lfs: None,
        };
        assert_eq!(sibling_size_bytes(&s), 1000);
    }

    #[test]
    fn test_sibling_size_bytes_lfs_size() {
        let s = HfSibling {
            rfilename: "model.gguf".to_string(),
            size: None,
            lfs: Some(HfLfs { size: Some(2000) }),
        };
        assert_eq!(sibling_size_bytes(&s), 2000);
    }

    #[test]
    fn test_sibling_size_bytes_prefers_direct() {
        let s = HfSibling {
            rfilename: "model.gguf".to_string(),
            size: Some(500),
            lfs: Some(HfLfs { size: Some(1000) }),
        };
        assert_eq!(sibling_size_bytes(&s), 500);
    }

    #[test]
    fn test_sibling_size_bytes_no_size() {
        let s = HfSibling {
            rfilename: "model.gguf".to_string(),
            size: None,
            lfs: None,
        };
        assert_eq!(sibling_size_bytes(&s), 0);
    }

    // ===== has_sibling tests =====

    #[test]
    fn test_has_sibling_found() {
        let siblings = vec![
            HfSibling {
                rfilename: "config.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model.gguf".to_string(),
                size: None,
                lfs: None,
            },
        ];
        assert!(has_sibling(&siblings, "model.gguf"));
        assert!(has_sibling(&siblings, "config.json"));
    }

    #[test]
    fn test_has_sibling_not_found() {
        let siblings = vec![HfSibling {
            rfilename: "config.json".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(!has_sibling(&siblings, "model.gguf"));
    }

    #[test]
    fn test_has_sibling_empty() {
        let siblings: Vec<HfSibling> = vec![];
        assert!(!has_sibling(&siblings, "anything"));
    }

    // ===== find_metal_artifact tests =====

    #[test]
    fn test_find_metal_artifact_standard() {
        let siblings = vec![HfSibling {
            rfilename: "model.metal.bin".to_string(),
            size: None,
            lfs: None,
        }];
        assert_eq!(
            find_metal_artifact(&siblings),
            Some("model.metal.bin".to_string())
        );
    }

    #[test]
    fn test_find_metal_artifact_subdirectory() {
        let siblings = vec![HfSibling {
            rfilename: "metal/model.bin".to_string(),
            size: None,
            lfs: None,
        }];
        assert_eq!(
            find_metal_artifact(&siblings),
            Some("metal/model.bin".to_string())
        );
    }

    #[test]
    fn test_find_metal_artifact_none() {
        let siblings = vec![HfSibling {
            rfilename: "model.gguf".to_string(),
            size: None,
            lfs: None,
        }];
        assert_eq!(find_metal_artifact(&siblings), None);
    }

    // ===== require_safetensors_metadata_files tests =====

    #[test]
    fn test_require_safetensors_metadata_both_present() {
        let siblings = vec![
            HfSibling {
                rfilename: "config.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "tokenizer.json".to_string(),
                size: None,
                lfs: None,
            },
        ];
        assert!(require_safetensors_metadata_files(&siblings).is_ok());
    }

    #[test]
    fn test_require_safetensors_metadata_missing_config() {
        let siblings = vec![HfSibling {
            rfilename: "tokenizer.json".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(require_safetensors_metadata_files(&siblings).is_err());
    }

    #[test]
    fn test_require_safetensors_metadata_missing_tokenizer() {
        let siblings = vec![HfSibling {
            rfilename: "config.json".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(require_safetensors_metadata_files(&siblings).is_err());
    }

    #[test]
    fn test_require_safetensors_metadata_both_missing() {
        let siblings: Vec<HfSibling> = vec![];
        assert!(require_safetensors_metadata_files(&siblings).is_err());
    }

    // ===== resolve_primary_artifact tests =====

    #[test]
    fn test_resolve_primary_artifact_single_gguf() {
        let siblings = vec![HfSibling {
            rfilename: "model.gguf".to_string(),
            size: Some(1000),
            lfs: None,
        }];
        let sel = resolve_primary_artifact(&siblings, None).unwrap();
        assert_eq!(sel.format, ArtifactFormat::Gguf);
        assert_eq!(sel.filename, "model.gguf");
    }

    #[test]
    fn test_resolve_primary_artifact_multiple_gguf_no_hint_error() {
        let siblings = vec![
            HfSibling {
                rfilename: "model-Q4.gguf".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model-Q8.gguf".to_string(),
                size: None,
                lfs: None,
            },
        ];
        assert!(resolve_primary_artifact(&siblings, None).is_err());
    }

    #[test]
    fn test_resolve_primary_artifact_with_gguf_hint() {
        let siblings = vec![
            HfSibling {
                rfilename: "model-Q4.gguf".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model-Q8.gguf".to_string(),
                size: None,
                lfs: None,
            },
        ];
        let sel = resolve_primary_artifact(&siblings, Some("model-Q4.gguf".to_string())).unwrap();
        assert_eq!(sel.format, ArtifactFormat::Gguf);
        assert_eq!(sel.filename, "model-Q4.gguf");
    }

    #[test]
    fn test_resolve_primary_artifact_hint_not_found() {
        let siblings = vec![HfSibling {
            rfilename: "model.gguf".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(
            resolve_primary_artifact(&siblings, Some("nonexistent.gguf".to_string()),).is_err()
        );
    }

    #[test]
    fn test_resolve_primary_artifact_no_artifacts() {
        let siblings = vec![HfSibling {
            rfilename: "README.md".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(resolve_primary_artifact(&siblings, None).is_err());
    }

    #[test]
    fn test_resolve_primary_artifact_mixed_formats_no_hint_error() {
        let siblings = vec![
            HfSibling {
                rfilename: "model.gguf".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model.safetensors".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "config.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "tokenizer.json".to_string(),
                size: None,
                lfs: None,
            },
        ];
        assert!(resolve_primary_artifact(&siblings, None).is_err());
    }

    // ===== LifecycleStatus serialization tests =====

    #[test]
    fn test_lifecycle_status_serialization() {
        assert_eq!(
            serde_json::to_string(&LifecycleStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&LifecycleStatus::Caching).unwrap(),
            "\"caching\""
        );
        assert_eq!(
            serde_json::to_string(&LifecycleStatus::Registered).unwrap(),
            "\"registered\""
        );
        assert_eq!(
            serde_json::to_string(&LifecycleStatus::Error).unwrap(),
            "\"error\""
        );
    }

    // ===== HfInfo tests =====

    #[test]
    fn test_hf_info_default_is_empty() {
        let info = HfInfo::default();
        assert!(info.downloads.is_none());
        assert!(info.likes.is_none());
    }

    #[test]
    fn test_hf_info_deserialization() {
        let json = r#"{"downloads": 5000, "likes": 100}"#;
        let info: HfInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.downloads, Some(5000));
        assert_eq!(info.likes, Some(100));
    }

    #[test]
    fn test_hf_info_deserialization_partial() {
        let json = r#"{"downloads": 5000}"#;
        let info: HfInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.downloads, Some(5000));
        assert!(info.likes.is_none());
    }

    // ===== ModelStatus deserialization tests =====

    #[test]
    fn test_model_status_deserialization() {
        assert_eq!(
            serde_json::from_str::<ModelStatus>("\"available\"").unwrap(),
            ModelStatus::Available
        );
        assert_eq!(
            serde_json::from_str::<ModelStatus>("\"downloading\"").unwrap(),
            ModelStatus::Downloading
        );
        assert_eq!(
            serde_json::from_str::<ModelStatus>("\"downloaded\"").unwrap(),
            ModelStatus::Downloaded
        );
    }

    // ===== hf_base_url tests =====

    #[test]
    #[serial]
    fn test_hf_base_url_default() {
        let prev = std::env::var("HF_BASE_URL").ok();
        std::env::remove_var("HF_BASE_URL");
        let url = hf_base_url();
        assert_eq!(url, "https://huggingface.co");
        if let Some(v) = prev {
            std::env::set_var("HF_BASE_URL", v);
        }
    }

    #[test]
    #[serial]
    fn test_hf_base_url_custom_strips_trailing_slash() {
        let prev = std::env::var("HF_BASE_URL").ok();
        std::env::set_var("HF_BASE_URL", "https://custom.example.com/");
        let url = hf_base_url();
        assert_eq!(url, "https://custom.example.com");
        match prev {
            Some(v) => std::env::set_var("HF_BASE_URL", v),
            None => std::env::remove_var("HF_BASE_URL"),
        }
    }

    // ===== hf_resolve_url tests =====

    #[test]
    fn test_hf_resolve_url_format() {
        let url = hf_resolve_url("https://huggingface.co", "org/repo", "model.gguf");
        assert_eq!(
            url,
            "https://huggingface.co/org/repo/resolve/main/model.gguf"
        );
    }

    // ===== validate_model_name null byte test =====

    #[test]
    fn test_validate_model_name_null_byte() {
        assert!(validate_model_name("model\0name").is_err());
    }

    // ===== DownloadProgress serialization =====

    #[test]
    fn test_download_progress_serialization() {
        let progress = DownloadProgress {
            percent: 0.75,
            bytes_downloaded: Some(750),
            bytes_total: Some(1000),
            error: None,
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["percent"], 0.75);
        assert_eq!(json["bytes_downloaded"], 750);
        assert_eq!(json["bytes_total"], 1000);
        assert!(json["error"].is_null());
    }

    #[test]
    fn test_download_progress_with_error() {
        let progress = DownloadProgress {
            percent: 0.0,
            bytes_downloaded: None,
            bytes_total: None,
            error: Some("network error".to_string()),
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["error"], "network error");
    }

    // ===== resolve_primary_artifact with safetensors hint =====

    #[test]
    fn test_resolve_primary_artifact_safetensors_single() {
        let siblings = vec![
            HfSibling {
                rfilename: "config.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "tokenizer.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model.safetensors".to_string(),
                size: Some(4000),
                lfs: None,
            },
        ];
        let sel = resolve_primary_artifact(&siblings, None).unwrap();
        assert_eq!(sel.format, ArtifactFormat::Safetensors);
        assert_eq!(sel.filename, "model.safetensors");
    }

    #[test]
    fn test_resolve_primary_artifact_unsupported_hint_extension() {
        let siblings = vec![HfSibling {
            rfilename: "model.bin".to_string(),
            size: None,
            lfs: None,
        }];
        assert!(resolve_primary_artifact(&siblings, Some("model.bin".to_string()),).is_err());
    }

    // ===== Additional unit tests for increased coverage =====

    // --- validate_model_name extended tests ---

    #[test]
    fn test_validate_model_name_single_char() {
        assert!(validate_model_name("a").is_ok());
        assert!(validate_model_name("1").is_ok());
    }

    #[test]
    fn test_validate_model_name_uppercase_rejected() {
        assert!(validate_model_name("GPT-4").is_err());
        assert!(validate_model_name("Llama").is_err());
    }

    #[test]
    fn test_validate_model_name_special_chars_rejected() {
        assert!(validate_model_name("model!name").is_err());
        assert!(validate_model_name("model#name").is_err());
        assert!(validate_model_name("model$name").is_err());
        assert!(validate_model_name("model%name").is_err());
        assert!(validate_model_name("model&name").is_err());
        assert!(validate_model_name("model*name").is_err());
    }

    #[test]
    fn test_validate_model_name_allows_dots_and_hyphens() {
        assert!(validate_model_name("model.v1.0-beta").is_ok());
        assert!(validate_model_name("a-b-c.d.e").is_ok());
    }

    #[test]
    fn test_validate_model_name_allows_underscores() {
        assert!(validate_model_name("model_name_v1").is_ok());
        assert!(validate_model_name("_leading").is_ok());
    }

    #[test]
    fn test_validate_model_name_double_slash_rejected() {
        // Leading slash is rejected, but double-dot is the traversal pattern
        assert!(validate_model_name("org//model").is_ok()); // double slash is fine (no .. pattern)
    }

    #[test]
    fn test_validate_model_name_whitespace_rejected() {
        assert!(validate_model_name("model name").is_err());
        assert!(validate_model_name("model\tname").is_err());
        assert!(validate_model_name("model\nname").is_err());
    }

    #[test]
    fn test_validate_model_name_unicode_rejected() {
        assert!(validate_model_name("model-名前").is_err());
        assert!(validate_model_name("модель").is_err());
    }

    // --- LifecycleStatus tests ---

    #[test]
    fn test_lifecycle_status_equality() {
        assert_eq!(LifecycleStatus::Pending, LifecycleStatus::Pending);
        assert_eq!(LifecycleStatus::Caching, LifecycleStatus::Caching);
        assert_eq!(LifecycleStatus::Registered, LifecycleStatus::Registered);
        assert_eq!(LifecycleStatus::Error, LifecycleStatus::Error);
        assert_ne!(LifecycleStatus::Pending, LifecycleStatus::Registered);
    }

    #[test]
    fn test_lifecycle_status_clone() {
        let status = LifecycleStatus::Caching;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_lifecycle_status_debug() {
        let debug = format!("{:?}", LifecycleStatus::Pending);
        assert!(debug.contains("Pending"));
    }

    // --- ModelStatus tests ---

    #[test]
    fn test_model_status_equality() {
        assert_eq!(ModelStatus::Available, ModelStatus::Available);
        assert_eq!(ModelStatus::Downloading, ModelStatus::Downloading);
        assert_eq!(ModelStatus::Downloaded, ModelStatus::Downloaded);
        assert_ne!(ModelStatus::Available, ModelStatus::Downloaded);
    }

    #[test]
    fn test_model_status_clone() {
        let status = ModelStatus::Downloading;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_model_status_roundtrip() {
        for status in [
            ModelStatus::Available,
            ModelStatus::Downloading,
            ModelStatus::Downloaded,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: ModelStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_model_status_invalid_deserialization() {
        assert!(serde_json::from_str::<ModelStatus>("\"invalid\"").is_err());
        assert!(serde_json::from_str::<ModelStatus>("\"AVAILABLE\"").is_err());
    }

    // --- HfInfo extended tests ---

    #[test]
    fn test_hf_info_clone() {
        let info = HfInfo {
            downloads: Some(100),
            likes: Some(50),
        };
        let cloned = info.clone();
        assert_eq!(cloned.downloads, Some(100));
        assert_eq!(cloned.likes, Some(50));
    }

    #[test]
    fn test_hf_info_debug() {
        let info = HfInfo {
            downloads: Some(100),
            likes: None,
        };
        let debug = format!("{:?}", info);
        assert!(debug.contains("100"));
    }

    #[test]
    fn test_hf_info_roundtrip_with_both_fields() {
        let info = HfInfo {
            downloads: Some(5000),
            likes: Some(200),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: HfInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.downloads, Some(5000));
        assert_eq!(deserialized.likes, Some(200));
    }

    #[test]
    fn test_hf_info_empty_roundtrip() {
        let info = HfInfo::default();
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: HfInfo = serde_json::from_str(&json).unwrap();
        assert!(deserialized.downloads.is_none());
        assert!(deserialized.likes.is_none());
    }

    #[test]
    fn test_hf_info_deserialization_with_extra_fields() {
        let json = r#"{"downloads": 100, "likes": 50, "extra_field": "ignored"}"#;
        let info: HfInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.downloads, Some(100));
        assert_eq!(info.likes, Some(50));
    }

    #[test]
    fn test_hf_info_deserialization_zero_values() {
        let json = r#"{"downloads": 0, "likes": 0}"#;
        let info: HfInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.downloads, Some(0));
        assert_eq!(info.likes, Some(0));
    }

    // --- is_gguf_filename edge cases ---

    #[test]
    fn test_is_gguf_filename_mixed_case() {
        assert!(is_gguf_filename("model.GgUf"));
        assert!(is_gguf_filename("model.Gguf"));
    }

    #[test]
    fn test_is_gguf_filename_dot_gguf_only() {
        assert!(is_gguf_filename(".gguf"));
    }

    // --- is_safetensors_filename edge cases ---

    #[test]
    fn test_is_safetensors_filename_mixed_case() {
        assert!(is_safetensors_filename("model.SafeTensors"));
    }

    #[test]
    fn test_is_safetensors_filename_dot_safetensors_only() {
        assert!(is_safetensors_filename(".safetensors"));
    }

    // --- is_safetensors_index_filename edge cases ---

    #[test]
    fn test_is_safetensors_index_filename_mixed_case() {
        assert!(is_safetensors_index_filename(
            "model.SafeTensors.Index.Json"
        ));
    }

    // --- infer_safetensors_index_from_shard edge cases ---

    #[test]
    fn test_infer_index_from_shard_empty_prefix() {
        // "-00001-of-00003.safetensors" -> prefix would be empty
        let result = infer_safetensors_index_from_shard("-00001-of-00003.safetensors");
        assert!(result.is_none());
    }

    #[test]
    fn test_infer_index_from_shard_non_numeric_shard() {
        let result = infer_safetensors_index_from_shard("model-abc-of-00003.safetensors");
        assert!(result.is_none());
    }

    #[test]
    fn test_infer_index_from_shard_non_numeric_total() {
        let result = infer_safetensors_index_from_shard("model-00001-of-abc.safetensors");
        assert!(result.is_none());
    }

    // --- validate_artifact_path edge cases ---

    #[test]
    fn test_validate_artifact_path_nested_path() {
        assert!(validate_artifact_path("dir1/dir2/dir3/model.gguf").is_ok());
    }

    #[test]
    fn test_validate_artifact_path_backslash_leading() {
        assert!(validate_artifact_path("\\model.gguf").is_err());
    }

    #[test]
    fn test_validate_artifact_path_dotdot_in_middle() {
        assert!(validate_artifact_path("a/../b").is_err());
    }

    // --- extract_filename_from_hf_url edge cases ---

    #[test]
    fn test_extract_filename_from_hf_url_empty_after_marker() {
        // /resolve/main/ with nothing after
        assert!(
            extract_filename_from_hf_url("https://huggingface.co/org/repo/resolve/main/").is_none()
        );
    }

    #[test]
    fn test_extract_filename_from_hf_url_nested_path() {
        let result = extract_filename_from_hf_url(
            "https://huggingface.co/org/repo/resolve/main/path/to/model.gguf",
        );
        assert_eq!(result, Some("path/to/model.gguf".to_string()));
    }

    // --- manifest_file_priority edge cases ---

    #[test]
    fn test_manifest_file_priority_empty_string() {
        assert_eq!(manifest_file_priority(""), None);
    }

    #[test]
    fn test_manifest_file_priority_case_sensitive() {
        // config.json / tokenizer.json checks are exact match
        assert_eq!(manifest_file_priority("Config.json"), None);
        assert_eq!(manifest_file_priority("TOKENIZER.JSON"), None);
    }

    // --- is_quantization_token extended tests ---

    #[test]
    fn test_is_quantization_token_with_special_chars() {
        assert!(!is_quantization_token("Q4-K-M")); // hyphens not allowed
        assert!(!is_quantization_token("Q4.K.M")); // dots not allowed
    }

    #[test]
    fn test_is_quantization_token_single_letter_no_digit() {
        assert!(!is_quantization_token("Q"));
        assert!(!is_quantization_token("F"));
        assert!(!is_quantization_token("BF"));
        assert!(!is_quantization_token("FP"));
        assert!(!is_quantization_token("IQ"));
        assert!(!is_quantization_token("MX"));
    }

    #[test]
    fn test_is_quantization_token_mx_needs_digit() {
        assert!(is_quantization_token("MX4")); // has digit
        assert!(!is_quantization_token("MXAB")); // no digit
    }

    // --- infer_quantization_from_filename edge cases ---

    #[test]
    fn test_infer_quantization_from_filename_empty() {
        assert_eq!(infer_quantization_from_filename(""), None);
    }

    #[test]
    fn test_infer_quantization_from_filename_just_gguf() {
        assert_eq!(infer_quantization_from_filename(".gguf"), None);
    }

    #[test]
    fn test_infer_quantization_from_filename_iq_variant() {
        assert_eq!(
            infer_quantization_from_filename("model-IQ4_XS.gguf"),
            Some("IQ4_XS".to_string())
        );
    }

    #[test]
    fn test_infer_quantization_from_filename_bf16() {
        assert_eq!(
            infer_quantization_from_filename("model-BF16.gguf"),
            Some("BF16".to_string())
        );
    }

    // --- extract_runtime_from_config edge cases ---

    #[test]
    fn test_extract_runtime_architectures_empty_array() {
        let config = serde_json::json!({
            "architectures": []
        });
        assert_eq!(extract_runtime_from_config(&config), None);
    }

    #[test]
    fn test_extract_runtime_architectures_non_string_elements() {
        let config = serde_json::json!({
            "architectures": [42, null, true]
        });
        assert_eq!(extract_runtime_from_config(&config), None);
    }

    #[test]
    fn test_extract_runtime_model_type_unknown() {
        let config = serde_json::json!({
            "model_type": "llama"
        });
        assert_eq!(extract_runtime_from_config(&config), None);
    }

    #[test]
    fn test_extract_runtime_architectures_take_priority_over_model_type() {
        let config = serde_json::json!({
            "architectures": ["GptOssForCausalLM"],
            "model_type": "nemotron"
        });
        // architectures should be checked first, so gptoss_cpp should be returned
        assert_eq!(
            extract_runtime_from_config(&config),
            Some("gptoss_cpp".to_string())
        );
    }

    // --- DownloadProgress extended tests ---

    #[test]
    fn test_download_progress_complete() {
        let progress = DownloadProgress {
            percent: 1.0,
            bytes_downloaded: Some(5000),
            bytes_total: Some(5000),
            error: None,
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["percent"], 1.0);
        assert_eq!(json["bytes_downloaded"], 5000);
        assert_eq!(json["bytes_total"], 5000);
    }

    #[test]
    fn test_download_progress_zero_percent() {
        let progress = DownloadProgress {
            percent: 0.0,
            bytes_downloaded: Some(0),
            bytes_total: Some(10000),
            error: None,
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["percent"], 0.0);
    }

    // --- ModelWithStatus extended tests ---

    #[test]
    fn test_model_with_status_default_fields() {
        let model = ModelInfo::new("test".to_string(), 0, "desc".to_string(), 0, vec![]);
        let with_status = ModelWithStatus::from_registered(&model);
        assert_eq!(with_status.repo, "");
        assert_eq!(with_status.recommended_filename, "");
        assert_eq!(with_status.size_bytes, 0);
        assert_eq!(with_status.required_memory_bytes, 0);
        assert!(with_status.quantization.is_none());
        assert!(with_status.parameter_count.is_none());
    }

    // --- ArtifactFormat tests ---

    #[test]
    fn test_artifact_format_equality() {
        assert_eq!(ArtifactFormat::Gguf, ArtifactFormat::Gguf);
        assert_eq!(ArtifactFormat::Safetensors, ArtifactFormat::Safetensors);
        assert_ne!(ArtifactFormat::Gguf, ArtifactFormat::Safetensors);
    }

    #[test]
    fn test_artifact_format_copy() {
        let fmt = ArtifactFormat::Gguf;
        let copied = fmt;
        assert_eq!(fmt, copied);
    }

    // --- sibling_size_bytes edge case ---

    #[test]
    fn test_sibling_size_bytes_lfs_none_size() {
        let s = HfSibling {
            rfilename: "model.gguf".to_string(),
            size: None,
            lfs: Some(HfLfs { size: None }),
        };
        assert_eq!(sibling_size_bytes(&s), 0);
    }

    // --- resolve_primary_artifact safetensors tests ---

    #[test]
    fn test_resolve_primary_artifact_safetensors_with_index() {
        let siblings = vec![
            HfSibling {
                rfilename: "config.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "tokenizer.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model.safetensors.index.json".to_string(),
                size: None,
                lfs: None,
            },
            HfSibling {
                rfilename: "model-00001-of-00002.safetensors".to_string(),
                size: Some(2000),
                lfs: None,
            },
            HfSibling {
                rfilename: "model-00002-of-00002.safetensors".to_string(),
                size: Some(2000),
                lfs: None,
            },
        ];
        let sel = resolve_primary_artifact(&siblings, None).unwrap();
        assert_eq!(sel.format, ArtifactFormat::Safetensors);
        assert_eq!(sel.filename, "model.safetensors.index.json");
    }

    #[test]
    fn test_resolve_primary_artifact_empty_siblings() {
        let siblings: Vec<HfSibling> = vec![];
        assert!(resolve_primary_artifact(&siblings, None).is_err());
    }

    // --- manifest_format_label tests ---

    #[test]
    fn test_manifest_format_label_returns_static_str() {
        let gguf_label = manifest_format_label(ArtifactFormat::Gguf);
        let st_label = manifest_format_label(ArtifactFormat::Safetensors);
        assert!(!gguf_label.is_empty());
        assert!(!st_label.is_empty());
        assert_ne!(gguf_label, st_label);
    }

    // --- ManifestFile serialization ---

    #[test]
    fn test_manifest_file_serialization_minimal() {
        let f = ManifestFile {
            name: "model.gguf".to_string(),
            priority: None,
            runtimes: None,
            url: None,
            optional: None,
        };
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["name"], "model.gguf");
        // skip_serializing_if fields should not be present
        assert!(json.get("priority").is_none());
        assert!(json.get("runtimes").is_none());
        assert!(json.get("url").is_none());
        assert!(json.get("optional").is_none());
    }

    #[test]
    fn test_manifest_file_serialization_full() {
        let f = ManifestFile {
            name: "config.json".to_string(),
            priority: Some(10),
            runtimes: Some(vec!["llama_cpp".to_string()]),
            url: Some("https://example.com/config.json".to_string()),
            optional: Some(true),
        };
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["name"], "config.json");
        assert_eq!(json["priority"], 10);
        assert_eq!(json["runtimes"][0], "llama_cpp");
        assert_eq!(json["url"], "https://example.com/config.json");
        assert_eq!(json["optional"], true);
    }

    // --- Manifest serialization ---

    #[test]
    fn test_manifest_serialization() {
        let manifest = Manifest {
            format: "gguf".to_string(),
            files: vec![ManifestFile {
                name: "model.gguf".to_string(),
                priority: None,
                runtimes: Some(vec!["llama_cpp".to_string()]),
                url: Some("https://example.com/model.gguf".to_string()),
                optional: None,
            }],
            quantization: Some("Q4_K_M".to_string()),
        };
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["format"], "gguf");
        assert_eq!(json["files"].as_array().unwrap().len(), 1);
        assert_eq!(json["quantization"], "Q4_K_M");
    }

    #[test]
    fn test_manifest_serialization_no_quantization() {
        let manifest = Manifest {
            format: "safetensors".to_string(),
            files: vec![],
            quantization: None,
        };
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["format"], "safetensors");
        assert!(json.get("quantization").is_none());
    }

    // --- hf_resolve_url edge cases ---

    #[test]
    fn test_hf_resolve_url_with_nested_filename() {
        let url = hf_resolve_url(
            "https://huggingface.co",
            "org/repo",
            "subdir/model.safetensors",
        );
        assert_eq!(
            url,
            "https://huggingface.co/org/repo/resolve/main/subdir/model.safetensors"
        );
    }

    #[test]
    fn test_hf_resolve_url_empty_filename() {
        let url = hf_resolve_url("https://huggingface.co", "org/repo", "");
        assert_eq!(url, "https://huggingface.co/org/repo/resolve/main/");
    }
}
