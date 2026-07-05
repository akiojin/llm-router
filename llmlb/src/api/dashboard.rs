//! ダッシュボードAPIハンドラー
//!
//! `/api/dashboard/*` 系のエンドポイントを提供し、ノードの状態および
//! システム統計を返却する。

use super::error::AppError;
use crate::types::HealthMetrics;
use crate::{balancer::RequestHistoryPoint, types::endpoint::EndpointStatus, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use uuid::Uuid;

mod dto;
pub use dto::*;
mod overview;
mod token_stats;
pub use token_stats::*;
mod endpoint_stats;
pub use endpoint_stats::*;
mod model_catalog;
pub use model_catalog::get_models;
use overview::*;

/// GET /api/dashboard/endpoints
///
/// SPEC-e8e9326e: llmlb主導エンドポイント登録システム
pub async fn get_endpoints(State(state): State<AppState>) -> Json<Vec<DashboardEndpoint>> {
    Json(collect_endpoints(&state).await)
}

/// GET /api/dashboard/stats
pub async fn get_stats(State(state): State<AppState>) -> Json<DashboardStats> {
    Json(collect_stats(&state).await)
}

/// GET /api/dashboard/request-history
pub async fn get_request_history(State(state): State<AppState>) -> Json<Vec<RequestHistoryPoint>> {
    Json(collect_history(&state).await)
}

/// GET /api/dashboard/overview
pub async fn get_overview(State(state): State<AppState>) -> Json<DashboardOverview> {
    let started = Instant::now();
    let endpoints = collect_endpoints(&state).await;
    let stats = collect_stats(&state).await;
    let operation_token_totals = collect_operation_token_totals(&state).await;
    let endpoint_tps = state.load_manager.get_all_endpoint_tps().await;
    let operations = collect_operations(&stats, &endpoints, operation_token_totals, &endpoint_tps);
    let capacity = collect_capacity(&state, &endpoints).await;
    let action_items = collect_action_items(&operations);
    let history = collect_history(&state).await;
    let generation_time_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let generated_at = Utc::now();
    Json(DashboardOverview {
        endpoints,
        operations,
        capacity,
        action_items,
        history,
        endpoint_tps,
        generated_at,
        generation_time_ms,
    })
}

/// GET /api/dashboard/metrics/:runtime_id
pub async fn get_node_metrics(
    Path(endpoint_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<HealthMetrics>>, AppError> {
    let history = state.load_manager.metrics_history(endpoint_id).await?;
    Ok(Json(history))
}

fn default_page() -> usize {
    1
}

/// Clientsランキングのクエリパラメータ
#[derive(Debug, Deserialize)]
pub struct ClientsQuery {
    /// ページ番号（デフォルト: 1）
    #[serde(default = "default_page")]
    pub page: usize,
    /// ページサイズ（デフォルト: 20）
    #[serde(default = "default_clients_per_page")]
    pub per_page: usize,
    /// IPアドレスプリフィックス（前方一致フィルタ）
    pub ip: Option<String>,
}

fn default_clients_per_page() -> usize {
    20
}

/// ヒートマップ取得のクエリパラメーター
#[derive(serde::Deserialize)]
pub struct HeatmapQuery {
    /// IPアドレスプリフィックス（前方一致フィルタ）
    pub ip: Option<String>,
}

mod request_history;
pub use request_history::*;
mod settings;
pub(crate) use settings::effective_ip_alert_threshold;
pub use settings::{get_setting, update_setting, SettingUpdateBody};
#[cfg(test)]
use settings::{
    parse_ip_alert_threshold, IP_ALERT_THRESHOLD_DEFAULT_VALUE, IP_ALERT_THRESHOLD_MIN,
};
mod clients;
pub use clients::{
    get_client_api_keys, get_client_detail, get_client_heatmap, get_client_models,
    get_client_rankings, get_client_timeline,
};

// NOTE: テストは NodeRegistry → EndpointRegistry 移行完了後に再実装
// 関連: SPEC-e8e9326e

#[cfg(test)]
mod tests;
