//! HuggingFace への HTTP 転送層。
//!
//! リポジトリの sibling 一覧・ファイル取得・safetensors index shard 解決・
//! chat_template 取得を担う。タイムアウト付きで外部依存を境界化する。

use super::HfSibling;
use crate::common::error::{CommonError, LbError};
use serde::Deserialize;
use std::time::Duration;

pub(crate) fn hf_base_url() -> String {
    std::env::var("HF_BASE_URL")
        .unwrap_or_else(|_| "https://huggingface.co".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub(crate) fn hf_resolve_url(base_url: &str, repo: &str, filename: &str) -> String {
    format!("{}/{}/resolve/main/{}", base_url, repo, filename)
}

// HuggingFace API is an external dependency. Keep requests bounded to avoid
// hanging /api/models/register and E2E workflows when HF is slow or unreachable.
const HF_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn fetch_repo_siblings(
    http_client: &reqwest::Client,
    repo: &str,
) -> Result<Vec<HfSibling>, LbError> {
    let base_url = hf_base_url();
    let url = format!("{}/api/models/{}?expand=siblings", base_url, repo);

    let mut req = http_client.get(&url);
    if let Ok(token) = std::env::var("HF_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.timeout(HF_HTTP_TIMEOUT).send().await.map_err(|e| {
        // Transport failures (timeouts/DNS/connect) are backend errors, not user input errors.
        // Return 5xx while keeping the message stable and non-leaky.
        let msg = "Failed to fetch specified repository".to_string();
        if e.is_timeout() {
            LbError::Timeout(msg)
        } else {
            LbError::Http(msg)
        }
    })?;
    if !resp.status().is_success() {
        return Err(LbError::Common(CommonError::Validation(
            "Failed to fetch specified repository".into(),
        )));
    }
    #[derive(Deserialize)]
    struct RepoDetail {
        siblings: Vec<HfSibling>,
    }
    let detail: RepoDetail = resp
        .json()
        .await
        .map_err(|e| LbError::Http(e.to_string()))?;
    Ok(detail.siblings)
}

pub(crate) async fn fetch_hf_file_bytes(
    http_client: &reqwest::Client,
    repo: &str,
    filename: &str,
) -> Result<Vec<u8>, LbError> {
    let base_url = hf_base_url();
    let url = hf_resolve_url(&base_url, repo, filename);
    let mut req = http_client.get(&url);
    if let Ok(token) = std::env::var("HF_TOKEN") {
        req = req.bearer_auth(token);
    }
    let resp = req.timeout(HF_HTTP_TIMEOUT).send().await.map_err(|e| {
        // Keep message stable, but treat transport failures as 5xx.
        let msg = format!("Failed to fetch file: {}", filename);
        if e.is_timeout() {
            LbError::Timeout(msg)
        } else {
            LbError::Http(msg)
        }
    })?;
    if !resp.status().is_success() {
        return Err(LbError::Common(CommonError::Validation(format!(
            "Failed to fetch file: {}",
            filename
        ))));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| LbError::Http(e.to_string()))?;
    Ok(bytes.to_vec())
}

pub(crate) async fn fetch_safetensors_index_shards(
    http_client: &reqwest::Client,
    repo: &str,
    index_filename: &str,
) -> Result<Vec<String>, LbError> {
    let bytes = fetch_hf_file_bytes(http_client, repo, index_filename).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| LbError::Http(e.to_string()))?;
    let Some(map) = value.get("weight_map").and_then(|v| v.as_object()) else {
        return Err(LbError::Common(CommonError::Validation(
            "Invalid safetensors index format".into(),
        )));
    };
    let mut shards: std::collections::HashSet<String> = std::collections::HashSet::new();
    for v in map.values() {
        if let Some(s) = v.as_str() {
            shards.insert(s.to_string());
        }
    }
    let mut list: Vec<String> = shards.into_iter().collect();
    list.sort();
    Ok(list)
}

/// HuggingFaceのtokenizer_config.jsonからchat_templateを取得
pub(crate) async fn fetch_chat_template_from_hf(
    http_client: &reqwest::Client,
    repo: &str,
) -> Option<String> {
    let base_url = std::env::var("HF_BASE_URL")
        .unwrap_or_else(|_| "https://huggingface.co".to_string())
        .trim_end_matches('/')
        .to_string();
    let url = format!("{}/{}/resolve/main/tokenizer_config.json", base_url, repo);

    let mut req = http_client.get(&url);
    if let Ok(token) = std::env::var("HF_TOKEN") {
        req = req.bearer_auth(token);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let template = json
                    .get("chat_template")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if template.is_some() {
                    tracing::info!(repo = %repo, "chat_template fetched from tokenizer_config.json");
                }
                template
            } else {
                tracing::debug!(repo = %repo, "Failed to parse tokenizer_config.json");
                None
            }
        }
        Ok(resp) => {
            tracing::debug!(repo = %repo, status = ?resp.status(), "tokenizer_config.json not found");
            None
        }
        Err(e) => {
            tracing::debug!(repo = %repo, error = %e, "Failed to fetch tokenizer_config.json");
            None
        }
    }
}
