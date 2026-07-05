//! 起動時 TPS シーディング用の当日統計取得。
//!
//! 全エンドポイントの当日 TPS 関連データと、そのシード用 DTO を提供する。

use sqlx::SqlitePool;
use uuid::Uuid;

/// 当日の全エンドポイントのTPS関連データを取得（起動時seeding用）
pub async fn get_today_stats_all(
    pool: &SqlitePool,
    today: &str,
) -> Result<Vec<TpsSeedEntry>, sqlx::Error> {
    let rows = sqlx::query_as::<_, TpsSeedRow>(
        r#"
        SELECT
            endpoint_id,
            model_id,
            api_kind,
            total_output_tokens,
            total_duration_ms,
            successful_requests
        FROM endpoint_daily_stats
        WHERE date = ?
          AND total_output_tokens > 0
          AND total_duration_ms > 0
        "#,
    )
    .bind(today)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// TPS seeding用のエントリ
#[derive(Debug, Clone)]
pub struct TpsSeedEntry {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// モデルID
    pub model_id: String,
    /// API種別（chat_completions/completions/responses）
    pub api_kind: String,
    /// 出力トークン累計
    pub total_output_tokens: i64,
    /// 処理時間累計（ミリ秒）
    pub total_duration_ms: i64,
    /// TPS対象リクエスト数（成功リクエストのみ）
    pub successful_requests: i64,
}

#[derive(sqlx::FromRow)]
struct TpsSeedRow {
    endpoint_id: String,
    model_id: String,
    api_kind: String,
    total_output_tokens: i64,
    total_duration_ms: i64,
    successful_requests: i64,
}

impl From<TpsSeedRow> for TpsSeedEntry {
    fn from(row: TpsSeedRow) -> Self {
        TpsSeedEntry {
            endpoint_id: Uuid::parse_str(&row.endpoint_id).unwrap_or_default(),
            model_id: row.model_id,
            api_kind: row.api_kind,
            total_output_tokens: row.total_output_tokens,
            total_duration_ms: row.total_duration_ms,
            successful_requests: row.successful_requests,
        }
    }
}
