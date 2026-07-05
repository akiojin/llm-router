//! モデルがサポートする API 種別
//!
//! arch-review [H6] round2: types/endpoint.rs から SupportedAPI を分離。

use serde::{Deserialize, Serialize};

/// モデルがサポートするAPI種別（SPEC-0f1de549）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SupportedAPI {
    /// Chat Completions API（/v1/chat/completions）
    ChatCompletions,
    /// Responses API（/v1/responses）
    Responses,
    /// Embeddings API（/v1/embeddings）
    Embeddings,
    /// Image input / vision support in chat-style requests.
    ImageInput,
    /// Image generation API（/v1/images/*）
    ImageGeneration,
}

impl SupportedAPI {
    /// SupportedAPIを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Embeddings => "embeddings",
            Self::ImageInput => "image_input",
            Self::ImageGeneration => "image_generation",
        }
    }

    /// Parse common upstream names into the normalized supported API enum.
    pub fn from_api_str(value: &str) -> Option<Self> {
        match normalize_api_name(value).as_str() {
            "chat" | "chat_completion" | "chat_completions" => Some(Self::ChatCompletions),
            "response" | "responses" => Some(Self::Responses),
            "embedding" | "embeddings" => Some(Self::Embeddings),
            "image_input" | "vision" | "visual" | "multimodal" => Some(Self::ImageInput),
            "image_generation" | "images_generation" | "images_generations" => {
                Some(Self::ImageGeneration)
            }
            _ => None,
        }
    }
}

fn normalize_api_name(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

impl std::fmt::Display for SupportedAPI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
