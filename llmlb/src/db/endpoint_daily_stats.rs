//! エンドポイント日次統計データベース操作
//!
//! SPEC-8c32349f: エンドポイント単位リクエスト統計
//! endpoint_daily_stats テーブルへのCRUD操作を提供する。
//! 日付はサーバーローカル時間 (chrono::Local::now().format("%Y-%m-%d").to_string()) を使用。

use sqlx::SqlitePool;
use uuid::Uuid;

mod queries;
mod seeding;
pub use queries::{get_all_model_stats, get_daily_stats, get_model_stats, get_today_stats};
pub use seeding::{get_today_stats_all, TpsSeedEntry};

/// 日次集計エントリ（日付ごとの集計結果）
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyStatEntry {
    /// 日付（YYYY-MM-DD形式）
    pub date: String,
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
}

/// モデル別集計エントリ（モデルごとの集計結果）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelStatEntry {
    /// モデルID
    pub model_id: String,
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
    /// 出力トークン累計（SPEC-4bb5b55f）
    pub total_output_tokens: i64,
    /// 処理時間累計（ミリ秒、SPEC-4bb5b55f）
    pub total_duration_ms: i64,
}

/// 日次統計をUPSERT（挿入または更新）
///
/// 指定のエンドポイント・モデル・日付の組み合わせでレコードが存在しない場合は新規挿入、
/// 存在する場合はカウンタをインクリメントする。
pub async fn upsert_daily_stats(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
    date: &str,
    success: bool,
    output_tokens: u64,
    duration_ms: u64,
) -> Result<(), sqlx::Error> {
    upsert_daily_stats_with_api_kind(
        pool,
        endpoint_id,
        model_id,
        date,
        "chat_completions",
        success,
        output_tokens,
        duration_ms,
    )
    .await
}

/// api_kind指定付きの日次統計UPSERT
#[allow(clippy::too_many_arguments)]
pub async fn upsert_daily_stats_with_api_kind(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
    date: &str,
    api_kind: &str,
    success: bool,
    output_tokens: u64,
    duration_ms: u64,
) -> Result<(), sqlx::Error> {
    let success_increment: i64 = if success { 1 } else { 0 };
    let failure_increment: i64 = if success { 0 } else { 1 };

    sqlx::query(
        r#"
        INSERT INTO endpoint_daily_stats (endpoint_id, model_id, date, api_kind, total_requests, successful_requests, failed_requests, total_output_tokens, total_duration_ms)
        VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?)
        ON CONFLICT(endpoint_id, model_id, date, api_kind) DO UPDATE SET
            total_requests = total_requests + 1,
            successful_requests = successful_requests + excluded.successful_requests,
            failed_requests = failed_requests + excluded.failed_requests,
            total_output_tokens = total_output_tokens + excluded.total_output_tokens,
            total_duration_ms = total_duration_ms + excluded.total_duration_ms
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(model_id)
    .bind(date)
    .bind(api_kind)
    .bind(success_increment)
    .bind(failure_increment)
    .bind(output_tokens as i64)
    .bind(duration_ms as i64)
    .execute(pool)
    .await?;

    Ok(())
}

/// 日次統計バッチタスクを開始（SPEC-8c32349f）
///
/// サーバーローカル時間の0:00に前日分の統計をログ出力する。
/// リアルタイムUPSERTで統計は更新済みのため、
/// このタスクは日次マーカーとログ記録の役割を担う。
pub fn start_daily_stats_task(pool: SqlitePool) {
    tokio::spawn(async move {
        loop {
            // 次の0:00までスリープ
            let now = chrono::Local::now();
            let tomorrow = (now + chrono::Duration::days(1))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("valid midnight time");
            let tomorrow = tomorrow
                .and_local_timezone(chrono::Local)
                .single()
                .unwrap_or_else(|| {
                    (now + chrono::Duration::days(1))
                        .date_naive()
                        .and_hms_opt(0, 0, 1)
                        .expect("valid midnight+1s")
                        .and_local_timezone(chrono::Local)
                        .latest()
                        .expect("valid local time")
                });
            let sleep_duration = (tomorrow - now).to_std().unwrap_or_default();
            tokio::time::sleep(sleep_duration).await;

            let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            tracing::info!("Daily stats batch: finalizing {}", yesterday);

            // 前日分のレコード数をログ出力
            match sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM endpoint_daily_stats WHERE date = ?",
            )
            .bind(&yesterday)
            .fetch_one(&pool)
            .await
            {
                Ok(count) => {
                    tracing::info!(
                        "Daily stats batch complete: {} records for {}",
                        count,
                        yesterday
                    );
                }
                Err(e) => {
                    tracing::error!("Daily stats batch failed: {}", e);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
