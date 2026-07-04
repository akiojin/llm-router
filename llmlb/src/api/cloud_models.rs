//! クラウドプロバイダーのモデル一覧取得・キャッシュ機能
//!
//! OpenAI/Google/Anthropic からモデル一覧を取得し、
//! TTL付きキャッシュで効率的に提供する。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// キャッシュTTL: 24時間（秒）
pub const CLOUD_MODELS_CACHE_TTL_SECS: u64 = 86400;

/// API呼び出しタイムアウト: 10秒
pub const CLOUD_MODELS_FETCH_TIMEOUT_SECS: u64 = 10;

/// クラウドモデル情報
///
/// OpenAI APIの `/v1/models` レスポンス形式に準拠
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelInfo {
    /// モデルID（プレフィックス付き: `openai:gpt-4o`）
    pub id: String,
    /// オブジェクトタイプ（固定: "model"）
    pub object: String,
    /// 作成日時（Unixタイムスタンプ）
    pub created: i64,
    /// プロバイダー名（openai, google, anthropic）
    pub owned_by: String,
}

/// クラウドモデルキャッシュ
#[derive(Debug, Clone)]
pub struct CloudModelsCache {
    /// キャッシュされたモデル一覧
    pub models: Vec<CloudModelInfo>,
    /// 取得時刻
    pub fetched_at: DateTime<Utc>,
}

impl CloudModelsCache {
    /// 新規キャッシュを作成
    pub fn new(models: Vec<CloudModelInfo>) -> Self {
        Self {
            models,
            fetched_at: Utc::now(),
        }
    }

    /// キャッシュが有効かどうかを判定
    pub fn is_valid(&self) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.fetched_at)
            .num_seconds();
        elapsed >= 0 && (elapsed as u64) < CLOUD_MODELS_CACHE_TTL_SECS
    }
}

/// グローバルキャッシュ（遅延初期化）
static CLOUD_MODELS_CACHE: once_cell::sync::OnceCell<Arc<RwLock<Option<CloudModelsCache>>>> =
    once_cell::sync::OnceCell::new();

/// キャッシュインスタンスを取得
fn get_cache() -> &'static Arc<RwLock<Option<CloudModelsCache>>> {
    CLOUD_MODELS_CACHE.get_or_init(|| Arc::new(RwLock::new(None)))
}

// ============================================================================
// プロバイダー固有レスポンス型
// ============================================================================

/// OpenAI モデル一覧レスポンス
#[derive(Debug, Deserialize)]
pub struct OpenAIModelsResponse {
    /// モデル一覧
    pub data: Vec<OpenAIModel>,
}

/// OpenAI 個別モデル
#[derive(Debug, Deserialize)]
pub struct OpenAIModel {
    /// モデルID
    pub id: String,
    /// オブジェクトタイプ
    pub object: String,
    /// 作成日時（Unixタイムスタンプ）
    pub created: i64,
    /// 所有者
    pub owned_by: String,
}

/// Google モデル一覧レスポンス
#[derive(Debug, Deserialize)]
pub struct GoogleModelsResponse {
    /// モデル一覧
    pub models: Vec<GoogleModel>,
}

/// Google 個別モデル
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleModel {
    /// `models/` プレフィックス付きの名前
    pub name: String,
    /// 表示名
    #[serde(default)]
    pub display_name: Option<String>,
}

/// Anthropic モデル一覧レスポンス
#[derive(Debug, Deserialize)]
pub struct AnthropicModelsResponse {
    /// モデル一覧
    pub data: Vec<AnthropicModel>,
}

/// Anthropic 個別モデル
#[derive(Debug, Deserialize)]
pub struct AnthropicModel {
    /// モデルID
    pub id: String,
    /// モデルタイプ
    #[serde(rename = "type")]
    pub model_type: String,
    /// 表示名
    #[serde(default)]
    pub display_name: Option<String>,
    /// ISO 8601形式の日時
    pub created_at: String,
}

// ============================================================================
// パース関数
// ============================================================================

/// OpenAIレスポンスをCloudModelInfoにパース
pub fn parse_openai_models(response: &OpenAIModelsResponse) -> Vec<CloudModelInfo> {
    response
        .data
        .iter()
        .map(|m| CloudModelInfo {
            id: format!("openai:{}", m.id),
            object: "model".to_string(),
            created: m.created,
            owned_by: "openai".to_string(),
        })
        .collect()
}

