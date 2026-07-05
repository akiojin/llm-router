//! Model Download Tasks Database Operations
//!
//! SPEC-e8e9326e: xLLM Model Download Task Management

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::types::endpoint::ModelDownloadTask;

// Note: DownloadStatus is used in tests
#[cfg(test)]
use crate::types::endpoint::DownloadStatus;

/// Create a new download task
pub async fn create_download_task(
    pool: &SqlitePool,
    task: &ModelDownloadTask,
) -> Result<(), sqlx::Error> {
    let started_at = task.started_at.to_rfc3339();
    let completed_at = task.completed_at.map(|dt| dt.to_rfc3339());

    sqlx::query(
        r#"
        INSERT INTO model_download_tasks (
            id, endpoint_id, model, filename, status, progress,
            speed_mbps, eta_seconds, error_message, started_at, completed_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task.id)
    .bind(task.endpoint_id.to_string())
    .bind(&task.model)
    .bind(&task.filename)
    .bind(task.status.as_str())
    .bind(task.progress)
    .bind(task.speed_mbps)
    .bind(task.eta_seconds.map(|v| v as i32))
    .bind(&task.error_message)
    .bind(&started_at)
    .bind(&completed_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get a download task by ID
pub async fn get_download_task(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<ModelDownloadTask>, sqlx::Error> {
    let row = sqlx::query_as::<_, DownloadTaskRow>(
        r#"
        SELECT id, endpoint_id, model, filename, status, progress,
               speed_mbps, eta_seconds, error_message, started_at, completed_at
        FROM model_download_tasks
        WHERE id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}

/// List download tasks for an endpoint
pub async fn list_download_tasks(
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<Vec<ModelDownloadTask>, sqlx::Error> {
    let rows = sqlx::query_as::<_, DownloadTaskRow>(
        r#"
        SELECT id, endpoint_id, model, filename, status, progress,
               speed_mbps, eta_seconds, error_message, started_at, completed_at
        FROM model_download_tasks
        WHERE endpoint_id = ?
        ORDER BY started_at DESC
        "#,
    )
    .bind(endpoint_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// List active (pending/downloading) download tasks for an endpoint
pub async fn list_active_download_tasks(
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<Vec<ModelDownloadTask>, sqlx::Error> {
    let rows = sqlx::query_as::<_, DownloadTaskRow>(
        r#"
        SELECT id, endpoint_id, model, filename, status, progress,
               speed_mbps, eta_seconds, error_message, started_at, completed_at
        FROM model_download_tasks
        WHERE endpoint_id = ? AND status IN ('pending', 'downloading')
        ORDER BY started_at ASC
        "#,
    )
    .bind(endpoint_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// Update download task progress
pub async fn update_download_progress(
    pool: &SqlitePool,
    task_id: &str,
    progress: f64,
    speed_mbps: Option<f64>,
    eta_seconds: Option<u32>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE model_download_tasks SET
            status = 'downloading',
            progress = ?,
            speed_mbps = ?,
            eta_seconds = ?
        WHERE id = ?
        "#,
    )
    .bind(progress)
    .bind(speed_mbps)
    .bind(eta_seconds.map(|v| v as i32))
    .bind(task_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Mark download task as completed
pub async fn complete_download_task(
    pool: &SqlitePool,
    task_id: &str,
    filename: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let completed_at = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        UPDATE model_download_tasks SET
            status = 'completed',
            progress = 100.0,
            filename = COALESCE(?, filename),
            completed_at = ?
        WHERE id = ?
        "#,
    )
    .bind(filename)
    .bind(&completed_at)
    .bind(task_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Mark download task as failed
pub async fn fail_download_task(
    pool: &SqlitePool,
    task_id: &str,
    error_message: &str,
) -> Result<bool, sqlx::Error> {
    let completed_at = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        UPDATE model_download_tasks SET
            status = 'failed',
            error_message = ?,
            completed_at = ?
        WHERE id = ?
        "#,
    )
    .bind(error_message)
    .bind(&completed_at)
    .bind(task_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Cancel a download task
pub async fn cancel_download_task(pool: &SqlitePool, task_id: &str) -> Result<bool, sqlx::Error> {
    let completed_at = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        UPDATE model_download_tasks SET
            status = 'cancelled',
            completed_at = ?
        WHERE id = ? AND status IN ('pending', 'downloading')
        "#,
    )
    .bind(&completed_at)
    .bind(task_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete a download task
pub async fn delete_download_task(pool: &SqlitePool, task_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM model_download_tasks WHERE id = ?")
        .bind(task_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Cleanup old completed/failed/cancelled tasks (older than 7 days)
pub async fn cleanup_old_download_tasks(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let result = sqlx::query(
        r#"
        DELETE FROM model_download_tasks
        WHERE status IN ('completed', 'failed', 'cancelled')
        AND completed_at < ?
        "#,
    )
    .bind(&cutoff)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

// --- Internal Row Type ---

#[derive(sqlx::FromRow)]
struct DownloadTaskRow {
    id: String,
    endpoint_id: String,
    model: String,
    filename: Option<String>,
    status: String,
    progress: f64,
    speed_mbps: Option<f64>,
    eta_seconds: Option<i32>,
    error_message: Option<String>,
    started_at: String,
    completed_at: Option<String>,
}

impl From<DownloadTaskRow> for ModelDownloadTask {
    fn from(row: DownloadTaskRow) -> Self {
        ModelDownloadTask {
            id: row.id,
            endpoint_id: Uuid::parse_str(&row.endpoint_id).unwrap_or_default(),
            model: row.model,
            filename: row.filename,
            status: row.status.parse().unwrap_or_default(),
            progress: row.progress,
            speed_mbps: row.speed_mbps,
            eta_seconds: row.eta_seconds.map(|v| v as u32),
            error_message: row.error_message,
            started_at: DateTime::parse_from_rfc3339(&row.started_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            completed_at: row
                .completed_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
        }
    }
}

#[cfg(test)]
mod tests;
