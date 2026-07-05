//! request_history のトークン利用量集計統計（全体/モデル別/エンドポイント別/日次/月次）
//!
//! arch-review [H6] round2: db/request_history.rs から集計統計を分離。

use super::RequestHistoryStorage;
use crate::common::error::{LbError, RouterResult};
use uuid::Uuid;

/// トークン統計（全体）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryTokenStatistics {
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
}

/// トークン統計（モデル別）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryModelTokenStatistics {
    /// モデル名
    pub model: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// トークン統計（エンドポイント別）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndpointTokenStatistics {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// エンドポイント名
    pub endpoint_name: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// SQLiteから取得したトークン統計行（全体）
#[derive(sqlx::FromRow)]
struct TokenStatisticsRow {
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
}

/// SQLiteから取得したトークン統計行（モデル別）
#[derive(sqlx::FromRow)]
struct ModelTokenStatisticsRow {
    model: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（エンドポイント別）
#[derive(sqlx::FromRow)]
struct EndpointTokenStatisticsRow {
    endpoint_id: String,
    endpoint_name: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（日次）
#[derive(sqlx::FromRow)]
struct DailyTokenStatisticsRow {
    date: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（月次）
#[derive(sqlx::FromRow)]
struct MonthlyTokenStatisticsRow {
    month: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

impl RequestHistoryStorage {
    /// トークン統計を取得（全体）
    pub async fn get_token_statistics(&self) -> RouterResult<HistoryTokenStatistics> {
        let row = sqlx::query_as::<_, TokenStatisticsRow>(
            r#"
            SELECT
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens
            FROM request_history
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get token statistics: {}", e)))?;

        Ok(HistoryTokenStatistics {
            total_input_tokens: row.total_input_tokens as u64,
            total_output_tokens: row.total_output_tokens as u64,
            total_tokens: row.total_tokens as u64,
        })
    }

    /// トークン統計を取得（モデル別）
    pub async fn get_token_statistics_by_model(
        &self,
    ) -> RouterResult<Vec<HistoryModelTokenStatistics>> {
        let rows = sqlx::query_as::<_, ModelTokenStatisticsRow>(
            r#"
            SELECT
                model,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            GROUP BY model
            ORDER BY total_tokens DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            LbError::Database(format!("Failed to get token statistics by model: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .map(|row| HistoryModelTokenStatistics {
                model: row.model,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }

    /// トークン統計を取得（エンドポイント別）
    pub async fn get_token_statistics_by_endpoint(
        &self,
    ) -> RouterResult<Vec<EndpointTokenStatistics>> {
        let rows = sqlx::query_as::<_, EndpointTokenStatisticsRow>(
            r#"
            SELECT
                endpoint_id,
                endpoint_name,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            GROUP BY endpoint_id
            ORDER BY total_tokens DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            LbError::Database(format!("Failed to get token statistics by endpoint: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let endpoint_id = Uuid::parse_str(&row.endpoint_id).ok()?;
                Some(EndpointTokenStatistics {
                    endpoint_id,
                    endpoint_name: row.endpoint_name,
                    total_input_tokens: row.total_input_tokens as u64,
                    total_output_tokens: row.total_output_tokens as u64,
                    total_tokens: row.total_tokens as u64,
                    request_count: row.request_count as u64,
                })
            })
            .collect())
    }

    /// 日次トークン統計を取得
    pub async fn get_daily_token_statistics(
        &self,
        days: u32,
    ) -> RouterResult<Vec<crate::api::dashboard::DailyTokenStats>> {
        let rows = sqlx::query_as::<_, DailyTokenStatisticsRow>(
            r#"
            SELECT
                DATE(timestamp) as date,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            WHERE timestamp >= DATE('now', '-' || ? || ' days')
            GROUP BY DATE(timestamp)
            ORDER BY date DESC
            "#,
        )
        .bind(days as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get daily token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| crate::api::dashboard::DailyTokenStats {
                date: row.date,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }

    /// 月次トークン統計を取得
    pub async fn get_monthly_token_statistics(
        &self,
        months: u32,
    ) -> RouterResult<Vec<crate::api::dashboard::MonthlyTokenStats>> {
        let rows = sqlx::query_as::<_, MonthlyTokenStatisticsRow>(
            r#"
            SELECT
                strftime('%Y-%m', timestamp) as month,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            WHERE timestamp >= DATE('now', '-' || ? || ' months')
            GROUP BY strftime('%Y-%m', timestamp)
            ORDER BY month DESC
            "#,
        )
        .bind(months as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get monthly token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| crate::api::dashboard::MonthlyTokenStats {
                month: row.month,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }
}
