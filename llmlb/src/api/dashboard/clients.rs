//! ダッシュボードのクライアントIP分析ハンドラ
//!
//! arch-review [M1]: api/dashboard.rs の grab-bag 化を抑えるため、Clients タブの
//! 分析ハンドラ群（ランキング/タイムライン/モデル分布/ヒートマップ/詳細/APIキー）を
//! submodule へ切り出した。ルータ登録名を保つため親モジュールで re-export する。

use super::{effective_ip_alert_threshold, ClientsQuery, HeatmapQuery};
use crate::api::error::AppError;
use crate::AppState;
use axum::extract::{Query, State};
use axum::Json;

/// GET /api/dashboard/clients - IPランキング
///
/// SPEC-62ac4b68: Clientsタブ基本分析
pub async fn get_client_rankings(
    Query(params): Query<ClientsQuery>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::request_history::ClientIpRankingResult>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let mut result = storage
        .get_client_ip_ranking(24, params.page, params.per_page, params.ip.as_deref())
        .await
        .map_err(AppError)?;

    // SPEC-62ac4b68: 閾値ベースの異常検知
    // 過去1時間のリクエスト数が閾値以上のIPにis_alert=trueを設定
    let settings = crate::db::settings::SettingsStorage::new(state.db_pool.clone());
    let threshold_raw = settings
        .get_setting("ip_alert_threshold")
        .await
        .map_err(AppError)?;
    let threshold = effective_ip_alert_threshold(threshold_raw.as_deref());

    let one_hour_counts = storage
        .get_ip_request_counts_since(1)
        .await
        .map_err(AppError)?;
    for ranking in &mut result.rankings {
        let count = one_hour_counts.get(&ranking.ip).copied().unwrap_or(0);
        ranking.is_alert = count >= threshold;
    }

    Ok(Json(result))
}

/// GET /api/dashboard/clients/timeline - ユニークIP数タイムライン
///
/// SPEC-62ac4b68: 使用パターンの時系列分析
pub async fn get_client_timeline(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::request_history::UniqueIpTimelinePoint>>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let result = storage.get_unique_ip_timeline(24).await.map_err(AppError)?;
    Ok(Json(result))
}

/// GET /api/dashboard/clients/models - モデル別リクエスト分布
///
/// SPEC-62ac4b68: 使用パターンの時系列分析
pub async fn get_client_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::request_history::ModelDistribution>>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let result = storage
        .get_model_distribution_by_clients(24)
        .await
        .map_err(AppError)?;
    Ok(Json(result))
}

/// GET /api/dashboard/clients/heatmap - リクエストヒートマップ
///
/// SPEC-62ac4b68: 時間帯×曜日ヒートマップ
pub async fn get_client_heatmap(
    Query(params): Query<HeatmapQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::request_history::HeatmapCell>>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let result = storage
        .get_request_heatmap(24 * 7, params.ip.as_deref())
        .await
        .map_err(AppError)?;
    Ok(Json(result))
}

/// GET /api/dashboard/clients/:ip/detail - IPドリルダウン詳細
///
/// SPEC-62ac4b68: IPドリルダウン詳細ビュー
pub async fn get_client_detail(
    axum::extract::Path(ip): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::request_history::ClientDetail>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let result = storage.get_client_detail(&ip, 20).await.map_err(AppError)?;
    Ok(Json(result))
}

/// GET /api/dashboard/clients/{ip}/api-keys - APIキー別集計
///
/// SPEC-62ac4b68: APIキーとのクロス分析
pub async fn get_client_api_keys(
    axum::extract::Path(ip): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::request_history::ClientApiKeyUsage>>, AppError> {
    let storage = crate::db::request_history::RequestHistoryStorage::new(state.db_pool.clone());
    let result = storage.get_client_api_keys(&ip).await.map_err(AppError)?;
    Ok(Json(result))
}
