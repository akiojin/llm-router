//! カタログ検索API
//!
//! HuggingFace APIをラップし、GGUFモデルの検索・詳細取得を提供する。

use super::error::AppError;
use crate::common::error::LbError;
use crate::models::mapping::{resolve_engine_names, supports_canonical_on_endpoint};
use crate::types::endpoint::EndpointType;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

/// キャッシュTTL: 10分（秒）
const CATALOG_CACHE_TTL_SECS: i64 = 600;

/// HuggingFace API デフォルトベースURL
const DEFAULT_HF_BASE_URL: &str = "https://huggingface.co";

/// API呼び出しタイムアウト: 15秒
const HF_FETCH_TIMEOUT_SECS: u64 = 15;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// 検索クエリパラメータ
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// 検索クエリ文字列
    pub q: String,
    /// 取得件数上限（デフォルト: 20）
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

/// エンジン別モデル名
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineNames {
    /// Ollama用モデル名
    pub ollama: Option<String>,
    /// LM Studio用モデル名
    pub lm_studio: Option<String>,
    /// xLLM用モデル名
    pub xllm: Option<String>,
    /// vLLM用モデル名
    pub vllm: Option<String>,
}

/// カタログモデル情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogModel {
    /// HuggingFaceリポジトリID
    pub repo_id: String,
    /// モデルの説明
    #[serde(default)]
    pub description: Option<String>,
    /// ダウンロード数
    #[serde(default)]
    pub downloads: u64,
    /// タグ一覧
    #[serde(default)]
    pub tags: Vec<String>,
    /// エンジン別モデル名
    pub engine_names: EngineNames,
    /// ダウンロードをサポートするエンジン一覧
    pub supports_download: Vec<String>,
}

/// 検索結果レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// モデル一覧
    pub models: Vec<CatalogModel>,
}

/// HuggingFace APIのモデル情報（検索レスポンス）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelInfo {
    /// リポジトリID（例: "TheBloke/Llama-2-7B-GGUF"）
    #[serde(alias = "_id", alias = "id", alias = "modelId")]
    pub model_id: Option<String>,
    /// タグ一覧
    #[serde(default)]
    pub tags: Vec<String>,
    /// ダウンロード数
    #[serde(default)]
    pub downloads: u64,
    /// ファイル一覧（siblings）
    #[serde(default)]
    pub siblings: Vec<HfSibling>,
    /// 説明テキスト
    #[serde(default)]
    pub description: Option<String>,
    /// pipeline_tag (text-generation etc)
    #[serde(default)]
    pub pipeline_tag: Option<String>,
}

/// HuggingFaceリポジトリ内のファイル情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfSibling {
    /// ファイルパス（rfilename）
    pub rfilename: String,
}

/// モデル詳細レスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetailResponse {
    /// リポジトリID
    pub repo_id: String,
    /// タグ一覧
    pub tags: Vec<String>,
    /// ダウンロード数
    pub downloads: u64,
    /// 説明テキスト
    pub description: Option<String>,
    /// pipeline_tag
    pub pipeline_tag: Option<String>,
    /// ファイル一覧
    pub siblings: Vec<HfSibling>,
    /// エンジン別モデル名
    pub engine_names: EngineNames,
    /// ダウンロードをサポートするエンジン一覧
    pub supports_download: Vec<String>,
}

/// 推奨エンドポイント情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedEndpoint {
    /// エンドポイントID
    pub id: String,
    /// エンドポイント名
    pub name: String,
    /// エンドポイントタイプ
    pub endpoint_type: EndpointType,
    /// ダウンロード可能か
    pub can_download: bool,
    /// 既にこのモデルを持っているか
    pub has_model: bool,
}

/// 推奨エンドポイントレスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendEndpointsResponse {
    /// 推奨エンドポイント一覧
    pub endpoints: Vec<RecommendedEndpoint>,
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// 検索結果キャッシュエントリ
struct CacheEntry {
    /// キャッシュキー（query + limit）
    key: String,
    /// キャッシュされたレスポンス
    response: SearchResponse,
    /// 取得時刻
    fetched_at: DateTime<Utc>,
}

impl CacheEntry {
    fn is_valid(&self) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.fetched_at)
            .num_seconds();
        (0..CATALOG_CACHE_TTL_SECS).contains(&elapsed)
    }
}

/// グローバル検索キャッシュ（遅延初期化）
static SEARCH_CACHE: once_cell::sync::OnceCell<Arc<RwLock<Vec<CacheEntry>>>> =
    once_cell::sync::OnceCell::new();

