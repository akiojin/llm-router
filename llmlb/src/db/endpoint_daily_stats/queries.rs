//! 日次統計の行マッピングと参照系クエリ。
//!
//! sqlx 行型から DTO への変換と、日次/モデル別集計の read-only 取得を担う。

use super::{DailyStatEntry, ModelStatEntry};
use sqlx::SqlitePool;
use uuid::Uuid;

// --- Internal Row Types ---

#[derive(sqlx::FromRow)]
struct DailyStatRow {
    date: String,
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
}

impl From<DailyStatRow> for DailyStatEntry {
    fn from(row: DailyStatRow) -> Self {
        DailyStatEntry {
            date: row.date,
            total_requests: row.total_requests,
            successful_requests: row.successful_requests,
            failed_requests: row.failed_requests,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ModelStatRow {
    model_id: String,
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    total_output_tokens: i64,
    total_duration_ms: i64,
}

impl From<ModelStatRow> for ModelStatEntry {
    fn from(row: ModelStatRow) -> Self {
        ModelStatEntry {
            model_id: row.model_id,
            total_requests: row.total_requests,
            successful_requests: row.successful_requests,
            failed_requests: row.failed_requests,
            total_output_tokens: row.total_output_tokens,
            total_duration_ms: row.total_duration_ms,
        }
    }
}

/// 日次集計データを取得（期間指定）
///
/// 指定エンドポイントの直近N日分の日次データを日付昇順で返す。
/// 全モデルの合計値を日付ごとに集計する。
/// 日付はサーバーローカル時間で計算（書き込み時と一致）。
pub async fn get_daily_stats(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    days: u32,
) -> Result<Vec<DailyStatEntry>, sqlx::Error> {
    let days = days.max(1);
    let start_date = (chrono::Local::now() - chrono::Duration::days((days - 1) as i64))
        .format("%Y-%m-%d")
        .to_string();

    let rows = sqlx::query_as::<_, DailyStatRow>(
        r#"
        SELECT
            date,
            SUM(total_requests) AS total_requests,
            SUM(successful_requests) AS successful_requests,
            SUM(failed_requests) AS failed_requests
        FROM endpoint_daily_stats
        WHERE endpoint_id = ?
          AND date >= ?
        GROUP BY date
        ORDER BY date ASC
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(&start_date)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// モデル別集計データを取得
///
/// 指定エンドポイントのモデル別累計統計を返す。
/// 全日付を通じた合計値をモデルごとに集計し、リクエスト数の降順で返す。
pub async fn get_model_stats(
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<Vec<ModelStatEntry>, sqlx::Error> {
    let rows = sqlx::query_as::<_, ModelStatRow>(
        r#"
        SELECT
            model_id,
            SUM(total_requests) AS total_requests,
            SUM(successful_requests) AS successful_requests,
            SUM(failed_requests) AS failed_requests,
            SUM(total_output_tokens) AS total_output_tokens,
            SUM(total_duration_ms) AS total_duration_ms
        FROM endpoint_daily_stats
        WHERE endpoint_id = ?
        GROUP BY model_id
        ORDER BY total_requests DESC
        "#,
    )
    .bind(endpoint_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// 全エンドポイント横断のモデル別集計データを取得
///
/// endpoint_daily_stats テーブルの全エンドポイントを通じた
/// モデル別累計統計を返す。リクエスト数の降順で返す。
pub async fn get_all_model_stats(pool: &SqlitePool) -> Result<Vec<ModelStatEntry>, sqlx::Error> {
    let rows = sqlx::query_as::<_, ModelStatRow>(
        r#"
        SELECT
            s.model_id,
            SUM(s.total_requests) AS total_requests,
            SUM(s.successful_requests) AS successful_requests,
            SUM(s.failed_requests) AS failed_requests,
            SUM(s.total_output_tokens) AS total_output_tokens,
            SUM(s.total_duration_ms) AS total_duration_ms
        FROM endpoint_daily_stats s
        INNER JOIN endpoints e
            ON e.id = s.endpoint_id
        GROUP BY s.model_id
        ORDER BY total_requests DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// 当日の集計データを取得
///
/// 指定エンドポイントの指定日付のデータを返す。
/// 全モデルの合計値を日付で集計し、単一のDailyStatEntryとして返す。
/// データが存在しない場合はカウンタ0のエントリを返す。
pub async fn get_today_stats(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    today: &str,
) -> Result<DailyStatEntry, sqlx::Error> {
    let row = sqlx::query_as::<_, DailyStatRow>(
        r#"
        SELECT
            date,
            SUM(total_requests) AS total_requests,
            SUM(successful_requests) AS successful_requests,
            SUM(failed_requests) AS failed_requests
        FROM endpoint_daily_stats
        WHERE endpoint_id = ?
          AND date = ?
        GROUP BY date
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(today)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()).unwrap_or(DailyStatEntry {
        date: today.to_string(),
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
    }))
}
