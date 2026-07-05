//! テキスト生成系リクエスト DTO（チャット/生成）
//!
//! arch-review [H6] round2: common/protocol.rs からリクエスト型を分離。

use serde::{Deserialize, Serialize};

/// LLM runtimeチャットリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// モデル名
    pub model: String,
    /// メッセージ配列
    pub messages: Vec<ChatMessage>,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}

/// チャットメッセージ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    /// ロール ("user", "assistant", "system")
    pub role: String,
    /// メッセージ内容
    pub content: String,
}

/// Chat Completionsリクエスト
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    /// モデル名
    pub model: String,
    /// メッセージ配列
    pub messages: Vec<ChatMessage>,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
    /// 最大トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Generateリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// モデル名
    pub model: String,
    /// プロンプト
    pub prompt: String,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}
