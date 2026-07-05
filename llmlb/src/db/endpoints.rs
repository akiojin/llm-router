//! エンドポイントデータベース操作
//!
//! SPEC-e8e9326e: llmlb主導エンドポイント登録システム

use crate::types::endpoint::{Endpoint, EndpointStatus};
use sqlx::SqlitePool;
use uuid::Uuid;

mod dto;
mod health_checks;
mod models;
mod queries;
pub use dto::EndpointRequestTotals;
use dto::{EndpointRequestTotalsRow, EndpointRow};
pub use health_checks::*;
pub use models::*;
pub use queries::*;

/// エンドポイントを登録
pub async fn create_endpoint(pool: &SqlitePool, endpoint: &Endpoint) -> Result<(), sqlx::Error> {
    let id = endpoint.id.to_string();
    let status = endpoint.status.as_str();
    let registered_at = endpoint.registered_at.to_rfc3339();
    let last_seen = endpoint.last_seen.map(|dt| dt.to_rfc3339());
    let capabilities = serde_json::to_string(&endpoint.capabilities).unwrap_or_default();
    // SPEC-f8e3a1b7: デバイス情報と推論レイテンシ
    let device_info = endpoint
        .device_info
        .as_ref()
        .and_then(|d| serde_json::to_string(d).ok());

    let endpoint_type = endpoint.endpoint_type.as_str();
    sqlx::query(
        r#"
        INSERT INTO endpoints (
            id, name, base_url, api_key_encrypted, status, endpoint_type,
            health_check_interval_secs, inference_timeout_secs,
            latency_ms, last_seen, last_error, error_count,
            registered_at, notes, capabilities, device_info, inference_latency_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&endpoint.name)
    .bind(&endpoint.base_url)
    .bind(&endpoint.api_key)
    .bind(status)
    .bind(endpoint_type)
    .bind(endpoint.health_check_interval_secs as i32)
    .bind(endpoint.inference_timeout_secs as i32)
    .bind(endpoint.latency_ms.map(|v| v as i32))
    .bind(&last_seen)
    .bind(&endpoint.last_error)
    .bind(endpoint.error_count as i32)
    .bind(&registered_at)
    .bind(&endpoint.notes)
    .bind(&capabilities)
    .bind(&device_info)
    .bind(endpoint.inference_latency_ms)
    .execute(pool)
    .await?;

    Ok(())
}

/// エンドポイント一覧を取得
pub async fn list_endpoints(pool: &SqlitePool) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        ORDER BY registered_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// IDでエンドポイントを取得
pub async fn get_endpoint(pool: &SqlitePool, id: Uuid) -> Result<Option<Endpoint>, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}

/// エンドポイントを更新
pub async fn update_endpoint(pool: &SqlitePool, endpoint: &Endpoint) -> Result<bool, sqlx::Error> {
    let id = endpoint.id.to_string();
    let status = endpoint.status.as_str();
    let endpoint_type = endpoint.endpoint_type.as_str();
    let last_seen = endpoint.last_seen.map(|dt| dt.to_rfc3339());
    let capabilities = serde_json::to_string(&endpoint.capabilities).unwrap_or_default();
    // SPEC-f8e3a1b7: デバイス情報と推論レイテンシ
    let device_info = endpoint
        .device_info
        .as_ref()
        .and_then(|d| serde_json::to_string(d).ok());

    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            name = ?, base_url = ?, api_key_encrypted = ?, status = ?, endpoint_type = ?,
            health_check_interval_secs = ?, inference_timeout_secs = ?,
            latency_ms = ?, last_seen = ?, last_error = ?, error_count = ?,
            notes = ?, capabilities = ?, device_info = ?, inference_latency_ms = ?
        WHERE id = ?
        "#,
    )
    .bind(&endpoint.name)
    .bind(&endpoint.base_url)
    .bind(&endpoint.api_key)
    .bind(status)
    .bind(endpoint_type)
    .bind(endpoint.health_check_interval_secs as i32)
    .bind(endpoint.inference_timeout_secs as i32)
    .bind(endpoint.latency_ms.map(|v| v as i32))
    .bind(&last_seen)
    .bind(&endpoint.last_error)
    .bind(endpoint.error_count as i32)
    .bind(&endpoint.notes)
    .bind(&capabilities)
    .bind(&device_info)
    .bind(endpoint.inference_latency_ms)
    .bind(&id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントを削除
pub async fn delete_endpoint(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM endpoints WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのタイプを更新（SPEC-e8e9326e）
pub async fn update_endpoint_type(
    pool: &SqlitePool,
    id: Uuid,
    endpoint_type: crate::types::endpoint::EndpointType,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            endpoint_type = ?
        WHERE id = ?
        "#,
    )
    .bind(endpoint_type.as_str())
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのステータスを更新
pub async fn update_endpoint_status(
    pool: &SqlitePool,
    id: Uuid,
    status: EndpointStatus,
    latency_ms: Option<u32>,
    last_error: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            status = ?,
            latency_ms = COALESCE(?, latency_ms),
            last_seen = ?,
            last_error = ?,
            error_count = CASE WHEN ? = 'error' THEN error_count + 1 ELSE 0 END
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(latency_ms.map(|v| v as i32))
    .bind(&now)
    .bind(last_error)
    .bind(status.as_str())
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントの推論レイテンシを更新（SPEC-f8e3a1b7）
/// EMA (α=0.2) で計算された値を保存
pub async fn update_inference_latency(
    pool: &SqlitePool,
    id: Uuid,
    inference_latency_ms: Option<f64>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            inference_latency_ms = ?
        WHERE id = ?
        "#,
    )
    .bind(inference_latency_ms)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのデバイス情報を更新（SPEC-f8e3a1b7）
/// /api/system APIから取得した情報を保存
pub async fn update_device_info(
    pool: &SqlitePool,
    id: Uuid,
    device_info: Option<&crate::types::endpoint::DeviceInfo>,
) -> Result<bool, sqlx::Error> {
    let device_info_json = device_info.and_then(|d| serde_json::to_string(d).ok());
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            device_info = ?
        WHERE id = ?
        "#,
    )
    .bind(&device_info_json)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのリクエストカウンタをインクリメント（SPEC-8c32349f）
pub async fn increment_request_counters(
    pool: &SqlitePool,
    id: Uuid,
    success: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            total_requests = total_requests + 1,
            successful_requests = successful_requests + CASE WHEN ? THEN 1 ELSE 0 END,
            failed_requests = failed_requests + CASE WHEN ? THEN 0 ELSE 1 END
        WHERE id = ?
        "#,
    )
    .bind(success)
    .bind(success)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントの累計リクエスト統計を集計して取得（TOPカード永続化用）
pub async fn get_request_totals(pool: &SqlitePool) -> Result<EndpointRequestTotals, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRequestTotalsRow>(
        r#"
        SELECT
            COALESCE(SUM(total_requests), 0) as total_requests,
            COALESCE(SUM(successful_requests), 0) as successful_requests,
            COALESCE(SUM(failed_requests), 0) as failed_requests
        FROM endpoints
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(EndpointRequestTotals {
        total_requests: row.total_requests,
        successful_requests: row.successful_requests,
        failed_requests: row.failed_requests,
    })
}

#[cfg(test)]
mod tests;
