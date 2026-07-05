//! 登録モデルの一覧・ステータス取得とライフサイクル DTO。
//!
//! 登録済みモデルの列挙、HF 動的情報を統合したステータス取得、
//! ライフサイクル状態や進捗の DTO を提供する。

use super::hf_info::fetch_hf_info;
use crate::api::error::AppError;
use crate::common::error::{LbError, RouterResult};
use crate::db::models::ModelStorage;
use crate::registry::models::ModelInfo;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

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