/// GoogleレスポンスをCloudModelInfoにパース
pub fn parse_google_models(response: &GoogleModelsResponse) -> Vec<CloudModelInfo> {
    response
        .models
        .iter()
        .map(|m| {
            // `models/` プレフィックスを除去
            let name = m.name.strip_prefix("models/").unwrap_or(&m.name);
            CloudModelInfo {
                id: format!("google:{}", name),
                object: "model".to_string(),
                created: 0, // Googleは作成日時を提供しない
                owned_by: "google".to_string(),
            }
        })
        .collect()
}

/// AnthropicレスポンスをCloudModelInfoにパース
pub fn parse_anthropic_models(response: &AnthropicModelsResponse) -> Vec<CloudModelInfo> {
    response
        .data
        .iter()
        .map(|m| {
            // ISO 8601 → Unixタイムスタンプ変換
            let created = chrono::DateTime::parse_from_rfc3339(&m.created_at)
                .map(|dt| dt.timestamp())
                .unwrap_or(0);
            CloudModelInfo {
                id: format!("anthropic:{}", m.id),
                object: "model".to_string(),
                created,
                owned_by: "anthropic".to_string(),
            }
        })
        .collect()
}

// ============================================================================
// フェッチ関数
// ============================================================================

/// OpenAIからモデル一覧を取得
pub async fn fetch_openai_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            tracing::debug!("OPENAI_API_KEY not set, skipping OpenAI models");
            return Vec::new();
        }
    };

    let result = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(
            CLOUD_MODELS_FETCH_TIMEOUT_SECS,
        ))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => match resp.json::<OpenAIModelsResponse>().await {
            Ok(data) => parse_openai_models(&data),
            Err(e) => {
                tracing::warn!("Failed to parse OpenAI models response: {}", e);
                Vec::new()
            }
        },
        Ok(resp) => {
            tracing::warn!("OpenAI models API returned status: {}", resp.status());
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Failed to fetch OpenAI models: {}", e);
            Vec::new()
        }
    }
}

/// Googleからモデル一覧を取得
pub async fn fetch_google_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let api_key = match std::env::var("GOOGLE_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            tracing::debug!("GOOGLE_API_KEY not set, skipping Google models");
            return Vec::new();
        }
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}",
        api_key
    );

    let result = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(
            CLOUD_MODELS_FETCH_TIMEOUT_SECS,
        ))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => match resp.json::<GoogleModelsResponse>().await {
            Ok(data) => parse_google_models(&data),
            Err(e) => {
                tracing::warn!("Failed to parse Google models response: {}", e);
                Vec::new()
            }
        },
        Ok(resp) => {
            tracing::warn!("Google models API returned status: {}", resp.status());
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Failed to fetch Google models: {}", e);
            Vec::new()
        }
    }
}

/// Anthropicからモデル一覧を取得
pub async fn fetch_anthropic_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            tracing::debug!("ANTHROPIC_API_KEY not set, skipping Anthropic models");
            return Vec::new();
        }
    };

    let result = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(
            CLOUD_MODELS_FETCH_TIMEOUT_SECS,
        ))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<AnthropicModelsResponse>().await {
                Ok(data) => parse_anthropic_models(&data),
                Err(e) => {
                    tracing::warn!("Failed to parse Anthropic models response: {}", e);
                    Vec::new()
                }
            }
        }
        Ok(resp) => {
            tracing::warn!("Anthropic models API returned status: {}", resp.status());
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Failed to fetch Anthropic models: {}", e);
            Vec::new()
        }
    }
}

/// 全プロバイダーからモデル一覧を並列取得
pub async fn fetch_all_cloud_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let (openai, google, anthropic) = tokio::join!(
        fetch_openai_models(client),
        fetch_google_models(client),
        fetch_anthropic_models(client),
    );

    let mut models = Vec::with_capacity(openai.len() + google.len() + anthropic.len());
    models.extend(openai);
    models.extend(google);
    models.extend(anthropic);
    models
}

// ============================================================================
// キャッシュ管理
// ============================================================================

/// キャッシュからモデル一覧を取得（必要に応じて更新）
/// クラウドモデルのキャッシュ更新方針（純粋・テスト可能）。
struct CloudModelsUpdate {
    /// 呼び出し元へ返すモデル一覧。
    returned: Vec<CloudModelInfo>,
    /// `Some` ならキャッシュへ保存するモデル一覧、`None` なら保存せず旧キャッシュを維持（stale）。
    to_store: Option<Vec<CloudModelInfo>>,
}

