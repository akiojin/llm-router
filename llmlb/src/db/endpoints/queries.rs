//! 属性フィルタによる読み取り専用クエリ群。
//!
//! 名前・ステータス・タイプでエンドポイントを検索する参照系関数。

use super::dto::EndpointRow;
use crate::types::endpoint::{Endpoint, EndpointStatus};
use sqlx::SqlitePool;

/// 名前でエンドポイントを検索
pub async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Endpoint>, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE name = ?
        "#,
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}

/// ステータスでフィルタしてエンドポイント一覧を取得
pub async fn list_endpoints_by_status(
    pool: &SqlitePool,
    status: EndpointStatus,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE status = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(status.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// タイプでフィルタしてエンドポイント一覧を取得（SPEC-e8e9326e）
pub async fn list_endpoints_by_type(
    pool: &SqlitePool,
    endpoint_type: crate::types::endpoint::EndpointType,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE endpoint_type = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(endpoint_type.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// タイプとステータスでフィルタしてエンドポイント一覧を取得（SPEC-e8e9326e）
pub async fn list_endpoints_by_type_and_status(
    pool: &SqlitePool,
    endpoint_type: crate::types::endpoint::EndpointType,
    status: EndpointStatus,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE endpoint_type = ? AND status = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(endpoint_type.as_str())
    .bind(status.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}
