//! エンドポイントタイプ（xLLM/Ollama/vLLM 等）とそのパース/表示
//!
//! arch-review [H6] round2: types/endpoint.rs から EndpointType を分離。

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// エンドポイントタイプ（SPEC-e8e9326e追加要件 2026-01-26）
///
/// エンドポイントの種別を表す列挙型。
/// 登録時に自動判別され、タイプに応じた機能制御に使用される。
/// 対応する5タイプのみ許可し、検出できないエンドポイントの登録は拒否する。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    /// 本プロジェクト独自の推論エンジン（xLLM）
    Xllm,
    /// Ollamaサーバー
    Ollama,
    /// vLLMサーバー
    Vllm,
    /// LM Studioサーバー
    LmStudio,
    /// llama.cppサーバー
    Llamacpp,
    /// その他のOpenAI互換API
    OpenaiCompatible,
}

impl EndpointType {
    /// EndpointTypeを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Xllm => "xllm",
            Self::Ollama => "ollama",
            Self::Vllm => "vllm",
            Self::LmStudio => "lm_studio",
            Self::Llamacpp => "llamacpp",
            Self::OpenaiCompatible => "openai_compatible",
        }
    }

    /// モデルダウンロードをサポートするか
    pub fn supports_model_download(&self) -> bool {
        matches!(self, Self::Xllm | Self::Ollama | Self::LmStudio)
    }

    /// モデル削除をサポートするか
    ///
    /// LM Studio: 削除APIなし（0.4.6時点）
    pub fn supports_model_delete(&self) -> bool {
        matches!(self, Self::Ollama)
    }

    /// モデルメタデータ取得をサポートするか
    pub fn supports_model_metadata(&self) -> bool {
        matches!(self, Self::Xllm | Self::Ollama | Self::LmStudio)
    }

    /// TPS（tokens per second）計測対象かどうか（SPEC-4bb5b55f）
    ///
    /// トークン使用量レポートの信頼性が保証されるエンドポイントタイプを判定する。
    ///
    /// arch-review [L15]: 現行仕様では OpenAI 互換を含む**全タイプが計測対象**のため
    /// 常に `true` を返す。特定タイプを計測対象外にする将来仕様のための拡張点として
    /// 呼び出し側のガード（例: proxy.rs の TPS 永続化判定）ごと保持している。
    /// タイプ依存の判定が不要と確定した場合はガードごと削除してよい。
    pub fn is_tps_trackable(&self) -> bool {
        true
    }

    /// エンドポイントタイプごとの推論タイムアウト推奨値（秒）
    pub fn recommended_inference_timeout_secs(&self) -> u32 {
        match self {
            Self::Xllm | Self::Ollama | Self::LmStudio => 600,
            Self::Vllm | Self::Llamacpp | Self::OpenaiCompatible => 120,
        }
    }
}

/// EndpointType のパースエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEndpointTypeError(pub String);

impl std::fmt::Display for ParseEndpointTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown endpoint type: '{}'", self.0)
    }
}

impl std::error::Error for ParseEndpointTypeError {}

impl FromStr for EndpointType {
    type Err = ParseEndpointTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "xllm" => Ok(Self::Xllm),
            "ollama" => Ok(Self::Ollama),
            "vllm" => Ok(Self::Vllm),
            "lm_studio" => Ok(Self::LmStudio),
            "llamacpp" => Ok(Self::Llamacpp),
            "openai_compatible" => Ok(Self::OpenaiCompatible),
            _ => Err(ParseEndpointTypeError(s.to_string())),
        }
    }
}

impl std::fmt::Display for EndpointType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
