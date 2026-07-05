//! 通信プロトコル定義
//!
//! OpenAI互換API用のリクエスト/レスポンス型を定義します。

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

mod audio;
pub use audio::*;
mod requests;
pub use requests::*;
mod tps;
pub use tps::*;
mod image;
#[cfg(test)]
use crate::types::media::{ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};
pub use image::*;

/// リクエスト/レスポンスレコード
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResponseRecord {
    /// レコードの一意識別子
    pub id: Uuid,
    /// リクエスト受信時刻
    pub timestamp: DateTime<Utc>,
    /// リクエストタイプ（Chat または Generate）
    pub request_type: RequestType,
    /// 使用されたモデル名
    pub model: String,
    /// 処理したエンドポイントのID
    pub endpoint_id: Uuid,
    /// エンドポイント名
    pub endpoint_name: String,
    /// エンドポイントのIPアドレス
    pub endpoint_ip: IpAddr,
    /// リクエスト元クライアントのIPアドレス
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<IpAddr>,
    /// リクエスト本文（JSON形式）
    pub request_body: serde_json::Value,
    /// レスポンス本文（JSON形式、エラー時はNone）
    pub response_body: Option<serde_json::Value>,
    /// 処理時間（ミリ秒）
    pub duration_ms: u64,
    /// レコードのステータス（成功 or エラー）
    pub status: RecordStatus,
    /// レスポンス完了時刻
    pub completed_at: DateTime<Utc>,
    /// 入力トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// 出力トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// 総トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    /// APIキーID（api_keysテーブル参照）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_id: Option<Uuid>,
}

/// リクエストタイプ
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestType {
    /// /v1/messages エンドポイント (Anthropic Messages API)
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
    /// /v1/chat/completions エンドポイント
    Chat,
    /// /v1/responses エンドポイント (Open Responses API)
    Responses,
    /// /v1/completions エンドポイント
    Generate,
    /// /v1/embeddings エンドポイント
    Embeddings,
    /// /v1/audio/transcriptions エンドポイント (ASR)
    Transcription,
    /// /v1/audio/speech エンドポイント (TTS)
    Speech,
    /// /v1/images/generations エンドポイント
    ImageGeneration,
    /// /v1/images/edits エンドポイント
    ImageEdit,
    /// /v1/images/variations エンドポイント
    ImageVariation,
}

/// レコードステータス
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RecordStatus {
    /// 正常に処理完了
    Success,
    /// エラー発生
    Error {
        /// エラーメッセージ
        message: String,
    },
}

impl RequestResponseRecord {
    /// エンドポイント特定済みのレコードを作成する。
    ///
    /// `status` から `RecordStatus` を自動判定する。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint_id: Uuid,
        endpoint_name: String,
        endpoint_ip: IpAddr,
        model: String,
        request_type: RequestType,
        request_body: serde_json::Value,
        status: StatusCode,
        duration: std::time::Duration,
        client_ip: Option<IpAddr>,
        api_key_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type,
            model,
            endpoint_id,
            endpoint_name,
            endpoint_ip,
            client_ip,
            request_body,
            response_body: None,
            duration_ms: duration.as_millis() as u64,
            status: if status.is_success() {
                RecordStatus::Success
            } else {
                RecordStatus::Error {
                    message: format!("HTTP {}", status.as_u16()),
                }
            },
            completed_at: Utc::now(),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id,
        }
    }

    /// エンドポイント未特定のエラーレコードを作成する。
    pub fn error(
        model: String,
        request_type: RequestType,
        request_body: serde_json::Value,
        message: String,
        duration_ms: u64,
        client_ip: Option<IpAddr>,
        api_key_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type,
            model,
            endpoint_id: Uuid::nil(),
            endpoint_name: "N/A".to_string(),
            endpoint_ip: IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            client_ip,
            request_body,
            response_body: None,
            duration_ms,
            status: RecordStatus::Error { message },
            completed_at: Utc::now(),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id,
        }
    }
}

#[cfg(test)]
mod tests;
