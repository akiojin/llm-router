//! モデル同期モジュール
//!
//! エンドポイントからモデル一覧を取得し、DBと同期

pub mod capabilities;
pub mod parser;

pub use capabilities::{
    capabilities_to_strings, capability_from_str, detect_capabilities, push_unique_api,
    push_unique_capability, supported_apis_from_capabilities, Capability,
};
pub use parser::{parse_models_response, ParsedModel, ResponseFormat};

use crate::common::http::RequestBuilderBearerExt;
use crate::db::endpoints as db;
use crate::metadata;
use crate::types::endpoint::{EndpointModel, EndpointType, SupportedAPI};
use chrono::Utc;
use reqwest::Client;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::debug;
use uuid::Uuid;

/// 同期結果
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// 同期されたモデル
    pub models: Vec<EndpointModel>,
    /// 追加されたモデル数
    pub added: usize,
    /// 削除されたモデル数
    pub removed: usize,
    /// 更新されたモデル数（既存モデルの再確認）
    pub updated: usize,
    /// 検出されたレスポンス形式
    pub format: ResponseFormat,
}

/// 同期エラー
#[derive(Debug)]
pub enum SyncError {
    /// HTTP接続エラー
    ConnectionError(String),
    /// HTTPエラーレスポンス
    HttpError(u16, String),
    /// パースエラー
    ParseError(String),
    /// DBエラー
    DbError(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            SyncError::HttpError(status, msg) => write!(f, "HTTP {}: {}", status, msg),
            SyncError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            SyncError::DbError(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for SyncError {}

/// エンドポイントからモデル一覧を取得してDBと同期
///
/// # 処理フロー
/// 1. GET /v1/models でモデル一覧を取得
/// 2. OpenAI/Ollama形式をパース
/// 3. 既存モデルと比較（差分計算）
/// 4. DBを更新（削除→追加）
/// 5. capabilitiesを自動判定
/// 6. xLLM/Ollamaの場合はmax_tokensを取得
pub async fn sync_models(
    pool: &SqlitePool,
    client: &Client,
    endpoint_id: Uuid,
    base_url: &str,
    api_key: Option<&str>,
    timeout_secs: u64,
) -> Result<SyncResult, SyncError> {
    // エンドポイントタイプを指定せずに同期（max_tokens取得なし）
    sync_models_with_type(
        pool,
        client,
        endpoint_id,
        base_url,
        api_key,
        timeout_secs,
        None,
    )
    .await
}

/// エンドポイントからモデル一覧を取得してDBと同期（タイプ指定版）
///
/// # 処理フロー
/// 1. GET /v1/models でモデル一覧を取得
/// 2. OpenAI/Ollama形式をパース
/// 3. 既存モデルと比較（差分計算）
/// 4. DBを更新（削除→追加）
/// 5. capabilitiesを自動判定
/// 6. xLLM/Ollamaの場合はmax_tokensを取得（SPEC-e8e9326e）
pub async fn sync_models_with_type(
    pool: &SqlitePool,
    client: &Client,
    endpoint_id: Uuid,
    base_url: &str,
    api_key: Option<&str>,
    timeout_secs: u64,
    endpoint_type: Option<EndpointType>,
) -> Result<SyncResult, SyncError> {
    // 既存モデルを取得
    let existing_models: HashSet<String> = match db::list_endpoint_models(pool, endpoint_id).await {
        Ok(models) => models.into_iter().map(|m| m.model_id).collect(),
        Err(_) => HashSet::new(),
    };

    // GET /v1/models でモデル一覧を取得。
    // LM Studio は OpenAI 互換の /v1/models だけでは vision capability を返さないため、
    // LM Studio 固有の /api/v1/models を優先して supported_apis を同期する。
    // ただし互換実装や古い環境では /api/v1/models が無いことがあるため、その場合は
    // OpenAI 互換の /v1/models にフォールバックする。
    let url = if endpoint_type == Some(EndpointType::LmStudio) {
        format!("{}/api/v1/models", base_url.trim_end_matches('/'))
    } else {
        format!("{}/v1/models", base_url.trim_end_matches('/'))
    };

    let json = match fetch_models_json(client, &url, api_key, timeout_secs).await {
        Ok(json) => json,
        Err(SyncError::HttpError(404, body)) if endpoint_type == Some(EndpointType::LmStudio) => {
            debug!(
                endpoint_id = %endpoint_id,
                error = %body,
                "LM Studio /api/v1/models unavailable; falling back to /v1/models"
            );
            let fallback_url = format!("{}/v1/models", base_url.trim_end_matches('/'));
            fetch_models_json(client, &fallback_url, api_key, timeout_secs).await?
        }
        Err(err) => return Err(err),
    };

    // モデル一覧をパース
    let (parsed_models, format) = parse_models_response(&json);

    // 新しいモデルIDのセット
    let new_model_ids: HashSet<String> = parsed_models.iter().map(|m| m.id.clone()).collect();
    let parsed_by_id: HashMap<&str, &ParsedModel> = parsed_models
        .iter()
        .map(|model| (model.id.as_str(), model))
        .collect();

    // 差分を計算
    let added_ids: Vec<_> = new_model_ids.difference(&existing_models).collect();
    let removed_ids: Vec<_> = existing_models.difference(&new_model_ids).collect();
    let updated_ids: Vec<_> = new_model_ids.intersection(&existing_models).collect();

    let added = added_ids.len();
    let removed = removed_ids.len();
    let updated = updated_ids.len();

    // 削除されたモデルを削除
    for model_id in removed_ids {
        if let Err(e) = db::delete_endpoint_model(pool, endpoint_id, model_id).await {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                model_id = %model_id,
                error = %e,
                "Failed to delete endpoint model during sync"
            );
        }
    }

    // 新しいモデルを追加（capabilitiesを自動判定 + エンドポイントからの情報を使用）
    let now = Utc::now();
    let mut synced_models = Vec::new();

    for model_id in &added_ids {
        let model = build_synced_endpoint_model(
            endpoint_id,
            model_id,
            parsed_by_id.get(model_id.as_str()).copied(),
            endpoint_type,
            now,
        );

        if let Err(e) = db::add_endpoint_model(pool, &model).await {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                model_id = %model.model_id,
                error = %e,
                "Failed to add endpoint model during sync"
            );
        }
        synced_models.push(model);
    }

    // 既存モデルのlast_checkedを更新
    for model_id in &updated_ids {
        let model = build_synced_endpoint_model(
            endpoint_id,
            model_id,
            parsed_by_id.get(model_id.as_str()).copied(),
            endpoint_type,
            now,
        );

        if let Err(e) = db::update_endpoint_model(pool, &model).await {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                model_id = %model.model_id,
                error = %e,
                "Failed to update endpoint model during sync"
            );
        }
        synced_models.push(model);
    }

