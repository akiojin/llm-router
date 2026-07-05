//! endpoint_health_checks テーブルの記録・履歴取得・クリーンアップ
//!
//! arch-review [H6]: db/endpoints.rs からヘルスチェック履歴の DB 操作を分離。

use crate::types::endpoint::EndpointHealthCheck;
use sqlx::SqlitePool;
use uuid::Uuid;

// --- EndpointHealthCheck CRUD ---

/// ヘルスチェック結果を記録
pub async fn record_health_check(
    pool: &SqlitePool,
    check: &EndpointHealthCheck,
) -> Result<i64, sqlx::Error> {
    let checked_at = check.checked_at.to_rfc3339();

    let result = sqlx::query(
        r#"
        INSERT INTO endpoint_health_checks (
            endpoint_id, checked_at, success, latency_ms,
            error_message, status_before, status_after
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(check.endpoint_id.to_string())
    .bind(&checked_at)
    .bind(check.success)
    .bind(check.latency_ms.map(|v| v as i32))
    .bind(&check.error_message)
    .bind(check.status_before.as_str())
    .bind(check.status_after.as_str())
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

/// エンドポイントのヘルスチェック履歴を取得
pub async fn list_health_checks(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    limit: i32,
) -> Result<Vec<EndpointHealthCheck>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointHealthCheckRow>(
        r#"
        SELECT id, endpoint_id, checked_at, success, latency_ms,
               error_message, status_before, status_after
        FROM endpoint_health_checks
        WHERE endpoint_id = ?
        ORDER BY checked_at DESC
        LIMIT ?
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// 古いヘルスチェック履歴を削除（30日以上前）
pub async fn cleanup_old_health_checks(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let result = sqlx::query("DELETE FROM endpoint_health_checks WHERE checked_at < ?")
        .bind(&cutoff)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

#[derive(sqlx::FromRow)]
struct EndpointHealthCheckRow {
    id: i64,
    endpoint_id: String,
    checked_at: String,
    success: bool,
    latency_ms: Option<i32>,
    error_message: Option<String>,
    status_before: String,
    status_after: String,
}

impl From<EndpointHealthCheckRow> for EndpointHealthCheck {
    fn from(row: EndpointHealthCheckRow) -> Self {
        EndpointHealthCheck {
            id: row.id,
            endpoint_id: Uuid::parse_str(&row.endpoint_id).unwrap_or_default(),
            checked_at: chrono::DateTime::parse_from_rfc3339(&row.checked_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            success: row.success,
            latency_ms: row.latency_ms.map(|v| v as u32),
            error_message: row.error_message,
            status_before: row.status_before.parse().unwrap_or_default(),
            status_after: row.status_after.parse().unwrap_or_default(),
        }
    }
}
