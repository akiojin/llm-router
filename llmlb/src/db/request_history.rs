//! リクエスト/レスポンス履歴のストレージ層
//!
//! SQLiteベースでリクエスト履歴を永続化（load balancer.dbと統合）

#[cfg(test)]
use crate::common::protocol::RecordStatus;
use crate::common::{
    error::{LbError, RouterResult},
    protocol::RequestResponseRecord,
};
use crate::config::get_env_with_fallback_parse;
#[cfg(test)]
use chrono::DateTime;
use chrono::{Duration, Utc};
use sqlx::SqlitePool;
#[cfg(test)]
use std::net::IpAddr;
use std::sync::Arc;
use uuid::Uuid;

mod row;
use row::RequestHistoryRow;
mod legacy;
#[cfg(test)]
use legacy::*;
mod analytics;
mod filter;
mod ingest;
mod statistics;
pub use analytics::{
    ClientApiKeyUsage, ClientDetail, ClientIpRanking, ClientIpRankingResult, ClientRecentRequest,
    HeatmapCell, HourlyPattern, ModelDistribution, UniqueIpTimelinePoint,
};
pub use filter::{FilterStatus, FilteredRecords, RecordFilter};
pub use statistics::{
    EndpointTokenStatistics, HistoryModelTokenStatistics, HistoryTokenStatistics,
};

const REQUEST_HISTORY_RETENTION_DAYS_ENV: &str = "LLMLB_REQUEST_HISTORY_RETENTION_DAYS";
const LEGACY_REQUEST_HISTORY_RETENTION_DAYS_ENV: &str = "REQUEST_HISTORY_RETENTION_DAYS";
const REQUEST_HISTORY_CLEANUP_INTERVAL_ENV: &str = "LLMLB_REQUEST_HISTORY_CLEANUP_INTERVAL_SECS";
const LEGACY_REQUEST_HISTORY_CLEANUP_INTERVAL_ENV: &str = "REQUEST_HISTORY_CLEANUP_INTERVAL_SECS";

/// リクエスト履歴ストレージ（SQLite版）
#[derive(Clone)]
pub struct RequestHistoryStorage {
    pool: SqlitePool,
}

impl RequestHistoryStorage {
    /// 新しいストレージインスタンスを作成
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// IDでレコードを取得
    pub async fn get_record_by_id(&self, id: Uuid) -> RouterResult<Option<RequestResponseRecord>> {
        let row = sqlx::query_as::<_, RequestHistoryRow>(
            "SELECT * FROM request_history WHERE id = ? LIMIT 1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to load record: {}", e)))?;

        match row {
            Some(row) => Ok(Some(row.try_into()?)),
            None => Ok(None),
        }
    }

    /// すべてのレコードを読み込み（タイムスタンプ降順）
    pub async fn load_records(&self) -> RouterResult<Vec<RequestResponseRecord>> {
        let rows = sqlx::query_as::<_, RequestHistoryRow>(
            "SELECT * FROM request_history ORDER BY timestamp DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to load records: {}", e)))?;

        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// 指定期間より古いレコードを削除
    pub async fn cleanup_old_records(&self, max_age: Duration) -> RouterResult<()> {
        let cutoff = (Utc::now() - max_age).to_rfc3339();

        sqlx::query("DELETE FROM request_history WHERE timestamp < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to cleanup records: {}", e)))?;

        Ok(())
    }

    /// 直近N分のリクエスト履歴を分単位で集計して返す（起動時seeding用）
    pub async fn get_recent_history_by_minute(
        &self,
        minutes: i64,
    ) -> RouterResult<Vec<MinuteHistoryPoint>> {
        let cutoff = (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339();

        let rows = sqlx::query_as::<_, MinuteHistoryRow>(
            r#"
            SELECT
                strftime('%Y-%m-%dT%H:%M:00Z', completed_at) AS minute,
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN status != 'success' THEN 1 ELSE 0 END) AS error_count
            FROM request_history
            WHERE completed_at >= ?
            GROUP BY strftime('%Y-%m-%dT%H:%M:00Z', completed_at)
            ORDER BY minute ASC
            "#,
        )
        .bind(&cutoff)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get recent history by minute: {}", e)))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

/// 分単位の履歴集計ポイント（起動時seeding用）
#[derive(Debug, Clone)]
pub struct MinuteHistoryPoint {
    /// 分（UTC ISO8601形式）
    pub minute: String,
    /// 成功リクエスト数
    pub success_count: i64,
    /// 失敗リクエスト数
    pub error_count: i64,
}

#[derive(sqlx::FromRow)]
struct MinuteHistoryRow {
    minute: String,
    success_count: i64,
    error_count: i64,
}

impl From<MinuteHistoryRow> for MinuteHistoryPoint {
    fn from(row: MinuteHistoryRow) -> Self {
        MinuteHistoryPoint {
            minute: row.minute,
            success_count: row.success_count,
            error_count: row.error_count,
        }
    }
}

/// 定期クリーンアップタスクを開始
pub fn start_cleanup_task(storage: Arc<RequestHistoryStorage>) {
    let retention_days = get_env_with_fallback_parse(
        REQUEST_HISTORY_RETENTION_DAYS_ENV,
        LEGACY_REQUEST_HISTORY_RETENTION_DAYS_ENV,
        7i64,
    );
    let interval_secs = get_env_with_fallback_parse(
        REQUEST_HISTORY_CLEANUP_INTERVAL_ENV,
        LEGACY_REQUEST_HISTORY_CLEANUP_INTERVAL_ENV,
        3600u64,
    );

    if retention_days <= 0 {
        tracing::info!("Request history cleanup disabled ({} <= 0)", retention_days);
        return;
    }

    tokio::spawn(async move {
        // 起動時に1回実行
        let retention = Duration::days(retention_days);
        if let Err(e) = storage.cleanup_old_records(retention).await {
            tracing::error!("Initial cleanup failed: {}", e);
        }

        // 1時間ごとに実行
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;

            if let Err(e) = storage.cleanup_old_records(retention).await {
                tracing::error!("Periodic cleanup failed: {}", e);
            } else {
                tracing::info!("Periodic cleanup completed");
            }
        }
    });
}

#[cfg(test)]
mod tests;
