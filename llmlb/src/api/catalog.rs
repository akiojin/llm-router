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
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_cache_invalidation_clears_entries() {
        // arch-review [L1]: クリアフックが検索キャッシュを確実に空にすることを検証。
        get_search_cache().write().await.push(CacheEntry {
            key: "query:10".to_string(),
            response: SearchResponse { models: Vec::new() },
            fetched_at: Utc::now(),
        });
        assert!(!get_search_cache().read().await.is_empty());
        invalidate_search_cache().await;
        assert!(get_search_cache().read().await.is_empty());
    }

    #[test]
    fn test_build_engine_names_known_model() {
        let names = build_engine_names("openai/gpt-oss-20b");
        assert_eq!(names.ollama, Some("gpt-oss:20b".to_string()));
        assert_eq!(names.lm_studio, Some("openai/gpt-oss-20b".to_string()));
        assert_eq!(names.xllm, None);
        assert_eq!(names.vllm, None);
    }

    #[test]
    fn test_build_engine_names_unknown_model() {
        let names = build_engine_names("unknown/model-123");
        assert_eq!(names.ollama, None);
        assert_eq!(names.lm_studio, None);
        assert_eq!(names.xllm, None);
        assert_eq!(names.vllm, None);
    }

    #[test]
    fn test_build_supports_download() {
        let download = build_supports_download("openai/gpt-oss-20b");
        assert!(download.contains(&"xllm".to_string()));
        assert!(download.contains(&"ollama".to_string()));
        assert!(download.contains(&"lm_studio".to_string()));
        assert!(!download.contains(&"openai_compatible".to_string()));
    }

    #[test]
    fn test_build_supports_download_includes_new_lm_studio_aliases() {
        let download = build_supports_download("google/gemma-3-27b-it");
        assert!(download.contains(&"xllm".to_string()));
        assert!(download.contains(&"ollama".to_string()));
        assert!(download.contains(&"lm_studio".to_string()));
    }

    #[test]
    fn test_build_supports_download_unknown_model_only_lists_xllm() {
        let download = build_supports_download("unknown/model-123");
        assert_eq!(download, vec!["xllm".to_string()]);
    }

    #[test]
    fn test_can_recommend_download_requires_ollama_alias() {
        assert!(can_recommend_download(
            EndpointType::Ollama,
            Some("gpt-oss:20b")
        ));
        assert!(!can_recommend_download(EndpointType::Ollama, None));
    }

    #[test]
    fn test_can_recommend_download_allows_xllm_without_alias() {
        assert!(can_recommend_download(EndpointType::Xllm, None));
    }

    #[test]
    fn test_can_recommend_download_allows_lm_studio_without_alias() {
        assert!(can_recommend_download(
            EndpointType::LmStudio,
            Some("openai/gpt-oss-20b")
        ));
        assert!(can_recommend_download(EndpointType::LmStudio, None));
    }

    #[test]
    fn test_resolve_engine_model_ids_includes_all_lm_studio_aliases() {
        let ids = resolve_engine_model_ids("Qwen/Qwen3.5-35B-A3B", EndpointType::LmStudio);
        assert_eq!(
            ids,
            vec![
                "qwen3.5-35b-a3b".to_string(),
                "qwen/qwen3.5-35b-a3b".to_string(),
                "qwen/qwen3.5-35b-a3b:2".to_string()
            ]
        );
    }

    #[test]
    fn test_endpoint_has_model_matches_secondary_alias() {
        let engine_model_ids =
            resolve_engine_model_ids("Qwen/Qwen3.5-35B-A3B", EndpointType::LmStudio);
        assert!(endpoint_has_model(
            "qwen/qwen3.5-35b-a3b:2",
            "Qwen/Qwen3.5-35B-A3B",
            &engine_model_ids
        ));
    }

    #[test]
    fn test_to_catalog_model() {
        let hf = HfModelInfo {
            model_id: Some("openai/gpt-oss-20b".to_string()),
            tags: vec!["text-generation".to_string(), "gguf".to_string()],
            downloads: 50000,
            siblings: vec![],
            description: Some("A test model".to_string()),
            pipeline_tag: Some("text-generation".to_string()),
        };

        let model = to_catalog_model(&hf);
        assert_eq!(model.repo_id, "openai/gpt-oss-20b");
        assert_eq!(model.downloads, 50000);
        assert_eq!(model.description, Some("A test model".to_string()));
        assert_eq!(model.tags.len(), 2);
        assert_eq!(model.engine_names.ollama, Some("gpt-oss:20b".to_string()));
        assert_eq!(
            model.engine_names.lm_studio,
            Some("openai/gpt-oss-20b".to_string())
        );
        assert_eq!(
            model.supports_download,
            vec![
                "xllm".to_string(),
                "ollama".to_string(),
                "lm_studio".to_string()
            ]
        );
    }

    #[test]
    fn test_to_catalog_model_missing_id() {
        let hf = HfModelInfo {
            model_id: None,
            tags: vec![],
            downloads: 0,
            siblings: vec![],
            description: None,
            pipeline_tag: None,
        };

        let model = to_catalog_model(&hf);
        assert_eq!(model.repo_id, "");
    }

    #[test]
    fn test_search_response_serialization() {
        let response = SearchResponse {
            models: vec![CatalogModel {
                repo_id: "test/model".to_string(),
                description: Some("desc".to_string()),
                downloads: 100,
                tags: vec!["gguf".to_string()],
                engine_names: EngineNames {
                    ollama: None,
                    lm_studio: None,
                    xllm: None,
                    vllm: None,
                },
                supports_download: vec!["xllm".to_string()],
            }],
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["models"][0]["repo_id"], "test/model");
        assert_eq!(json["models"][0]["downloads"], 100);
        assert_eq!(json["models"][0]["supports_download"][0], "xllm");
    }

    #[test]
    fn test_search_response_deserialization() {
        let json_str = r#"{"models":[{"repo_id":"test/model","description":"desc","downloads":100,"tags":["gguf"],"engine_names":{"ollama":null,"lm_studio":null,"xllm":null,"vllm":null},"supports_download":["xllm"]}]}"#;
        let response: SearchResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.models.len(), 1);
        assert_eq!(response.models[0].repo_id, "test/model");
    }

    #[test]
    fn test_hf_model_info_deserialization() {
        let json_str = r#"{
            "modelId": "TheBloke/Llama-2-7B-GGUF",
            "tags": ["text-generation", "gguf"],
            "downloads": 123456,
            "siblings": [{"rfilename": "llama-2-7b.Q4_K_M.gguf"}],
            "description": "Llama 2 7B GGUF",
            "pipeline_tag": "text-generation"
        }"#;
        let info: HfModelInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(info.model_id, Some("TheBloke/Llama-2-7B-GGUF".to_string()));
        assert_eq!(info.downloads, 123456);
        assert_eq!(info.siblings.len(), 1);
        assert_eq!(info.siblings[0].rfilename, "llama-2-7b.Q4_K_M.gguf");
    }

    #[test]
    fn test_hf_model_info_deserialization_minimal() {
        let json_str = r#"{"tags":[]}"#;
        let info: HfModelInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(info.model_id, None);
        assert_eq!(info.downloads, 0);
        assert!(info.siblings.is_empty());
    }

    #[test]
    fn test_engine_names_equality() {
        let a = EngineNames {
            ollama: Some("model:7b".to_string()),
            lm_studio: None,
            xllm: None,
            vllm: None,
        };
        let b = EngineNames {
            ollama: Some("model:7b".to_string()),
            lm_studio: None,
            xllm: None,
            vllm: None,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_model_detail_response_serialization() {
        let detail = ModelDetailResponse {
            repo_id: "test/model".to_string(),
            tags: vec!["gguf".to_string()],
            downloads: 999,
            description: Some("A model".to_string()),
            pipeline_tag: Some("text-generation".to_string()),
            siblings: vec![HfSibling {
                rfilename: "model.gguf".to_string(),
            }],
            engine_names: EngineNames {
                ollama: None,
                lm_studio: None,
                xllm: None,
                vllm: None,
            },
            supports_download: vec!["xllm".to_string()],
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["repo_id"], "test/model");
        assert_eq!(json["downloads"], 999);
        assert_eq!(json["siblings"][0]["rfilename"], "model.gguf");
    }

    #[test]
    fn test_recommended_endpoint_serialization() {
        let ep = RecommendedEndpoint {
            id: "123".to_string(),
            name: "My Endpoint".to_string(),
            endpoint_type: EndpointType::Ollama,
            can_download: true,
            has_model: false,
        };

        let json = serde_json::to_value(&ep).unwrap();
        assert_eq!(json["id"], "123");
        assert_eq!(json["name"], "My Endpoint");
        assert_eq!(json["endpoint_type"], "ollama");
        assert_eq!(json["can_download"], true);
        assert_eq!(json["has_model"], false);
    }

    #[test]
    fn test_search_query_defaults() {
        let json_str = r#"q=llama"#;
        let query: SearchQuery = serde_urlencoded::from_str(json_str).unwrap();
        assert_eq!(query.q, "llama");
        assert_eq!(query.limit, 20);
    }

    #[test]
    fn test_search_query_with_limit() {
        let json_str = r#"q=llama&limit=5"#;
        let query: SearchQuery = serde_urlencoded::from_str(json_str).unwrap();
        assert_eq!(query.q, "llama");
        assert_eq!(query.limit, 5);
    }

    #[test]
    fn test_cache_entry_validity() {
        let entry = CacheEntry {
            key: "test".to_string(),
            response: SearchResponse { models: vec![] },
            fetched_at: Utc::now(),
        };
        assert!(entry.is_valid());
    }

    #[test]
    fn test_cache_entry_expired() {
        let entry = CacheEntry {
            key: "test".to_string(),
            response: SearchResponse { models: vec![] },
            fetched_at: Utc::now() - chrono::Duration::seconds(CATALOG_CACHE_TTL_SECS + 1),
        };
        assert!(!entry.is_valid());
    }

    #[test]
    fn test_hf_base_url_default() {
        // HF_BASE_URL が未設定の場合はデフォルト値を返す
        // NOTE: テスト環境で HF_BASE_URL が設定されている場合はそちらが返る
        let url = hf_base_url();
        assert!(!url.is_empty());
    }

    #[test]
    fn test_build_engine_names_qwen3() {
        let names = build_engine_names("Qwen/Qwen3-30B");
        assert_eq!(names.ollama, Some("qwen3:30b".to_string()));
        assert_eq!(names.lm_studio, Some("qwen/qwen3-30b-a3b".to_string()));
    }

    #[test]
    fn test_build_engine_names_gemma3() {
        let names = build_engine_names("google/gemma-3-27b-it");
        assert_eq!(names.ollama, Some("gemma3:27b".to_string()));
        assert_eq!(names.lm_studio, Some("google/gemma-3-27b".to_string()));
    }
}
