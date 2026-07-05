//! エンドポイント別リクエスト統計（当日/日次/モデル別）とモデル別 TPS
//!
//! arch-review [H6]: api/dashboard.rs からエンドポイント統計ハンドラと DTO を分離。

use super::*;

/// GET /api/endpoints/{id}/today-stats - 当日リクエスト統計
///
/// SPEC-8c32349f: エンドポイント単位リクエスト統計 (Phase 5)
pub async fn get_endpoint_today_stats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::endpoint_daily_stats::DailyStatEntry>, AppError> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let stats = crate::db::endpoint_daily_stats::get_today_stats(&state.db_pool, id, &today)
        .await
        .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;
    Ok(Json(stats))
}

/// GET /api/endpoints/{id}/daily-stats - 日次リクエスト統計
///
/// SPEC-8c32349f: エンドポイント単位リクエスト統計 (Phase 6)
pub async fn get_endpoint_daily_stats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    Query(query): Query<EndpointDailyStatsQuery>,
) -> Result<Json<Vec<crate::db::endpoint_daily_stats::DailyStatEntry>>, AppError> {
    let days = query.days.unwrap_or(7).min(365);
    let stats = crate::db::endpoint_daily_stats::get_daily_stats(&state.db_pool, id, days)
        .await
        .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;
    Ok(Json(stats))
}

/// エンドポイント日次統計クエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct EndpointDailyStatsQuery {
    /// 取得する日数（デフォルト: 7、最大: 365）
    #[serde(default)]
    pub days: Option<u32>,
}

/// GET /api/endpoints/{id}/model-stats - モデル別リクエスト統計
///
/// SPEC-8c32349f: エンドポイント単位リクエスト統計 (Phase 7)
pub async fn get_endpoint_model_stats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::endpoint_daily_stats::ModelStatEntry>>, AppError> {
    let stats = crate::db::endpoint_daily_stats::get_model_stats(&state.db_pool, id)
        .await
        .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;
    Ok(Json(stats))
}

/// GET /api/dashboard/all-model-stats - 全エンドポイント横断のモデル別統計
///
/// SPEC-8c32349f: ダッシュボード向けモデル別集計
pub async fn get_all_model_stats(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::endpoint_daily_stats::ModelStatEntry>>, AppError> {
    let stats = crate::db::endpoint_daily_stats::get_all_model_stats(&state.db_pool)
        .await
        .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;
    Ok(Json(stats))
}

/// エンドポイント×モデル単位のTPS情報（SPEC-4bb5b55f）
#[derive(Debug, Clone, Serialize)]
pub struct ModelTpsEntry {
    /// モデルID
    pub model_id: String,
    /// API種別（chat/completions/responses）
    pub api_kind: crate::common::protocol::TpsApiKind,
    /// 計測元（production / benchmark）
    pub source: crate::common::protocol::TpsSource,
    /// EMA平滑化されたTPS値（None=未計測）
    pub tps: Option<f64>,
    /// リクエスト完了数
    pub request_count: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 平均処理時間（ミリ秒、None=未計測）
    pub average_duration_ms: Option<f64>,
}

/// GET /api/endpoints/{id}/model-tps - エンドポイント×モデル単位のTPS情報
///
/// SPEC-4bb5b55f: エンドポイント×モデル単位TPS可視化 (Phase 3)
pub async fn get_endpoint_model_tps(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Json<Vec<ModelTpsEntry>> {
    let tps_list = state.load_manager.get_model_tps(id).await;
    Json(
        tps_list
            .into_iter()
            .map(|info| ModelTpsEntry {
                model_id: info.model_id,
                api_kind: info.api_kind,
                source: info.source,
                tps: info.tps,
                request_count: info.request_count,
                total_output_tokens: info.total_output_tokens,
                average_duration_ms: info.average_duration_ms,
            })
            .collect(),
    )
}
