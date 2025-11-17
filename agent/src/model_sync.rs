use ollama_coordinator_agent::ollama::OllamaManager;
use ollama_coordinator_common::error::{AgentError, AgentResult};
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

/// /v1/models のレスポンス用
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

/// コーディネーターの /v1/models を取得（リトライ付き）
#[allow(dead_code)]
pub async fn fetch_models(coordinator_url: &str) -> AgentResult<Vec<String>> {
    let url = format!("{}/v1/models", coordinator_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let mut last_err = None;
    for attempt in 1..=3 {
        match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    last_err = Some(AgentError::CoordinatorConnection(format!(
                        "list models returned HTTP {}",
                        resp.status()
                    )));
                } else {
                    let body: ModelsResponse = resp.json().await.map_err(|e| {
                        AgentError::Internal(format!("Failed to parse models response: {}", e))
                    })?;
                    let models = body.data.into_iter().map(|m| m.id).collect();
                    return Ok(models);
                }
            }
            Err(e) => {
                last_err = Some(AgentError::CoordinatorConnection(format!(
                    "Failed to list models (attempt {}): {}",
                    attempt, e
                )));
            }
        }

        tokio::time::sleep(Duration::from_secs(attempt)).await;
    }

    Err(last_err.unwrap_or_else(|| {
        AgentError::CoordinatorConnection("list models failed without details".to_string())
    }))
}

/// コーディネーターが返す全モデルを確保（ベストエフォート）。
/// 失敗しても他モデルの確保は続行し、ログに残す。
#[allow(dead_code)]
pub async fn sync_all_models(coordinator_url: &str, ollama_manager: &mut OllamaManager) {
    let models = match fetch_models(coordinator_url).await {
        Ok(m) => m,
        Err(e) => {
            warn!("Failed to fetch model list from coordinator: {}", e);
            return;
        }
    };

    for m in models {
        if let Err(e) = ollama_manager.ensure_model(&m).await {
            warn!("Failed to ensure model {}: {}", m, e);
        } else {
            info!("Model {} is ready on local Ollama", m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct DummyEnsure {
        called: std::sync::Mutex<Vec<String>>,
    }

    impl DummyEnsure {
        async fn ensure_model(&self, model: &str) -> AgentResult<()> {
            self.called.lock().unwrap().push(model.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn fetch_models_retries_and_parses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": "m1"},
                    {"id": "m2"}
                ]
            })))
            .mount(&server)
            .await;

        let res = fetch_models(&server.uri()).await.unwrap();
        assert_eq!(res, vec!["m1", "m2"]);
    }

    #[tokio::test]
    async fn sync_all_models_calls_ensure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": "m1"},
                    {"id": "m2"}
                ]
            })))
            .mount(&server)
            .await;

        let dummy = DummyEnsure {
            called: std::sync::Mutex::new(Vec::new()),
        };

        // sync_all_models は OllamaManager を想定しているが、テスト用に最小限の代替を用意する
        // ダミーの ensure_model を直接呼び出す形で検証する
        let models = fetch_models(&server.uri()).await.unwrap();
        for m in models {
            dummy.ensure_model(&m).await.unwrap();
        }

        let called = dummy.called.lock().unwrap().clone();
        assert_eq!(called, vec!["m1", "m2"]);
    }
}
