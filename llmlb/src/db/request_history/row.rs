//! SQLite request_history 行構造体とドメイン RequestResponseRecord への変換
//!
//! arch-review [H6]: db/request_history.rs から行→レコードのマッピングを分離。

use crate::common::error::LbError;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType};
use chrono::{DateTime, Utc};
use std::net::IpAddr;
use uuid::Uuid;

/// SQLiteから取得した行データ
#[derive(sqlx::FromRow)]
pub(crate) struct RequestHistoryRow {
    id: String,
    timestamp: String,
    request_type: String,
    model: String,
    endpoint_id: String,
    endpoint_name: String,
    endpoint_ip: String,
    client_ip: Option<String>,
    request_body: String,
    response_body: Option<String>,
    duration_ms: i64,
    status: String,
    error_message: Option<String>,
    completed_at: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    api_key_id: Option<String>,
}

impl TryFrom<RequestHistoryRow> for RequestResponseRecord {
    type Error = LbError;

    fn try_from(row: RequestHistoryRow) -> Result<Self, Self::Error> {
        let id = Uuid::parse_str(&row.id)
            .map_err(|e| LbError::Database(format!("Invalid UUID: {}", e)))?;

        let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
            .map_err(|e| LbError::Database(format!("Invalid timestamp: {}", e)))?
            .with_timezone(&Utc);

        let request_type = match row.request_type.as_str() {
            "AnthropicMessages" => RequestType::AnthropicMessages,
            "Chat" => RequestType::Chat,
            "Responses" => RequestType::Responses,
            "Generate" => RequestType::Generate,
            "Embeddings" => RequestType::Embeddings,
            "Transcription" => RequestType::Transcription,
            "Speech" => RequestType::Speech,
            "ImageGeneration" => RequestType::ImageGeneration,
            "ImageEdit" => RequestType::ImageEdit,
            "ImageVariation" => RequestType::ImageVariation,
            _ => RequestType::Chat, // フォールバック
        };

        let endpoint_id = Uuid::parse_str(&row.endpoint_id)
            .map_err(|e| LbError::Database(format!("Invalid endpoint UUID: {}", e)))?;

        let endpoint_ip: IpAddr = row
            .endpoint_ip
            .parse()
            .map_err(|e| LbError::Database(format!("Invalid endpoint IP: {}", e)))?;

        let client_ip = row
            .client_ip
            .map(|ip| {
                ip.parse::<IpAddr>()
                    .map_err(|e| LbError::Database(format!("Invalid client IP: {}", e)))
            })
            .transpose()?;

        let request_body: serde_json::Value = serde_json::from_str(&row.request_body)
            .map_err(|e| LbError::Database(format!("Invalid request body: {}", e)))?;

        let response_body = row
            .response_body
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| LbError::Database(format!("Invalid response body: {}", e)))?;

        let status = match row.status.as_str() {
            "success" => RecordStatus::Success,
            "error" => RecordStatus::Error {
                message: row.error_message.unwrap_or_default(),
            },
            _ => RecordStatus::Success,
        };

        let completed_at = DateTime::parse_from_rfc3339(&row.completed_at)
            .map_err(|e| LbError::Database(format!("Invalid completed_at: {}", e)))?
            .with_timezone(&Utc);

        Ok(RequestResponseRecord {
            id,
            timestamp,
            request_type,
            model: row.model,
            endpoint_id,
            endpoint_name: row.endpoint_name,
            endpoint_ip,
            client_ip,
            request_body,
            response_body,
            duration_ms: row.duration_ms as u64,
            status,
            completed_at,
            input_tokens: row.input_tokens.map(|v| v as u32),
            output_tokens: row.output_tokens.map(|v| v as u32),
            total_tokens: row.total_tokens.map(|v| v as u32),
            api_key_id: row
                .api_key_id
                .map(|id| {
                    Uuid::parse_str(&id)
                        .map_err(|e| LbError::Database(format!("Invalid api_key_id UUID: {}", e)))
                })
                .transpose()?,
        })
    }
}
