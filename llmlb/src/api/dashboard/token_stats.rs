//! トークン利用量統計エンドポイント（全体/日次/月次）
//!
//! arch-review [H6]: api/dashboard.rs からトークン統計ハンドラと DTO を分離。

use super::*;

/// GET /api/dashboard/stats/tokens - トークン統計取得
///
/// NOTE: request_history廃止完了まで request_history を集計元として扱う
pub async fn get_token_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::db::request_history::HistoryTokenStatistics>, AppError> {
    let stats = state
        .request_history
        .get_token_statistics()
        .await
        .map_err(AppError::from)?;
    Ok(Json(stats))
}

/// 日次トークン統計クエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct DailyTokenStatsQuery {
    /// 取得する日数（デフォルト: 30）
    #[serde(default = "default_days")]
    pub days: Option<u32>,
}

pub(crate) fn default_days() -> Option<u32> {
    Some(30)
}

/// 日次トークン統計レスポンス
#[derive(Debug, Clone, Serialize)]
pub struct DailyTokenStats {
    /// 日付（YYYY-MM-DD形式）
    pub date: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// GET /api/dashboard/stats/tokens/daily - 日次トークン統計取得
///
/// NOTE: request_history廃止完了まで request_history を集計元として扱う
pub async fn get_daily_token_stats(
    State(state): State<AppState>,
    Query(query): Query<DailyTokenStatsQuery>,
) -> Result<Json<Vec<DailyTokenStats>>, AppError> {
    let days = query.days.unwrap_or(30);
    let stats = state
        .request_history
        .get_daily_token_statistics(days)
        .await
        .map_err(AppError::from)?;
    Ok(Json(
        stats
            .into_iter()
            .map(|s| DailyTokenStats {
                date: s.date,
                total_input_tokens: s.total_input_tokens,
                total_output_tokens: s.total_output_tokens,
                total_tokens: s.total_tokens,
                request_count: s.request_count,
            })
            .collect(),
    ))
}

/// 月次トークン統計クエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct MonthlyTokenStatsQuery {
    /// 取得する月数（デフォルト: 12）
    #[serde(default = "default_months")]
    pub months: Option<u32>,
}

pub(crate) fn default_months() -> Option<u32> {
    Some(12)
}

/// 月次トークン統計レスポンス
#[derive(Debug, Clone, Serialize)]
pub struct MonthlyTokenStats {
    /// 月（YYYY-MM形式）
    pub month: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// GET /api/dashboard/stats/tokens/monthly - 月次トークン統計取得
///
/// NOTE: request_history廃止完了まで request_history を集計元として扱う
pub async fn get_monthly_token_stats(
    State(state): State<AppState>,
    Query(query): Query<MonthlyTokenStatsQuery>,
) -> Result<Json<Vec<MonthlyTokenStats>>, AppError> {
    let months = query.months.unwrap_or(12);
    let stats = state
        .request_history
        .get_monthly_token_statistics(months)
        .await
        .map_err(AppError::from)?;
    Ok(Json(
        stats
            .into_iter()
            .map(|s| MonthlyTokenStats {
                month: s.month,
                total_input_tokens: s.total_input_tokens,
                total_output_tokens: s.total_output_tokens,
                total_tokens: s.total_tokens,
                request_count: s.request_count,
            })
            .collect(),
    ))
}
