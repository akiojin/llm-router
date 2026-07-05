//! endpoints テーブルの SQL 行構造体とドメイン変換
//!
//! arch-review [H6] round2: db/endpoints.rs から行/集計 DTO を分離。

use crate::types::endpoint::Endpoint;
use uuid::Uuid;

// --- Internal Row Types ---

#[derive(sqlx::FromRow)]
pub(crate) struct EndpointRequestTotalsRow {
    pub(crate) total_requests: i64,
    pub(crate) successful_requests: i64,
    pub(crate) failed_requests: i64,
}

/// エンドポイント集計リクエスト数の合計値。
#[derive(Debug, Clone, Copy)]
pub struct EndpointRequestTotals {
    /// 全リクエスト数。
    pub total_requests: i64,
    /// 成功リクエスト数。
    pub successful_requests: i64,
    /// 失敗リクエスト数。
    pub failed_requests: i64,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EndpointRow {
    id: String,
    name: String,
    base_url: String,
    api_key_encrypted: Option<String>,
    status: String,
    /// SPEC-e8e9326e: エンドポイントタイプ
    endpoint_type: String,
    health_check_interval_secs: i32,
    inference_timeout_secs: i32,
    latency_ms: Option<i32>,
    last_seen: Option<String>,
    last_error: Option<String>,
    error_count: i32,
    registered_at: String,
    notes: Option<String>,
    /// SPEC-e8e9326e移行用: エンドポイントの機能一覧（JSON形式）
    capabilities: Option<String>,
    /// SPEC-f8e3a1b7: デバイス情報（JSON形式）
    device_info: Option<String>,
    /// SPEC-f8e3a1b7: 推論レイテンシ（EMA α=0.2で計算）
    inference_latency_ms: Option<f64>,
    /// SPEC-8c32349f: 累計リクエスト数
    total_requests: i64,
    /// SPEC-8c32349f: 累計成功リクエスト数
    successful_requests: i64,
    /// SPEC-8c32349f: 累計失敗リクエスト数
    failed_requests: i64,
}

impl From<EndpointRow> for Endpoint {
    fn from(row: EndpointRow) -> Self {
        use crate::types::endpoint::EndpointCapability;

        Endpoint {
            id: Uuid::parse_str(&row.id).unwrap_or_default(),
            name: row.name,
            base_url: row.base_url,
            api_key: row.api_key_encrypted,
            status: row.status.parse().unwrap_or_default(),
            endpoint_type: row
                .endpoint_type
                .parse()
                .unwrap_or(crate::types::endpoint::EndpointType::OpenaiCompatible),
            health_check_interval_secs: row.health_check_interval_secs as u32,
            inference_timeout_secs: row.inference_timeout_secs as u32,
            latency_ms: row.latency_ms.map(|v| v as u32),
            last_seen: row
                .last_seen
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            last_error: row.last_error,
            error_count: row.error_count as u32,
            registered_at: chrono::DateTime::parse_from_rfc3339(&row.registered_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            notes: row.notes,
            capabilities: row
                .capabilities
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| vec![EndpointCapability::ChatCompletion]),
            // GPU情報（/api/healthから取得、DBには未保存）
            gpu_device_count: None,
            gpu_total_memory_bytes: None,
            gpu_used_memory_bytes: None,
            gpu_capability_score: None,
            active_requests: None,
            // SPEC-f8e3a1b7: デバイス情報とレイテンシ
            device_info: row.device_info.and_then(|s| serde_json::from_str(&s).ok()),
            inference_latency_ms: row.inference_latency_ms,
            total_requests: row.total_requests,
            successful_requests: row.successful_requests,
            failed_requests: row.failed_requests,
        }
    }
}