fn get_search_cache() -> &'static Arc<RwLock<Vec<CacheEntry>>> {
    SEARCH_CACHE.get_or_init(|| Arc::new(RwLock::new(Vec::new())))
}

/// 検索キャッシュを明示的にクリアする（テスト分離・強制無効化フック）。
///
/// arch-review [L1]: モジュールグローバル static のためテスト間で状態が漏れていた。
/// クリアフックを提供して分離可能にする。
#[cfg(test)]
pub(crate) async fn invalidate_search_cache() {
    get_search_cache().write().await.clear();
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// HuggingFace APIのベースURLを取得
///
/// 末尾スラッシュを除去する（`HF_BASE_URL=http://host:1234/` 設定時に
/// `//api/models` の二重スラッシュを生まないため。api/models.rs の hf_base_url と挙動を揃える）。
fn hf_base_url() -> String {
    std::env::var("HF_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_HF_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// HuggingFace APIのAuthorizationヘッダー値を取得
fn hf_auth_header() -> Option<String> {
    std::env::var("HF_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .map(|t| format!("Bearer {}", t))
}

/// HfModelInfoからCatalogModelへ変換
pub fn to_catalog_model(hf: &HfModelInfo) -> CatalogModel {
    let repo_id = hf.model_id.clone().unwrap_or_default();
    let engine_names = build_engine_names(&repo_id);
    let supports_download = build_supports_download(&repo_id);

    CatalogModel {
        repo_id,
        description: hf.description.clone(),
        downloads: hf.downloads,
        tags: hf.tags.clone(),
        engine_names,
        supports_download,
    }
}

/// 正規名（repo_id）からエンジン別名を構築
pub fn build_engine_names(repo_id: &str) -> EngineNames {
    EngineNames {
        ollama: resolve_engine_model_ids(repo_id, EndpointType::Ollama)
            .into_iter()
            .next(),
        lm_studio: resolve_engine_model_ids(repo_id, EndpointType::LmStudio)
            .into_iter()
            .next(),
        xllm: resolve_engine_model_ids(repo_id, EndpointType::Xllm)
            .into_iter()
            .next(),
        vllm: resolve_engine_model_ids(repo_id, EndpointType::Vllm)
            .into_iter()
            .next(),
    }
}

/// ダウンロードをサポートするエンジン一覧を構築
pub fn build_supports_download(repo_id: &str) -> Vec<String> {
    let mut supported = Vec::new();

    if EndpointType::Xllm.supports_model_download() {
        supported.push(EndpointType::Xllm.as_str().to_string());
    }

    for endpoint_type in [EndpointType::Ollama, EndpointType::LmStudio] {
        if supports_canonical_on_endpoint(repo_id, &endpoint_type) {
            supported.push(endpoint_type.as_str().to_string());
        }
    }

    supported
}

fn can_recommend_download(endpoint_type: EndpointType, engine_name: Option<&str>) -> bool {
    if !endpoint_type.supports_model_download() {
        return false;
    }

    match endpoint_type {
        EndpointType::Ollama => engine_name.is_some(),
        EndpointType::LmStudio => true,
        EndpointType::Xllm => true,
        EndpointType::Llamacpp | EndpointType::Vllm | EndpointType::OpenaiCompatible => false,
    }
}

fn resolve_engine_model_ids(repo_id: &str, endpoint_type: EndpointType) -> Vec<String> {
    resolve_engine_names(repo_id, &endpoint_type)
        .into_iter()
        .map(|name| name.to_string())
        .collect()
}

fn endpoint_has_model(model_id: &str, repo_id: &str, engine_model_ids: &[String]) -> bool {
    model_id == repo_id || engine_model_ids.iter().any(|name| model_id == name)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/catalog/search?q=...&limit=...
///
/// HuggingFace APIを使ってGGUFモデルを検索する。
/// 結果は10分間キャッシュされる。
pub async fn search_catalog(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, AppError> {
    let cache_key = format!("{}:{}", params.q, params.limit);

    // キャッシュチェック
    {
        let cache = get_search_cache().read().await;
        if let Some(entry) = cache.iter().find(|e| e.key == cache_key) {
            if entry.is_valid() {
                return Ok(Json(entry.response.clone()));
            }
        }
    }

    // HuggingFace APIにリクエスト
    let base_url = hf_base_url();
    let url = format!("{}/api/models", base_url);

    let mut req = state
        .http_client
        .get(&url)
        .query(&[
            ("search", params.q.as_str()),
            ("limit", &params.limit.to_string()),
            ("filter", "gguf"),
        ])
        .timeout(std::time::Duration::from_secs(HF_FETCH_TIMEOUT_SECS));

    if let Some(auth) = hf_auth_header() {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await.map_err(|e| {
        warn!("HuggingFace API request failed: {}", e);
        AppError(LbError::Http(format!(
            "Failed to fetch from HuggingFace: {}",
            e
        )))
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        warn!("HuggingFace API returned status: {}", status);
        return Err(AppError(LbError::Http(format!(
            "HuggingFace API returned status: {}",
            status
        ))));
    }

    let hf_models: Vec<HfModelInfo> = resp.json().await.map_err(|e| {
        warn!("Failed to parse HuggingFace response: {}", e);
        AppError(LbError::Internal(format!(
            "Failed to parse HuggingFace response: {}",
            e
        )))
    })?;

    let models: Vec<CatalogModel> = hf_models.iter().map(to_catalog_model).collect();
    let response = SearchResponse { models };

    // キャッシュ更新
    {
        let mut cache = get_search_cache().write().await;
        // 古いエントリを削除
        cache.retain(|e| e.is_valid() && e.key != cache_key);
        cache.push(CacheEntry {
            key: cache_key,
            response: response.clone(),
            fetched_at: Utc::now(),
        });
    }

    Ok(Json(response))
}

/// GET /api/catalog/:repo_id
///
/// HuggingFace APIからモデル詳細情報を取得する。
/// repo_idはパスパラメータとして `owner/model` 形式で渡される。
pub async fn get_catalog_model(
    State(state): State<AppState>,
    Path(repo_id): Path<String>,
) -> Result<Json<ModelDetailResponse>, AppError> {
    let base_url = hf_base_url();
    let url = format!("{}/api/models/{}", base_url, repo_id);

    let mut req = state
        .http_client
        .get(&url)
        .timeout(std::time::Duration::from_secs(HF_FETCH_TIMEOUT_SECS));

    if let Some(auth) = hf_auth_header() {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await.map_err(|e| {
        warn!("HuggingFace model detail request failed: {}", e);
        AppError(LbError::Http(format!(
            "Failed to fetch model detail: {}",
            e
        )))
    })?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError(LbError::NotFound(format!(
            "Model not found: {}",
            repo_id
        ))));
    }

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(AppError(LbError::Http(format!(
            "HuggingFace API returned status: {}",
            status
        ))));
    }

    let hf_model: HfModelInfo = resp.json().await.map_err(|e| {
        warn!("Failed to parse HuggingFace model detail: {}", e);
        AppError(LbError::Internal(format!(
            "Failed to parse model detail: {}",
            e
        )))
    })?;

    let engine_names = build_engine_names(&repo_id);
    let supports_download = build_supports_download(&repo_id);

    let detail = ModelDetailResponse {
        repo_id,
        tags: hf_model.tags,
        downloads: hf_model.downloads,
        description: hf_model.description,
        pipeline_tag: hf_model.pipeline_tag,
        siblings: hf_model.siblings,
        engine_names,
        supports_download,
    };

    Ok(Json(detail))
}

/// GET /api/catalog/:repo_id/recommend-endpoints
///
/// 指定モデルのダウンロードに推奨されるオンラインエンドポイントを返す。
pub async fn recommend_endpoints(
    State(state): State<AppState>,
    Path(repo_id): Path<String>,
) -> Result<Json<RecommendEndpointsResponse>, AppError> {
    let online = state.endpoint_registry.list_online().await;

    let mut recommendations = Vec::new();

    for ep in &online {
        let engine_model_ids = resolve_engine_model_ids(&repo_id, ep.endpoint_type);
        // エンドポイントのモデル一覧からこのモデルを持っているか確認
        let engine_name = engine_model_ids.first().map(String::as_str);
        let can_download = can_recommend_download(ep.endpoint_type, engine_name);

        // モデル保有チェック: repo_id自体またはエンジン固有名で確認
        let models = crate::db::endpoints::list_endpoint_models(&state.db_pool, ep.id)
            .await
            .unwrap_or_default();
        let has_model = models
            .iter()
            .any(|m| endpoint_has_model(&m.model_id, &repo_id, &engine_model_ids));

        recommendations.push(RecommendedEndpoint {
            id: ep.id.to_string(),
            name: ep.name.clone(),
            endpoint_type: ep.endpoint_type,
            can_download,
            has_model,
        });
    }

    Ok(Json(RecommendEndpointsResponse {
        endpoints: recommendations,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
