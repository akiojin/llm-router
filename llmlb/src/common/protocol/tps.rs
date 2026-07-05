//! TPS 計測対象の API 種別とデータ取得元の DTO
//!
//! arch-review [H6] round2: common/protocol.rs から TPS 関連型を分離。

use super::RequestType;
use serde::{Deserialize, Serialize};

/// TPS計測対象のAPI種別。
///
/// 比較可能性を担保するため、TPSは API 種別ごとに分離して集計する。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TpsApiKind {
    /// /v1/chat/completions
    ChatCompletions,
    /// /v1/completions
    Completions,
    /// /v1/responses
    Responses,
}

impl TpsApiKind {
    /// RequestType から TPS API 種別を解決する。
    ///
    /// テキスト生成系以外（embeddings/audio/images）は TPS対象外として None を返す。
    pub fn from_request_type(request_type: RequestType) -> Option<Self> {
        match request_type {
            RequestType::AnthropicMessages | RequestType::Chat => Some(Self::ChatCompletions),
            RequestType::Responses => Some(Self::Responses),
            RequestType::Generate => Some(Self::Completions),
            RequestType::Embeddings
            | RequestType::Transcription
            | RequestType::Speech
            | RequestType::ImageGeneration
            | RequestType::ImageEdit
            | RequestType::ImageVariation => None,
        }
    }
}

/// TPSデータの取得元。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TpsSource {
    /// 本番リクエスト由来
    Production,
    /// ベンチマーク実行由来
    Benchmark,
}