/// 取得結果と旧キャッシュから、返却値と保存値を決める。
///
/// - 取得成功（非空）: fetched を保存し返す。
/// - 取得失敗（空）で旧キャッシュあり: 旧モデルを返し保存しない（stale フォールバック）。
/// - 取得失敗（空）で旧キャッシュなし: 空を保存し返す。
fn resolve_cloud_models_update(
    old_models: Option<Vec<CloudModelInfo>>,
    fetched: Vec<CloudModelInfo>,
) -> CloudModelsUpdate {
    if fetched.is_empty() {
        if let Some(old) = old_models {
            return CloudModelsUpdate {
                returned: old,
                to_store: None,
            };
        }
    }
    CloudModelsUpdate {
        returned: fetched.clone(),
        to_store: Some(fetched),
    }
}

/// キャッシュ済みのクラウドモデル一覧を返す（TTL 内はキャッシュ、失効時は再取得）。
///
/// 再取得に失敗した場合は旧キャッシュへフォールバックする（`resolve_cloud_models_update`）。
pub async fn get_cached_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let cache = get_cache();

    // キャッシュが有効ならそのまま返却
    {
        let guard = cache.read().await;
        if let Some(ref cached) = *guard {
            if cached.is_valid() {
                return cached.models.clone();
            }
        }
    }

    // キャッシュ更新
    let models = fetch_all_cloud_models(client).await;

    // 「返す値」と「保存する値」を純粋関数で決める（取得失敗時の stale フォールバックを含む）。
    let mut guard = cache.write().await;
    let old_models = guard.as_ref().map(|c| c.models.clone());
    let update = resolve_cloud_models_update(old_models, models);
    match update.to_store {
        Some(to_store) => *guard = Some(CloudModelsCache::new(to_store)),
        None => tracing::info!("Cloud models fetch failed, using stale cache"),
    }
    update.returned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openai_models() {
        let response = OpenAIModelsResponse {
            data: vec![
                OpenAIModel {
                    id: "gpt-4o".to_string(),
                    object: "model".to_string(),
                    created: 1704067200,
                    owned_by: "openai".to_string(),
                },
                OpenAIModel {
                    id: "gpt-3.5-turbo".to_string(),
                    object: "model".to_string(),
                    created: 1677649963,
                    owned_by: "openai-internal".to_string(),
                },
            ],
        };

        let models = parse_openai_models(&response);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "openai:gpt-4o");
        assert_eq!(models[0].owned_by, "openai");
        assert_eq!(models[0].created, 1704067200);
        assert_eq!(models[1].id, "openai:gpt-3.5-turbo");
    }

    #[test]
    fn test_parse_google_models() {
        let response = GoogleModelsResponse {
            models: vec![
                GoogleModel {
                    name: "models/gemini-2.0-flash".to_string(),
                    display_name: Some("Gemini 2.0 Flash".to_string()),
                },
                GoogleModel {
                    name: "models/gemini-1.5-pro".to_string(),
                    display_name: None,
                },
            ],
        };

        let models = parse_google_models(&response);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "google:gemini-2.0-flash");
        assert_eq!(models[0].owned_by, "google");
        assert_eq!(models[1].id, "google:gemini-1.5-pro");
    }

    #[test]
    fn test_parse_anthropic_models() {
        let response = AnthropicModelsResponse {
            data: vec![AnthropicModel {
                id: "claude-sonnet-4-20250514".to_string(),
                model_type: "model".to_string(),
                display_name: Some("Claude Sonnet 4".to_string()),
                created_at: "2025-05-14T00:00:00Z".to_string(),
            }],
        };

        let models = parse_anthropic_models(&response);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "anthropic:claude-sonnet-4-20250514");
        assert_eq!(models[0].owned_by, "anthropic");
        // ISO 8601 → Unixタイムスタンプ変換を検証
        assert!(models[0].created > 0);
    }

    #[test]
    fn test_cache_is_valid() {
        let cache = CloudModelsCache::new(vec![]);
        assert!(cache.is_valid());
    }

    #[test]
    fn test_constants() {
        assert_eq!(CLOUD_MODELS_CACHE_TTL_SECS, 86400); // 24時間
        assert_eq!(CLOUD_MODELS_FETCH_TIMEOUT_SECS, 10); // 10秒
    }

    // --- additional coverage tests ---

    #[test]
    fn test_parse_openai_models_empty() {
        let response = OpenAIModelsResponse { data: vec![] };
        let models = parse_openai_models(&response);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_google_models_empty() {
        let response = GoogleModelsResponse { models: vec![] };
        let models = parse_google_models(&response);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_anthropic_models_empty() {
        let response = AnthropicModelsResponse { data: vec![] };
        let models = parse_anthropic_models(&response);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_google_models_without_prefix() {
        let response = GoogleModelsResponse {
            models: vec![GoogleModel {
                name: "gemini-flash".to_string(), // no "models/" prefix
                display_name: Some("Flash".to_string()),
            }],
        };
        let models = parse_google_models(&response);
        assert_eq!(models.len(), 1);
        // Without "models/" prefix, the name should be used as-is
        assert_eq!(models[0].id, "google:gemini-flash");
    }

    #[test]
    fn test_parse_anthropic_models_invalid_date() {
        let response = AnthropicModelsResponse {
            data: vec![AnthropicModel {
                id: "claude-test".to_string(),
                model_type: "model".to_string(),
                display_name: None,
                created_at: "not-a-date".to_string(), // invalid ISO 8601
            }],
        };
        let models = parse_anthropic_models(&response);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "anthropic:claude-test");
        // Invalid date falls back to 0
        assert_eq!(models[0].created, 0);
    }

    #[test]
    fn test_cloud_model_info_serialization() {
        let model = CloudModelInfo {
            id: "openai:gpt-4o".to_string(),
            object: "model".to_string(),
            created: 1704067200,
            owned_by: "openai".to_string(),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["id"], "openai:gpt-4o");
        assert_eq!(json["object"], "model");
        assert_eq!(json["created"], 1704067200);
        assert_eq!(json["owned_by"], "openai");
    }

    #[test]
    fn test_cloud_model_info_deserialization() {
        let json_str =
            r#"{"id":"google:gemini-pro","object":"model","created":0,"owned_by":"google"}"#;
        let model: CloudModelInfo = serde_json::from_str(json_str).unwrap();
        assert_eq!(model.id, "google:gemini-pro");
        assert_eq!(model.owned_by, "google");
    }

    #[test]
    fn test_cloud_models_cache_new() {
        let models = vec![
            CloudModelInfo {
                id: "openai:gpt-4o".to_string(),
                object: "model".to_string(),
                created: 1704067200,
                owned_by: "openai".to_string(),
            },
            CloudModelInfo {
                id: "google:gemini".to_string(),
                object: "model".to_string(),
                created: 0,
                owned_by: "google".to_string(),
            },
        ];
        let cache = CloudModelsCache::new(models.clone());
        assert_eq!(cache.models.len(), 2);
        assert!(cache.is_valid());
    }

    #[test]
    fn test_cloud_models_cache_expired() {
        let cache = CloudModelsCache {
            models: vec![],
            fetched_at: Utc::now()
                - chrono::Duration::seconds(CLOUD_MODELS_CACHE_TTL_SECS as i64 + 1),
        };
        assert!(!cache.is_valid());
    }

    #[test]
    fn test_cloud_models_cache_just_created_is_valid() {
        let cache = CloudModelsCache {
            models: vec![],
            fetched_at: Utc::now(),
        };
        assert!(cache.is_valid());
    }

    #[test]
    fn test_parse_openai_models_preserves_created_timestamp() {
        let response = OpenAIModelsResponse {
            data: vec![OpenAIModel {
                id: "model-a".to_string(),
                object: "model".to_string(),
                created: 1234567890,
                owned_by: "test-owner".to_string(),
            }],
        };
        let models = parse_openai_models(&response);
        assert_eq!(models[0].created, 1234567890);
        assert_eq!(models[0].object, "model");
    }

    #[test]
    fn test_parse_google_models_created_is_always_zero() {
        let response = GoogleModelsResponse {
            models: vec![GoogleModel {
                name: "models/gemini-2.0-flash".to_string(),
                display_name: Some("Flash".to_string()),
            }],
        };
        let models = parse_google_models(&response);
        // Google doesn't provide creation timestamp
        assert_eq!(models[0].created, 0);
    }

    #[test]
    fn test_parse_anthropic_models_valid_rfc3339() {
        let response = AnthropicModelsResponse {
            data: vec![AnthropicModel {
                id: "claude-3-opus".to_string(),
                model_type: "model".to_string(),
                display_name: Some("Claude 3 Opus".to_string()),
                created_at: "2024-02-29T12:00:00+00:00".to_string(),
            }],
        };
        let models = parse_anthropic_models(&response);
        assert_eq!(models[0].id, "anthropic:claude-3-opus");
        assert!(models[0].created > 0);
    }

    #[test]
    fn test_openai_model_deserialization() {
        let json_str =
            r#"{"id":"gpt-4o","object":"model","created":1704067200,"owned_by":"openai"}"#;
        let model: OpenAIModel = serde_json::from_str(json_str).unwrap();
        assert_eq!(model.id, "gpt-4o");
        assert_eq!(model.created, 1704067200);
    }

    #[test]
    fn test_google_model_deserialization() {
        let json_str = r#"{"name":"models/gemini-pro","displayName":"Gemini Pro"}"#;
        let model: GoogleModel = serde_json::from_str(json_str).unwrap();
        assert_eq!(model.name, "models/gemini-pro");
        assert_eq!(model.display_name, Some("Gemini Pro".to_string()));
    }

    #[test]
    fn test_google_model_deserialization_no_display_name() {
        let json_str = r#"{"name":"models/gemini-nano"}"#;
        let model: GoogleModel = serde_json::from_str(json_str).unwrap();
        assert_eq!(model.name, "models/gemini-nano");
        assert_eq!(model.display_name, None);
    }

    #[test]
    fn test_anthropic_model_deserialization() {
        let json_str = r#"{"id":"claude-3-haiku","type":"model","display_name":"Claude 3 Haiku","created_at":"2024-03-07T00:00:00Z"}"#;
        let model: AnthropicModel = serde_json::from_str(json_str).unwrap();
        assert_eq!(model.id, "claude-3-haiku");
        assert_eq!(model.model_type, "model");
        assert_eq!(model.display_name, Some("Claude 3 Haiku".to_string()));
    }

    #[test]
    fn test_openai_models_response_deserialization() {
        let json_str = r#"{"data":[{"id":"gpt-4o","object":"model","created":1704067200,"owned_by":"openai"},{"id":"gpt-3.5-turbo","object":"model","created":1677649963,"owned_by":"openai-internal"}]}"#;
        let response: OpenAIModelsResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.data.len(), 2);
    }

    #[test]
    fn test_google_models_response_deserialization() {
        let json_str = r#"{"models":[{"name":"models/gemini-pro"},{"name":"models/gemini-flash","displayName":"Gemini Flash"}]}"#;
        let response: GoogleModelsResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.models.len(), 2);
    }

    #[test]
    fn test_anthropic_models_response_deserialization() {
        let json_str = r#"{"data":[{"id":"claude-3-opus","type":"model","display_name":"Opus","created_at":"2024-02-29T00:00:00Z"}]}"#;
        let response: AnthropicModelsResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.data.len(), 1);
    }

    #[test]
    fn test_cloud_model_info_clone() {
        let model = CloudModelInfo {
            id: "openai:gpt-4o".to_string(),
            object: "model".to_string(),
            created: 100,
            owned_by: "openai".to_string(),
        };
        let cloned = model.clone();
        assert_eq!(cloned.id, model.id);
        assert_eq!(cloned.created, model.created);
    }

    #[test]
    fn test_parse_openai_models_owned_by_normalized() {
        let response = OpenAIModelsResponse {
            data: vec![OpenAIModel {
                id: "custom-model".to_string(),
                object: "model".to_string(),
                created: 0,
                owned_by: "user-org-abc123".to_string(), // original owned_by varies
            }],
        };
        let models = parse_openai_models(&response);
        // parse_openai_models normalizes owned_by to "openai"
        assert_eq!(models[0].owned_by, "openai");
    }

    // --- resolve_cloud_models_update tests (arch-review [M12]) ---

    fn sample_cloud_model(id: &str) -> super::CloudModelInfo {
        super::CloudModelInfo {
            id: id.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "openai".to_string(),
        }
    }

    #[test]
    fn resolve_cloud_models_update_stores_fetched_on_success() {
        let fetched = vec![sample_cloud_model("openai:gpt-4o")];
        let update = super::resolve_cloud_models_update(None, fetched);
        assert_eq!(update.returned.len(), 1);
        assert_eq!(update.returned[0].id, "openai:gpt-4o");
        let stored = update.to_store.expect("成功時は保存する");
        assert_eq!(stored[0].id, "openai:gpt-4o");
    }

    #[test]
    fn resolve_cloud_models_update_falls_back_to_stale_on_empty_fetch() {
        let old = vec![sample_cloud_model("openai:gpt-4o")];
        let update = super::resolve_cloud_models_update(Some(old), vec![]);
        assert_eq!(update.returned.len(), 1, "空取得時は旧キャッシュを返す");
        assert_eq!(update.returned[0].id, "openai:gpt-4o");
        assert!(
            update.to_store.is_none(),
            "stale フォールバック時は保存しない"
        );
    }

    #[test]
    fn resolve_cloud_models_update_stores_empty_when_no_old_cache() {
        let update = super::resolve_cloud_models_update(None, vec![]);
        assert!(update.returned.is_empty());
        assert!(
            update.to_store.map(|m| m.is_empty()).unwrap_or(false),
            "旧キャッシュ無しなら空を保存する"
        );
    }
}
