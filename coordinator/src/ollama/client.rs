//! Ollamaクライアント
//!
//! エージェント経由でモデル情報を取得し、事前定義リストと統合

use crate::registry::models::ModelInfo;
use ollama_coordinator_common::error::{CoordinatorError, CoordinatorResult};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

/// Ollamaクライアント
pub struct OllamaClient {
    http_client: Client,
}

/// Ollama APIのモデル一覧レスポンス
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

/// Ollamaモデル情報
#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    #[allow(dead_code)] // 将来の使用のために保持
    digest: Option<String>,
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

/// Ollamaモデル詳細
#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}

impl OllamaClient {
    /// 新しいOllamaClientを作成
    pub fn new() -> CoordinatorResult<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                CoordinatorError::Internal(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self { http_client })
    }

    /// エージェントからモデル一覧を取得
    ///
    /// # Arguments
    /// * `agent_base_url` - エージェントのベースURL（例: "http://192.168.1.10:11434"）
    pub async fn fetch_models_from_agent(
        &self,
        agent_base_url: &str,
    ) -> CoordinatorResult<Vec<ModelInfo>> {
        let url = format!("{}/api/tags", agent_base_url);

        debug!("Fetching models from agent: {}", url);

        let response = self.http_client.get(&url).send().await.map_err(|e| {
            CoordinatorError::Internal(format!("Failed to fetch models from agent: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(CoordinatorError::Internal(format!(
                "Failed to fetch models: HTTP {}",
                response.status()
            )));
        }

        let tags_response: OllamaTagsResponse = response.json().await.map_err(|e| {
            CoordinatorError::Internal(format!("Failed to parse models response: {}", e))
        })?;

        let models = tags_response
            .models
            .into_iter()
            .map(|m| self.convert_ollama_model(m))
            .collect();

        Ok(models)
    }

    /// 事前定義モデルリストを取得
    pub fn get_predefined_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo::new(
                "gpt-oss:20b".to_string(),
                10_000_000_000,
                "GPT-OSS 20B parameter model".to_string(),
                16_000_000_000,
                vec!["llm".to_string(), "text".to_string()],
            ),
            ModelInfo::new(
                "gpt-oss:7b".to_string(),
                4_000_000_000,
                "GPT-OSS 7B parameter model".to_string(),
                8_000_000_000,
                vec!["llm".to_string(), "text".to_string()],
            ),
            ModelInfo::new(
                "gpt-oss:3b".to_string(),
                2_000_000_000,
                "GPT-OSS 3B parameter model".to_string(),
                4_500_000_000,
                vec!["llm".to_string(), "text".to_string()],
            ),
            ModelInfo::new(
                "gpt-oss:1b".to_string(),
                1_000_000_000,
                "GPT-OSS 1B parameter model".to_string(),
                2_000_000_000,
                vec!["llm".to_string(), "text".to_string()],
            ),
            ModelInfo::new(
                "llama3.2".to_string(),
                5_000_000_000,
                "Llama 3.2 model".to_string(),
                6_000_000_000,
                vec!["llm".to_string(), "text".to_string()],
            ),
            ModelInfo::new(
                "deepseek-r1".to_string(),
                8_000_000_000,
                "DeepSeek R1 reasoning model".to_string(),
                10_000_000_000,
                vec!["llm".to_string(), "reasoning".to_string()],
            ),
        ]
    }

    /// エージェントから取得したモデルと事前定義リストをマージ
    pub async fn get_available_models(
        &self,
        agent_base_urls: Vec<String>,
    ) -> CoordinatorResult<Vec<ModelInfo>> {
        let mut all_models = Vec::new();
        let mut model_names = std::collections::HashSet::new();

        // エージェントからモデルを取得
        for agent_url in agent_base_urls {
            match self.fetch_models_from_agent(&agent_url).await {
                Ok(models) => {
                    for model in models {
                        if model_names.insert(model.name.clone()) {
                            all_models.push(model);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch models from {}: {}", agent_url, e);
                }
            }
        }

        // 事前定義モデルを追加（重複を避ける）
        for model in self.get_predefined_models() {
            if model_names.insert(model.name.clone()) {
                all_models.push(model);
            }
        }

        Ok(all_models)
    }

    /// OllamaModelをModelInfoに変換
    fn convert_ollama_model(&self, ollama_model: OllamaModel) -> ModelInfo {
        let description = if let Some(details) = &ollama_model.details {
            format!(
                "{} ({})",
                details.parameter_size.as_deref().unwrap_or("unknown size"),
                details
                    .quantization_level
                    .as_deref()
                    .unwrap_or("unknown quantization")
            )
        } else {
            "No description available".to_string()
        };

        // モデルサイズから必要メモリを推定（1.5倍）
        let required_memory = (ollama_model.size as f64 * 1.5) as u64;

        ModelInfo::new(
            ollama_model.name,
            ollama_model.size,
            description,
            required_memory,
            vec!["llm".to_string()],
        )
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_predefined_models() {
        let client = OllamaClient::new().unwrap();
        let models = client.get_predefined_models();

        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.name == "gpt-oss:20b"));
        assert!(models.iter().any(|m| m.name == "llama3.2"));
    }

    #[test]
    fn test_convert_ollama_model() {
        let client = OllamaClient::new().unwrap();

        let ollama_model = OllamaModel {
            name: "test-model:latest".to_string(),
            size: 5_000_000_000,
            digest: Some("abc123".to_string()),
            details: Some(OllamaModelDetails {
                parameter_size: Some("7B".to_string()),
                quantization_level: Some("Q4_K_M".to_string()),
            }),
        };

        let model_info = client.convert_ollama_model(ollama_model);

        assert_eq!(model_info.name, "test-model:latest");
        assert_eq!(model_info.size, 5_000_000_000);
        assert!(model_info.description.contains("7B"));
        assert!(model_info.description.contains("Q4_K_M"));
        assert_eq!(model_info.required_memory, 7_500_000_000); // 5GB * 1.5
    }
}