    // SPEC-e8e9326e: xLLM/Ollamaの場合はmax_tokensを取得
    if let Some(ep_type) = endpoint_type {
        if ep_type == EndpointType::Xllm
            || ep_type == EndpointType::Ollama
            || ep_type == EndpointType::LmStudio
        {
            // 非同期でmax_tokensを取得（同期をブロックしない）
            let models_to_update: Vec<_> =
                synced_models.iter().map(|m| m.model_id.clone()).collect();
            for model_id in models_to_update {
                match metadata::get_model_metadata(client, base_url, api_key, &ep_type, &model_id)
                    .await
                {
                    Ok(meta) => {
                        if let Some(model) = synced_models
                            .iter_mut()
                            .find(|model| model.model_id == model_id)
                        {
                            if apply_metadata_to_synced_model(model, &meta) {
                                if let Err(e) = db::update_endpoint_model(pool, model).await {
                                    debug!(
                                        endpoint_id = %endpoint_id,
                                        model_id = %model_id,
                                        error = %e,
                                        "Failed to update model metadata"
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            endpoint_id = %endpoint_id,
                            model_id = %model_id,
                            error = %e,
                            "Failed to fetch model metadata for max_tokens"
                        );
                    }
                }
            }
        }
    }

    Ok(SyncResult {
        models: synced_models,
        added,
        removed,
        updated,
        format,
    })
}

async fn fetch_models_json(
    client: &Client,
    url: &str,
    api_key: Option<&str>,
    timeout_secs: u64,
) -> Result<serde_json::Value, SyncError> {
    let mut request = client.get(url);
    request = request.bearer_opt(api_key);

    let response = request
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| SyncError::ConnectionError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(SyncError::HttpError(status, body));
    }

    response
        .json()
        .await
        .map_err(|e| SyncError::ParseError(e.to_string()))
}

/// sync 時の add / update 双方で使う EndpointModel を組み立てる（重複排除）。
fn build_synced_endpoint_model(
    endpoint_id: Uuid,
    model_id: &str,
    parsed: Option<&ParsedModel>,
    endpoint_type: Option<EndpointType>,
    now: chrono::DateTime<Utc>,
) -> EndpointModel {
    let (caps_vec, supported_apis) = build_endpoint_model_capability_view(model_id, parsed);
    // マッピングテーブルからcanonical_nameを解決
    let canonical_name = endpoint_type
        .and_then(|et| crate::models::mapping::resolve_canonical(model_id, &et))
        .map(|s| s.to_string());
    EndpointModel {
        endpoint_id,
        model_id: model_id.to_string(),
        capabilities: caps_vec,
        max_tokens: None,
        last_checked: Some(now),
        supported_apis,
        canonical_name,
    }
}

fn build_endpoint_model_capability_view(
    model_id: &str,
    parsed: Option<&ParsedModel>,
) -> (Option<Vec<String>>, Vec<SupportedAPI>) {
    let detected = detect_capabilities(model_id);
    let mut capabilities = capabilities_to_strings(&detected);

    if let Some(parsed) = parsed {
        if let Some(reported_capabilities) = parsed.capabilities.as_deref() {
            for capability in reported_capabilities {
                push_unique_capability(&mut capabilities, capability);
            }
        }
        for api in &parsed.supported_apis {
            if let Some(capability) = capabilities::capability_from_supported_api(*api) {
                push_unique_capability(&mut capabilities, capability);
            }
        }
    }

    capabilities.sort();
    let mut supported_apis = supported_apis_from_capabilities(&capabilities);
    if let Some(parsed) = parsed {
        for api in &parsed.supported_apis {
            push_unique_api(&mut supported_apis, *api);
        }
    }
    if supported_apis.is_empty() {
        supported_apis.push(SupportedAPI::ChatCompletions);
    }
    supported_apis.sort_by_key(|api| api.as_str());

    (Some(capabilities), supported_apis)
}

fn apply_metadata_to_synced_model(
    model: &mut EndpointModel,
    metadata: &metadata::ModelMetadata,
) -> bool {
    let mut changed = false;

    if let Some(context_length) = metadata.context_length {
        if model.max_tokens != Some(context_length) {
            model.max_tokens = Some(context_length);
            changed = true;
        }
    }

    if metadata.supports_vision == Some(true) {
        let capabilities = model.capabilities.get_or_insert_with(Vec::new);
        let before_capabilities = capabilities.len();
        push_unique_capability(capabilities, "image_input");
        capabilities.sort();
        changed |= capabilities.len() != before_capabilities;

        let before_supported_apis = model.supported_apis.len();
        push_unique_api(&mut model.supported_apis, SupportedAPI::ImageInput);
        model.supported_apis.sort_by_key(|api| api.as_str());
        changed |= model.supported_apis.len() != before_supported_apis;
    }

    changed
}

/// 2つのモデルセット間の差分を計算
///
/// # Returns
/// (追加されるモデル, 削除されるモデル, 更新されるモデル)
pub fn calculate_diff(
    existing: &HashSet<String>,
    new: &HashSet<String>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let added: Vec<String> = new.difference(existing).cloned().collect();
    let removed: Vec<String> = existing.difference(new).cloned().collect();
    let updated: Vec<String> = new.intersection(existing).cloned().collect();

    (added, removed, updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_diff_all_new() {
        let existing = HashSet::new();
        let new: HashSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();

        let (added, removed, updated) = calculate_diff(&existing, &new);
        assert_eq!(added.len(), 3);
        assert!(removed.is_empty());
        assert!(updated.is_empty());
    }

    #[test]
    fn test_calculate_diff_all_removed() {
        let existing: HashSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let new = HashSet::new();

        let (added, removed, updated) = calculate_diff(&existing, &new);
        assert!(added.is_empty());
        assert_eq!(removed.len(), 3);
        assert!(updated.is_empty());
    }

    #[test]
    fn test_calculate_diff_mixed() {
        let existing: HashSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let new: HashSet<String> = ["b", "c", "d"].into_iter().map(String::from).collect();

        let (added, removed, updated) = calculate_diff(&existing, &new);
        assert_eq!(added, vec!["d"]);
        assert_eq!(removed, vec!["a"]);
        assert_eq!(updated.len(), 2);
        assert!(updated.contains(&"b".to_string()));
        assert!(updated.contains(&"c".to_string()));
    }

    #[test]
    fn test_calculate_diff_no_change() {
        let existing: HashSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        let new = existing.clone();

        let (added, removed, updated) = calculate_diff(&existing, &new);
        assert!(added.is_empty());
        assert!(removed.is_empty());
        assert_eq!(updated.len(), 2);
    }

    #[test]
    fn test_build_endpoint_model_capability_view_preserves_reported_image_input() {
        let parsed = ParsedModel {
            id: "qwen/qwen3-vl-30b".to_string(),
            capabilities: Some(vec!["vision".to_string()]),
            supported_apis: vec![SupportedAPI::ImageInput],
        };

        let (capabilities, supported_apis) =
            build_endpoint_model_capability_view(&parsed.id, Some(&parsed));

        let capabilities = capabilities.expect("capabilities should be present");
        assert!(capabilities.contains(&"chat".to_string()));
        assert!(capabilities.contains(&"image_input".to_string()));
        assert!(supported_apis.contains(&SupportedAPI::ChatCompletions));
        assert!(supported_apis.contains(&SupportedAPI::ImageInput));
    }

    #[test]
    fn test_apply_metadata_to_synced_model_adds_lm_studio_vision_api() {
        let mut model = EndpointModel {
            endpoint_id: Uuid::nil(),
            model_id: "google/gemma-4-26b-a4b".to_string(),
            capabilities: Some(vec!["chat".to_string()]),
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        let metadata = metadata::ModelMetadata {
            model: "google/gemma-4-26b-a4b".to_string(),
            context_length: Some(262144),
            supports_vision: Some(true),
            ..Default::default()
        };

        let changed = apply_metadata_to_synced_model(&mut model, &metadata);

        assert!(changed);
        assert_eq!(model.max_tokens, Some(262144));
        assert!(model
            .capabilities
            .as_ref()
            .expect("capabilities")
            .contains(&"image_input".to_string()));
        assert!(model.supported_apis.contains(&SupportedAPI::ImageInput));
    }

    #[test]
    fn test_apply_metadata_to_synced_model_ignores_non_vision_metadata() {
        let mut model = EndpointModel {
            endpoint_id: Uuid::nil(),
            model_id: "openai/gpt-oss-20b".to_string(),
            capabilities: Some(vec!["chat".to_string()]),
            max_tokens: Some(131072),
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        let metadata = metadata::ModelMetadata {
            model: "openai/gpt-oss-20b".to_string(),
            context_length: Some(131072),
            supports_vision: Some(false),
            ..Default::default()
        };

        let changed = apply_metadata_to_synced_model(&mut model, &metadata);

        assert!(!changed);
        assert!(!model.supported_apis.contains(&SupportedAPI::ImageInput));
        assert!(!model
            .capabilities
            .as_ref()
            .expect("capabilities")
            .contains(&"image_input".to_string()));
    }

    #[test]
    fn test_sync_error_display() {
        let err = SyncError::ConnectionError("timeout".to_string());
        assert!(err.to_string().contains("timeout"));

        let err = SyncError::HttpError(404, "Not found".to_string());
        assert!(err.to_string().contains("404"));

        let err = SyncError::ParseError("invalid json".to_string());
        assert!(err.to_string().contains("Parse error"));

        let err = SyncError::DbError("constraint violation".to_string());
        assert!(err.to_string().contains("Database error"));
    }
}
