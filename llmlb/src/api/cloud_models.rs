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

/// クラウドプロバイダのモデル一覧を共通処理で取得・パースする。
///
/// arch-review [L9]: 各 `fetch_*_models` が「タイムアウト付き送信 → ステータス
/// 判定 → JSON パース → 失敗時に warn して空返し」を逐語重複していたため集約した。
/// URL と認証ヘッダはプロバイダ側で `request` に設定し、応答型 `T` とパース関数
/// `parse` のみを差し替える。挙動（警告メッセージ含む）は従来と同一。
async fn fetch_and_parse_models<T, F>(
    provider: &str,
    request: reqwest::RequestBuilder,
    parse: F,
) -> Vec<CloudModelInfo>
where
    T: serde::de::DeserializeOwned,
    F: FnOnce(&T) -> Vec<CloudModelInfo>,
{
    let result = request
        .timeout(std::time::Duration::from_secs(
            CLOUD_MODELS_FETCH_TIMEOUT_SECS,
        ))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => match resp.json::<T>().await {
            Ok(data) => parse(&data),
            Err(e) => {
                tracing::warn!("Failed to parse {} models response: {}", provider, e);
                Vec::new()
            }
        },
        Ok(resp) => {
            tracing::warn!("{} models API returned status: {}", provider, resp.status());
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Failed to fetch {} models: {}", provider, e);
            Vec::new()
        }
    }
}

/// OpenAIからモデル一覧を取得
pub async fn fetch_openai_models(client: &reqwest::Client) -> Vec<CloudModelInfo> {
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            tracing::debug!("OPENAI_API_KEY not set, skipping OpenAI models");
            return Vec::new();
        }
    };

    let request = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", api_key));
    fetch_and_parse_models("OpenAI", request, parse_openai_models).await
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

    let request = client.get(&url);
    fetch_and_parse_models("Google", request, parse_google_models).await
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

    let request = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01");
    fetch_and_parse_models("Anthropic", request, parse_anthropic_models).await
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
mod tests;
