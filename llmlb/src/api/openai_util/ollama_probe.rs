//! 要求された Ollama モデルが現在メモリ常駐かを確認するプローブ
//!
//! arch-review [H6]: api/openai_util.rs から Ollama /api/ps プローブを分離。

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct OllamaPsResponse {
    #[serde(default)]
    models: Vec<OllamaPsModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaPsModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

fn normalize_ollama_model_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn ollama_model_name_matches(candidate: &str, target: &str) -> bool {
    let candidate = normalize_ollama_model_name(candidate);
    let target = normalize_ollama_model_name(target);

    candidate == target
        || candidate.trim_end_matches(":latest") == target
        || candidate == target.trim_end_matches(":latest")
        || candidate.trim_end_matches(":latest") == target.trim_end_matches(":latest")
}

/// Returns whether the requested Ollama model is currently resident in memory.
///
/// `None` means the probe itself was inconclusive.
pub async fn probe_ollama_model_loaded(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Option<bool> {
    if model.trim().is_empty() {
        return None;
    }

    let url = format!("{}/api/ps", base_url.trim_end_matches('/'));
    let mut request = client.get(url).timeout(Duration::from_secs(3));
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    let ps: OllamaPsResponse = response.json().await.ok()?;
    Some(ps.models.into_iter().any(|entry| {
        ollama_model_name_matches(&entry.name, model)
            || ollama_model_name_matches(&entry.model, model)
    }))
}
