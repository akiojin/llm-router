//! トークン利用量の集計統計（全体/モデル別/日次/月次）
//!
//! arch-review [H6]: db/audit_log.rs から集計統計の型とクエリメソッドを分離。
//! AuditLogStorage の inherent impl を分割し、親は集計出力型を pub use で再エクスポートする。

use super::AuditLogStorage;
use crate::common::error::{LbError, RouterResult};
use serde::{Deserialize, Serialize};

/// トークン全体統計
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatistics {
    /// 入力トークン合計
    pub total_input_tokens: i64,
    /// 出力トークン合計
    pub total_output_tokens: i64,
    /// 総トークン合計
    pub total_tokens: i64,
}

/// モデル別トークン統計
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTokenStatistics {
    /// モデル名
    pub model_name: String,
    /// 入力トークン合計
    pub total_input_tokens: i64,
    /// 出力トークン合計
    pub total_output_tokens: i64,
    /// 総トークン合計
    pub total_tokens: i64,
}

/// 日次トークン統計
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyTokenStatistics {
    /// 日付（YYYY-MM-DD）
    pub date: String,
    /// 入力トークン合計
    pub total_input_tokens: i64,
    /// 出力トークン合計
    pub total_output_tokens: i64,
    /// 総トークン合計
    pub total_tokens: i64,
    /// リクエスト数
    pub request_count: i64,
}

/// 月次トークン統計
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyTokenStatistics {
    /// 月（YYYY-MM）
    pub month: String,
    /// 入力トークン合計
    pub total_input_tokens: i64,
    /// 出力トークン合計
    pub total_output_tokens: i64,
    /// 総トークン合計
    pub total_tokens: i64,
    /// リクエスト数
    pub request_count: i64,
}

/// sqlx::FromRow用の行構造体（トークン全体統計）
#[derive(Debug, sqlx::FromRow)]
struct TokenStatisticsRow {
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
}

/// sqlx::FromRow用の行構造体（モデル別トークン統計）
#[derive(Debug, sqlx::FromRow)]
struct ModelTokenStatisticsRow {
    model_name: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
}

/// sqlx::FromRow用の行構造体（日次トークン統計）
#[derive(Debug, sqlx::FromRow)]
struct DailyTokenStatisticsRow {
    date: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// sqlx::FromRow用の行構造体（月次トークン統計）
#[derive(Debug, sqlx::FromRow)]
struct MonthlyTokenStatisticsRow {
    month: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

impl AuditLogStorage {
    /// トークン全体統計を取得
    pub async fn get_token_statistics(&self) -> RouterResult<TokenStatistics> {
        let row = sqlx::query_as::<_, TokenStatisticsRow>(
            r#"SELECT
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(
                    COALESCE(total_tokens, COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0))
                ), 0) as total_tokens
            FROM audit_log_entries"#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get token statistics: {}", e)))?;

        Ok(TokenStatistics {
            total_input_tokens: row.total_input_tokens,
            total_output_tokens: row.total_output_tokens,
            total_tokens: row.total_tokens,
        })
    }

    /// モデル別トークン統計を取得
    pub async fn get_token_statistics_by_model(&self) -> RouterResult<Vec<ModelTokenStatistics>> {
        let rows = sqlx::query_as::<_, ModelTokenStatisticsRow>(
            r#"SELECT
                model_name,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(
                    COALESCE(total_tokens, COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0))
                ), 0) as total_tokens
            FROM audit_log_entries
            WHERE model_name IS NOT NULL
            GROUP BY model_name
            ORDER BY total_tokens DESC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            LbError::Database(format!("Failed to get token statistics by model: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .map(|row| ModelTokenStatistics {
                model_name: row.model_name,
                total_input_tokens: row.total_input_tokens,
                total_output_tokens: row.total_output_tokens,
                total_tokens: row.total_tokens,
            })
            .collect())
    }

    /// 日次トークン統計を取得
    pub async fn get_daily_token_statistics(
        &self,
        days: i64,
    ) -> RouterResult<Vec<DailyTokenStatistics>> {
        let rows = sqlx::query_as::<_, DailyTokenStatisticsRow>(
            r#"SELECT
                DATE(timestamp) as date,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(
                    COALESCE(total_tokens, COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0))
                ), 0) as total_tokens,
                COUNT(*) as request_count
            FROM audit_log_entries
            WHERE timestamp >= DATE('now', '-' || ? || ' days')
            GROUP BY DATE(timestamp)
            ORDER BY date DESC"#,
        )
        .bind(days)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get daily token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| DailyTokenStatistics {
                date: row.date,
                total_input_tokens: row.total_input_tokens,
                total_output_tokens: row.total_output_tokens,
                total_tokens: row.total_tokens,
                request_count: row.request_count,
            })
            .collect())
    }

    /// 月次トークン統計を取得
    pub async fn get_monthly_token_statistics(
        &self,
        months: i64,
    ) -> RouterResult<Vec<MonthlyTokenStatistics>> {
        let rows = sqlx::query_as::<_, MonthlyTokenStatisticsRow>(
            r#"SELECT
                strftime('%Y-%m', timestamp) as month,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(
                    COALESCE(total_tokens, COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0))
                ), 0) as total_tokens,
                COUNT(*) as request_count
            FROM audit_log_entries
            WHERE timestamp >= DATE('now', '-' || ? || ' months')
            GROUP BY strftime('%Y-%m', timestamp)
            ORDER BY month DESC"#,
        )
        .bind(months)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get monthly token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| MonthlyTokenStatistics {
                month: row.month,
                total_input_tokens: row.total_input_tokens,
                total_output_tokens: row.total_output_tokens,
                total_tokens: row.total_tokens,
                request_count: row.request_count,
            })
            .collect())
    }
}
