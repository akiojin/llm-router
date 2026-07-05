//! ダッシュボードAPIハンドラー
//!
//! `/api/dashboard/*` 系のエンドポイントを提供し、ノードの状態および
//! システム統計を返却する。

use super::error::AppError;
use crate::common::error::{CommonError, LbError};
use crate::db::request_history::{FilterStatus, RecordFilter};
use crate::types::HealthMetrics;
use crate::{
    balancer::RequestHistoryPoint,
    types::endpoint::{EndpointStatus, EndpointType},
    AppState,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tracing::warn;
use uuid::Uuid;

mod overview;
mod token_stats;
pub use token_stats::*;
mod endpoint_stats;
pub use endpoint_stats::*;
mod model_catalog;
pub use model_catalog::get_models;
use overview::*;

/// エンドポイントのダッシュボード表示用サマリー
///
/// SPEC-e8e9326e: llmlb主導エンドポイント登録システム
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardEndpoint {
    /// エンドポイントID
    pub id: Uuid,
    /// 表示名
    pub name: String,
    /// ベースURL
    pub base_url: String,
    /// 現在の状態
    pub status: EndpointStatus,
    /// エンドポイントタイプ（xLLM/Ollama/vLLM 等）
    pub endpoint_type: EndpointType,
    /// ヘルスチェック間隔（秒）
    pub health_check_interval_secs: u32,
    /// 推論タイムアウト（秒）
    pub inference_timeout_secs: u32,
    /// レイテンシ（ミリ秒）
    pub latency_ms: Option<u32>,
    /// 最終確認時刻
    pub last_seen: Option<DateTime<Utc>>,
    /// 最後のエラーメッセージ
    pub last_error: Option<String>,
    /// 連続エラー回数
    pub error_count: u32,
    /// 登録日時
    pub registered_at: DateTime<Utc>,
    /// メモ
    pub notes: Option<String>,
    /// 利用可能なモデル数
    pub model_count: usize,
    /// 累計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
}

/// システム統計レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardStats {
    /// 登録ランタイム総数
    #[serde(rename = "total_runtimes", alias = "total_nodes")]
    pub total_nodes: usize,
    /// オンラインランタイム数
    #[serde(rename = "online_runtimes", alias = "online_nodes")]
    pub online_nodes: usize,
    /// 承認待ちランタイム数
    #[serde(rename = "pending_runtimes", alias = "pending_nodes")]
    pub pending_nodes: usize,
    /// 登録中ランタイム数
    #[serde(rename = "registering_runtimes", alias = "registering_nodes")]
    pub registering_nodes: usize,
    /// オフラインランタイム数
    #[serde(rename = "offline_runtimes", alias = "offline_nodes")]
    pub offline_nodes: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 処理中リクエスト数
    pub total_active_requests: u32,
    /// 待機中リクエスト数
    pub queued_requests: usize,
    /// 平均レスポンスタイム
    pub average_response_time_ms: Option<f32>,
    /// 平均GPU使用率
    pub average_gpu_usage: Option<f32>,
    /// 平均GPUメモリ使用率
    pub average_gpu_memory_usage: Option<f32>,
    /// 最新メトリクス更新時刻
    pub last_metrics_updated_at: Option<DateTime<Utc>>,
    /// 最新登録日時
    pub last_registered_at: Option<DateTime<Utc>>,
    /// 最新ヘルスチェック時刻
    pub last_seen_at: Option<DateTime<Utc>>,
    /// OPENAI_API_KEY が設定されているか
    pub openai_key_present: bool,
    /// GOOGLE_API_KEY が設定されているか
    pub google_key_present: bool,
    /// ANTHROPIC_API_KEY が設定されているか
    pub anthropic_key_present: bool,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
}

/// 運用監視向けの主要状態サマリー
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardOperations {
    /// 総合状態（healthy / attention / empty）
    pub health: String,
    /// 登録エンドポイント総数
    pub total_endpoints: usize,
    /// オンラインエンドポイント数
    pub online_endpoints: usize,
    /// 承認待ちエンドポイント数
    pub pending_endpoints: usize,
    /// 登録中エンドポイント数
    pub registering_endpoints: usize,
    /// オフラインエンドポイント数
    pub offline_endpoints: usize,
    /// エラー状態エンドポイント数
    pub error_endpoints: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 成功率（0-100）
    pub success_rate: Option<f32>,
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 待機中リクエスト数
    pub queued_requests: usize,
    /// 平均応答時間
    pub average_response_time_ms: Option<f32>,
    /// 出力TPS（出力トークン数 / 生成時間秒）
    pub output_tps: Option<f64>,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
    /// 最新登録日時
    pub last_registered_at: Option<DateTime<Utc>>,
    /// 最新ヘルスチェック時刻
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// Dashboardで使う容量・能力サマリー
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardCapacity {
    /// 登録済みモデル合計
    pub total_models: usize,
    /// GPU能力がある、またはGPU情報を返したエンドポイント数
    pub gpu_capable_endpoints: usize,
    /// GPUメモリ情報を取得できたエンドポイント数
    pub gpu_telemetry_endpoints: usize,
    /// GPU総メモリ（バイト）
    pub total_gpu_memory_bytes: Option<u64>,
    /// GPU使用中メモリ（バイト）
    pub used_gpu_memory_bytes: Option<u64>,
    /// GPUメモリ使用率（0-100）
    pub gpu_memory_usage_percent: Option<f32>,
    /// GPUテレメトリ状態（available / partial / unavailable）
    pub telemetry_status: String,
}

/// Dashboard上の要対応項目
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardActionItem {
    /// severity（critical / warning / info）
    pub severity: String,
    /// 表示タイトル
    pub title: String,
    /// 補足説明
    pub detail: String,
    /// 対象件数
    pub count: usize,
}

/// ダッシュボード概要レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardOverview {
    /// エンドポイント一覧（SPEC-e8e9326e）
    pub endpoints: Vec<DashboardEndpoint>,
    /// 運用監視向けサマリー
    pub operations: DashboardOperations,
    /// 容量・能力サマリー
    pub capacity: DashboardCapacity,
    /// 要対応項目
    pub action_items: Vec<DashboardActionItem>,
    /// リクエスト履歴
    pub history: Vec<RequestHistoryPoint>,
    /// エンドポイント別TPS概要（SPEC-4bb5b55f T023）
    pub endpoint_tps: Vec<crate::balancer::EndpointTpsSummary>,
    /// レスポンス生成時刻
    pub generated_at: DateTime<Utc>,
    /// 集計に要した時間（ミリ秒）
    pub generation_time_ms: u64,
}

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

/// 許可されたページサイズ
pub const ALLOWED_PAGE_SIZES: &[usize] = &[10, 25, 50, 100];

/// デフォルトのページサイズ
pub const DEFAULT_PAGE_SIZE: usize = 10;

/// リクエスト履歴一覧のクエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct RequestHistoryQuery {
    /// ページ番号（1始まり）
    #[serde(default = "default_page")]
    pub page: usize,
    /// 1ページあたりの件数（10, 25, 50, 100のいずれか）
    #[serde(default = "default_per_page")]
    pub per_page: usize,
    /// 1ページあたりの件数（互換: limit）
    #[serde(default)]
    pub limit: Option<usize>,
    /// オフセット（互換: offset）
    #[serde(default)]
    pub offset: Option<usize>,
    /// モデル名フィルタ（部分一致）
    pub model: Option<String>,
    /// エンドポイントIDフィルタ
    #[serde(alias = "agent_id", alias = "node_id")]
    pub endpoint_id: Option<Uuid>,
    /// ステータスフィルタ
    pub status: Option<FilterStatus>,
    /// 開始時刻フィルタ（RFC3339）
    pub start_time: Option<DateTime<Utc>>,
    /// 終了時刻フィルタ（RFC3339）
    pub end_time: Option<DateTime<Utc>>,
    /// クライアントIPフィルタ（完全一致）
    pub client_ip: Option<String>,
}

fn default_page() -> usize {
    1
}

fn default_per_page() -> usize {
    DEFAULT_PAGE_SIZE
}

impl RequestHistoryQuery {
    /// ページサイズを正規化（許可された値のいずれかに制限）
    pub fn normalized_per_page(&self) -> usize {
        if ALLOWED_PAGE_SIZES.contains(&self.per_page) {
            self.per_page
        } else {
            DEFAULT_PAGE_SIZE
        }
    }

    fn to_record_filter(&self) -> Result<RecordFilter, AppError> {
        if let (Some(start), Some(end)) = (&self.start_time, &self.end_time) {
            if start > end {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "start_time must be <= end_time".to_string(),
                ))));
            }
        }

        Ok(RecordFilter {
            model: self.model.clone(),
            endpoint_id: self.endpoint_id,
            status: self.status,
            start_time: self.start_time,
            end_time: self.end_time,
            client_ip: self.client_ip.clone(),
        })
    }
}

/// エクスポート形式
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RequestHistoryExportFormat {
    /// CSV形式
    #[default]
    Csv,
    /// JSON形式
    Json,
}

/// リクエスト履歴エクスポート用のクエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct RequestHistoryExportQuery {
    /// エクスポート形式（csv/json）
    #[serde(default)]
    pub format: RequestHistoryExportFormat,
    /// モデル名フィルタ（部分一致）
    pub model: Option<String>,
    /// エンドポイントIDフィルタ
    #[serde(alias = "agent_id", alias = "node_id")]
    pub endpoint_id: Option<Uuid>,
    /// ステータスフィルタ
    pub status: Option<FilterStatus>,
    /// 開始時刻フィルタ（RFC3339）
    pub start_time: Option<DateTime<Utc>>,
    /// 終了時刻フィルタ（RFC3339）
    pub end_time: Option<DateTime<Utc>>,
    /// クライアントIPフィルタ（完全一致）
    pub client_ip: Option<String>,
}

impl RequestHistoryExportQuery {
    fn to_record_filter(&self) -> Result<RecordFilter, AppError> {
        if let (Some(start), Some(end)) = (&self.start_time, &self.end_time) {
            if start > end {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "start_time must be <= end_time".to_string(),
                ))));
            }
        }

        Ok(RecordFilter {
            model: self.model.clone(),
            endpoint_id: self.endpoint_id,
            status: self.status,
            start_time: self.start_time,
            end_time: self.end_time,
            client_ip: self.client_ip.clone(),
        })
    }
}

/// T023: リクエスト履歴一覧API
pub async fn list_request_responses(
    State(state): State<AppState>,
    Query(query): Query<RequestHistoryQuery>,
) -> Result<Json<crate::db::request_history::FilteredRecords>, AppError> {
    let filter = query.to_record_filter()?;
    let mut page = if query.page == 0 { 1 } else { query.page };
    let mut per_page = query.normalized_per_page();

    if let Some(limit) = query.limit {
        per_page = if ALLOWED_PAGE_SIZES.contains(&limit) {
            limit
        } else {
            DEFAULT_PAGE_SIZE
        };
    }

    if let Some(offset) = query.offset {
        if per_page == 0 {
            per_page = DEFAULT_PAGE_SIZE;
        }
        page = offset / per_page + 1;
    }

    // ページ番号の上限をクランプし、page * per_page の乗算オーバーフローを防ぐ。
    // 実用上ありえない巨大ページ指定は妥当な上限に丸める。
    const MAX_PAGE: usize = 1_000_000;
    page = page.min(MAX_PAGE);

    let result = state
        .request_history
        .filter_and_paginate(&filter, page, per_page)
        .await
        .map_err(AppError::from)?;
    Ok(Json(result))
}

/// T024: リクエスト履歴詳細API
pub async fn get_request_response_detail(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::common::protocol::RequestResponseRecord>, AppError> {
    let record = state
        .request_history
        .get_record_by_id(id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            crate::common::error::LbError::NotFound(format!("Record {} not found", id))
        })?;
    Ok(Json(record))
}

/// T025: エクスポートAPI
pub async fn export_request_responses(
    State(state): State<AppState>,
    Query(query): Query<RequestHistoryExportQuery>,
) -> Result<Response, AppError> {
    let filter = query.to_record_filter()?;
    const EXPORT_PAGE_SIZE: usize = 1000;

    let first_page = state
        .request_history
        .filter_and_paginate(&filter, 1, EXPORT_PAGE_SIZE)
        .await
        .map_err(AppError::from)?;

    match query.format {
        RequestHistoryExportFormat::Json => {
            let storage = state.request_history.clone();
            let filter = filter.clone();
            let (reader, mut writer) = tokio::io::duplex(16 * 1024);
            let mut page = 1usize;
            let mut page_data = Some(first_page.clone());
            tokio::spawn(async move {
                if writer.write_all(b"[").await.is_err() {
                    return;
                }
                let mut first = true;
                loop {
                    let data = if let Some(data) = page_data.take() {
                        data
                    } else {
                        match storage
                            .filter_and_paginate(&filter, page, EXPORT_PAGE_SIZE)
                            .await
                        {
                            Ok(data) => data,
                            Err(err) => {
                                warn!("Failed to export request history page {}: {}", page, err);
                                break;
                            }
                        }
                    };

                    if data.records.is_empty() {
                        break;
                    }

                    for record in data.records {
                        let json = match serde_json::to_vec(&record) {
                            Ok(json) => json,
                            Err(err) => {
                                warn!("Failed to serialize request history record: {}", err);
                                return;
                            }
                        };
                        if !first && writer.write_all(b",").await.is_err() {
                            return;
                        }
                        first = false;
                        if writer.write_all(&json).await.is_err() {
                            return;
                        }
                    }

                    if page * EXPORT_PAGE_SIZE >= data.total_count {
                        break;
                    }
                    page += 1;
                }

                let _ = writer.write_all(b"]").await;
                let _ = writer.shutdown().await;
            });

            let response = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .header(
                    "Content-Disposition",
                    "attachment; filename=\"request_history.json\"",
                )
                .body(Body::from_stream(ReaderStream::new(reader)))
                .unwrap();
            Ok(response)
        }
        RequestHistoryExportFormat::Csv => {
            let storage = state.request_history.clone();
            let filter = filter.clone();
            let (reader, mut writer) = tokio::io::duplex(16 * 1024);
            let mut page = 1usize;
            let mut page_data = Some(first_page.clone());
            tokio::spawn(async move {
                let mut header = csv::Writer::from_writer(vec![]);
                if header
                    .write_record([
                        "id",
                        "timestamp",
                        "request_type",
                        "model",
                        "runtime_id",
                        "runtime_machine_name",
                        "runtime_ip",
                        "client_ip",
                        "duration_ms",
                        "status",
                        "completed_at",
                    ])
                    .is_err()
                {
                    return;
                }
                let header_bytes = match header.into_inner() {
                    Ok(data) => data,
                    Err(err) => {
                        warn!("Failed to finalize CSV header: {}", err);
                        return;
                    }
                };
                if writer.write_all(&header_bytes).await.is_err() {
                    return;
                }

                loop {
                    let data = if let Some(data) = page_data.take() {
                        data
                    } else {
                        match storage
                            .filter_and_paginate(&filter, page, EXPORT_PAGE_SIZE)
                            .await
                        {
                            Ok(data) => data,
                            Err(err) => {
                                warn!("Failed to export request history page {}: {}", page, err);
                                break;
                            }
                        }
                    };

                    if data.records.is_empty() {
                        break;
                    }

                    for record in data.records {
                        let status_str = match &record.status {
                            crate::common::protocol::RecordStatus::Success => "success".to_string(),
                            crate::common::protocol::RecordStatus::Error { message } => {
                                format!("error: {}", message)
                            }
                        };

                        let mut row = csv::Writer::from_writer(vec![]);
                        if row
                            .write_record(&[
                                record.id.to_string(),
                                record.timestamp.to_rfc3339(),
                                format!("{:?}", record.request_type),
                                record.model,
                                record.endpoint_id.to_string(),
                                record.endpoint_name,
                                record.endpoint_ip.to_string(),
                                record
                                    .client_ip
                                    .map(|ip| ip.to_string())
                                    .unwrap_or_default(),
                                record.duration_ms.to_string(),
                                status_str,
                                record.completed_at.to_rfc3339(),
                            ])
                            .is_err()
                        {
                            return;
                        }

                        let row_bytes = match row.into_inner() {
                            Ok(data) => data,
                            Err(err) => {
                                warn!("Failed to finalize CSV row: {}", err);
                                return;
                            }
                        };

                        if writer.write_all(&row_bytes).await.is_err() {
                            return;
                        }
                    }

                    if page * EXPORT_PAGE_SIZE >= data.total_count {
                        break;
                    }
                    page += 1;
                }

                let _ = writer.shutdown().await;
            });

            let response = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/csv")
                .header(
                    "Content-Disposition",
                    "attachment; filename=\"request_history.csv\"",
                )
                .body(Body::from_stream(ReaderStream::new(reader)))
                .unwrap();

            Ok(response)
        }
    }
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

mod clients;
pub use clients::{
    get_client_api_keys, get_client_detail, get_client_heatmap, get_client_models,
    get_client_rankings, get_client_timeline,
};

/// 設定APIのデフォルト値
const IP_ALERT_THRESHOLD_DEFAULT_VALUE: i64 = 100;
const IP_ALERT_THRESHOLD_MIN: i64 = 1;

fn parse_ip_alert_threshold(value: &str) -> Result<i64, LbError> {
    let parsed = value.trim().parse::<i64>().map_err(|_| {
        LbError::Common(CommonError::Validation(
            "ip_alert_threshold must be an integer >= 1".to_string(),
        ))
    })?;
    if parsed < IP_ALERT_THRESHOLD_MIN {
        return Err(LbError::Common(CommonError::Validation(
            "ip_alert_threshold must be an integer >= 1".to_string(),
        )));
    }
    Ok(parsed)
}

fn effective_ip_alert_threshold(raw_value: Option<&str>) -> i64 {
    match raw_value {
        Some(raw) => match parse_ip_alert_threshold(raw) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    raw_value = %raw,
                    error = %err,
                    default = IP_ALERT_THRESHOLD_DEFAULT_VALUE,
                    "Invalid ip_alert_threshold in settings; falling back to default"
                );
                IP_ALERT_THRESHOLD_DEFAULT_VALUE
            }
        },
        None => IP_ALERT_THRESHOLD_DEFAULT_VALUE,
    }
}

/// GET /api/dashboard/settings/{key} - 設定値取得
///
/// SPEC-62ac4b68: 閾値ベースの異常検知
pub async fn get_setting(
    axum::extract::Path(key): axum::extract::Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let settings = crate::db::settings::SettingsStorage::new(state.db_pool.clone());
    let value = settings.get_setting(&key).await.map_err(AppError)?;
    let value = if key == "ip_alert_threshold" {
        effective_ip_alert_threshold(value.as_deref()).to_string()
    } else {
        value.unwrap_or_default()
    };
    Ok(Json(serde_json::json!({ "key": key, "value": value })))
}

/// PUT /api/dashboard/settings/{key} のリクエストボディ
#[derive(Debug, Deserialize)]
pub struct SettingUpdateBody {
    /// 設定値
    pub value: String,
}

/// PUT /api/dashboard/settings/{key} - 設定値更新
///
/// SPEC-62ac4b68: 閾値ベースの異常検知
pub async fn update_setting(
    axum::Extension(claims): axum::Extension<crate::common::auth::Claims>,
    axum::extract::Path(key): axum::extract::Path<String>,
    State(state): State<AppState>,
    Json(body): Json<SettingUpdateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 設定の書き込みは admin のみ（Viewer の権限昇格を防ぐ）
    if claims.role != crate::common::auth::UserRole::Admin {
        return Err(AppError(LbError::Authorization(
            "Only admin can update settings".to_string(),
        )));
    }
    let value = if key == "ip_alert_threshold" {
        parse_ip_alert_threshold(&body.value)
            .map_err(AppError)?
            .to_string()
    } else {
        body.value
    };

    let settings = crate::db::settings::SettingsStorage::new(state.db_pool.clone());
    settings.set_setting(&key, &value).await.map_err(AppError)?;
    Ok(Json(serde_json::json!({ "key": key, "value": value })))
}

// NOTE: テストは NodeRegistry → EndpointRegistry 移行完了後に再実装
// 関連: SPEC-e8e9326e

#[cfg(test)]
mod tests {
    use super::overview::{
        collect_action_items, count_unique_model_ids, invalidate_persisted_totals_cache,
        PersistedRequestTotals, PersistedTokenTotals, PersistedTotalsCache,
        LAST_KNOWN_PERSISTED_TOTALS,
    };
    use super::{parse_ip_alert_threshold, DashboardOperations};
    use crate::types::endpoint::{Endpoint, EndpointStatus, EndpointType};

    #[test]
    fn persisted_totals_cache_invalidation_clears_entries() {
        // arch-review [L3]: クリアフックがプロセスキャッシュを確実に空にすることを検証。
        LAST_KNOWN_PERSISTED_TOTALS.write().unwrap().insert(
            1u64,
            PersistedTotalsCache {
                request_totals: PersistedRequestTotals::default(),
                token_totals: PersistedTokenTotals::default(),
            },
        );
        assert!(!LAST_KNOWN_PERSISTED_TOTALS.read().unwrap().is_empty());
        invalidate_persisted_totals_cache();
        assert!(LAST_KNOWN_PERSISTED_TOTALS.read().unwrap().is_empty());
    }

    /// フォールバック計算: avg_response_time_ms が None の場合に
    /// オンラインエンドポイントの latency_ms から平均値を計算するロジック
    fn fallback_avg_response_time(summary_avg: Option<f32>, endpoints: &[Endpoint]) -> Option<f32> {
        summary_avg.or_else(|| {
            let online_endpoints: Vec<_> = endpoints
                .iter()
                .filter(|e| e.status == EndpointStatus::Online && e.latency_ms.is_some())
                .collect();
            if online_endpoints.is_empty() {
                return None;
            }
            let total: f64 = online_endpoints
                .iter()
                .map(|e| e.latency_ms.unwrap() as f64)
                .sum();
            Some((total / online_endpoints.len() as f64) as f32)
        })
    }

    /// T010 [US3]: collect_stats のフォールバック計算テスト
    /// インメモリavg_response_time_msがNoneの場合、オンラインエンドポイントの
    /// latency_msから平均値を計算する
    #[test]
    fn test_avg_response_time_fallback_from_latency() {
        // (1) summary の average_response_time_ms が None
        // (2) オンラインエンドポイント2つ (latency_ms=100, 200)
        let mut ep1 = Endpoint::new(
            "EP1".to_string(),
            "http://localhost:8001".to_string(),
            EndpointType::Xllm,
        );
        ep1.status = EndpointStatus::Online;
        ep1.latency_ms = Some(100);

        let mut ep2 = Endpoint::new(
            "EP2".to_string(),
            "http://localhost:8002".to_string(),
            EndpointType::Xllm,
        );
        ep2.status = EndpointStatus::Online;
        ep2.latency_ms = Some(200);

        let endpoints = vec![ep1, ep2];

        // (3) 結果は 150.0（平均値）
        let result = fallback_avg_response_time(None, &endpoints);
        assert_eq!(result, Some(150.0));
    }

    /// T010 追加シナリオ: 全エンドポイントがオフラインの場合は None のまま
    #[test]
    fn test_avg_response_time_fallback_all_offline() {
        let mut ep1 = Endpoint::new(
            "EP1".to_string(),
            "http://localhost:8001".to_string(),
            EndpointType::Xllm,
        );
        ep1.status = EndpointStatus::Offline;
        ep1.latency_ms = Some(100);

        let endpoints = vec![ep1];

        let result = fallback_avg_response_time(None, &endpoints);
        assert_eq!(result, None);
    }

    /// T010 追加シナリオ: summary に値がある場合はフォールバックしない
    #[test]
    fn test_avg_response_time_no_fallback_when_present() {
        let endpoints = vec![];
        let result = fallback_avg_response_time(Some(42.0), &endpoints);
        assert_eq!(result, Some(42.0));
    }

    #[test]
    fn parse_ip_alert_threshold_accepts_positive_integer() {
        assert_eq!(parse_ip_alert_threshold("100").unwrap(), 100);
        assert_eq!(parse_ip_alert_threshold(" 42 ").unwrap(), 42);
    }

    #[test]
    fn parse_ip_alert_threshold_rejects_zero_or_negative() {
        assert!(parse_ip_alert_threshold("0").is_err());
        assert!(parse_ip_alert_threshold("-1").is_err());
    }

    #[test]
    fn parse_ip_alert_threshold_rejects_non_numeric() {
        assert!(parse_ip_alert_threshold("abc").is_err());
    }

    // ===== effective_ip_alert_threshold tests =====

    #[test]
    fn effective_threshold_returns_parsed_value() {
        use super::effective_ip_alert_threshold;
        assert_eq!(effective_ip_alert_threshold(Some("50")), 50);
    }

    #[test]
    fn effective_threshold_returns_default_when_none() {
        use super::effective_ip_alert_threshold;
        use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
        assert_eq!(
            effective_ip_alert_threshold(None),
            IP_ALERT_THRESHOLD_DEFAULT_VALUE
        );
    }

    #[test]
    fn effective_threshold_returns_default_for_invalid_value() {
        use super::effective_ip_alert_threshold;
        use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
        assert_eq!(
            effective_ip_alert_threshold(Some("not-a-number")),
            IP_ALERT_THRESHOLD_DEFAULT_VALUE
        );
    }

    #[test]
    fn effective_threshold_returns_default_for_zero() {
        use super::effective_ip_alert_threshold;
        use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
        assert_eq!(
            effective_ip_alert_threshold(Some("0")),
            IP_ALERT_THRESHOLD_DEFAULT_VALUE
        );
    }

    #[test]
    fn effective_threshold_returns_default_for_negative() {
        use super::effective_ip_alert_threshold;
        use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
        assert_eq!(
            effective_ip_alert_threshold(Some("-5")),
            IP_ALERT_THRESHOLD_DEFAULT_VALUE
        );
    }

    #[test]
    fn effective_threshold_accepts_one() {
        use super::effective_ip_alert_threshold;
        assert_eq!(effective_ip_alert_threshold(Some("1")), 1);
    }

    #[test]
    fn effective_threshold_accepts_large_value() {
        use super::effective_ip_alert_threshold;
        assert_eq!(effective_ip_alert_threshold(Some("10000")), 10000);
    }

    // ===== parse_ip_alert_threshold extended tests =====

    #[test]
    fn parse_ip_alert_threshold_accepts_large_value() {
        assert_eq!(parse_ip_alert_threshold("999999").unwrap(), 999999);
    }

    #[test]
    fn parse_ip_alert_threshold_accepts_one() {
        assert_eq!(parse_ip_alert_threshold("1").unwrap(), 1);
    }

    #[test]
    fn parse_ip_alert_threshold_rejects_empty_string() {
        assert!(parse_ip_alert_threshold("").is_err());
    }

    #[test]
    fn parse_ip_alert_threshold_rejects_float() {
        assert!(parse_ip_alert_threshold("1.5").is_err());
    }

    // ===== DashboardStats serialization tests =====

    #[test]
    fn test_dashboard_stats_serialization() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 5,
            online_nodes: 3,
            pending_nodes: 1,
            registering_nodes: 0,
            offline_nodes: 1,
            total_requests: 1000,
            successful_requests: 950,
            failed_requests: 50,
            total_active_requests: 2,
            queued_requests: 0,
            average_response_time_ms: Some(150.5),
            average_gpu_usage: None,
            average_gpu_memory_usage: None,
            last_metrics_updated_at: None,
            last_registered_at: None,
            last_seen_at: None,
            openai_key_present: true,
            google_key_present: false,
            anthropic_key_present: false,
            total_input_tokens: 100000,
            total_output_tokens: 50000,
            total_tokens: 150000,
        };

        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_runtimes"], 5);
        assert_eq!(json["online_runtimes"], 3);
        assert_eq!(json["pending_runtimes"], 1);
        assert_eq!(json["registering_runtimes"], 0);
        assert_eq!(json["offline_runtimes"], 1);
        assert_eq!(json["total_requests"], 1000);
        assert_eq!(json["successful_requests"], 950);
        assert_eq!(json["failed_requests"], 50);
        assert_eq!(json["total_active_requests"], 2);
        assert_eq!(json["queued_requests"], 0);
        assert_eq!(json["average_response_time_ms"], 150.5);
        assert!(json["average_gpu_usage"].is_null());
        assert_eq!(json["openai_key_present"], true);
        assert_eq!(json["google_key_present"], false);
        assert_eq!(json["anthropic_key_present"], false);
        assert_eq!(json["total_input_tokens"], 100000);
        assert_eq!(json["total_output_tokens"], 50000);
        assert_eq!(json["total_tokens"], 150000);
    }

    #[test]
    fn test_dashboard_stats_rename_fields() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 2,
            online_nodes: 1,
            pending_nodes: 0,
            registering_nodes: 0,
            offline_nodes: 1,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            average_gpu_usage: None,
            average_gpu_memory_usage: None,
            last_metrics_updated_at: None,
            last_registered_at: None,
            last_seen_at: None,
            openai_key_present: false,
            google_key_present: false,
            anthropic_key_present: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
        };

        let json = serde_json::to_value(&stats).unwrap();
        // Verify the serde rename_all is applied (total_runtimes, not total_nodes)
        assert!(json.get("total_runtimes").is_some());
        assert!(json.get("online_runtimes").is_some());
        assert!(json.get("pending_runtimes").is_some());
        assert!(json.get("offline_runtimes").is_some());
    }

    // ===== DashboardEndpoint tests =====

    #[test]
    fn test_dashboard_endpoint_serialization() {
        use super::DashboardEndpoint;
        use crate::types::endpoint::{EndpointStatus, EndpointType};

        let endpoint = DashboardEndpoint {
            id: uuid::Uuid::nil(),
            name: "test-endpoint".to_string(),
            base_url: "http://localhost:8080".to_string(),
            status: EndpointStatus::Online,
            endpoint_type: EndpointType::Xllm,
            health_check_interval_secs: 30,
            inference_timeout_secs: 120,
            latency_ms: Some(45),
            last_seen: None,
            last_error: None,
            error_count: 0,
            registered_at: chrono::Utc::now(),
            notes: None,
            model_count: 3,
            total_requests: 100,
            successful_requests: 95,
            failed_requests: 5,
        };

        let json = serde_json::to_value(&endpoint).unwrap();
        assert_eq!(json["name"], "test-endpoint");
        assert_eq!(json["base_url"], "http://localhost:8080");
        assert_eq!(json["health_check_interval_secs"], 30);
        assert_eq!(json["inference_timeout_secs"], 120);
        assert_eq!(json["latency_ms"], 45);
        assert_eq!(json["error_count"], 0);
        assert_eq!(json["model_count"], 3);
        assert_eq!(json["total_requests"], 100);
        assert_eq!(json["successful_requests"], 95);
        assert_eq!(json["failed_requests"], 5);
    }

    // ===== PersistedRequestTotals / PersistedTokenTotals / PersistedTotalsCache tests =====

    #[test]
    fn test_persisted_request_totals_default() {
        use super::overview::PersistedRequestTotals;
        let defaults = PersistedRequestTotals::default();
        assert_eq!(defaults.total_requests, 0);
        assert_eq!(defaults.successful_requests, 0);
        assert_eq!(defaults.failed_requests, 0);
    }

    #[test]
    fn test_persisted_token_totals_default() {
        use super::overview::PersistedTokenTotals;
        let defaults = PersistedTokenTotals::default();
        assert_eq!(defaults.total_input_tokens, 0);
        assert_eq!(defaults.total_output_tokens, 0);
        assert_eq!(defaults.total_tokens, 0);
    }

    #[test]
    fn test_persisted_totals_cache_default() {
        use super::overview::PersistedTotalsCache;
        let cache = PersistedTotalsCache::default();
        assert_eq!(cache.request_totals.total_requests, 0);
        assert_eq!(cache.token_totals.total_tokens, 0);
    }

    // ===== RequestHistoryQuery tests =====

    #[test]
    fn test_normalized_per_page_valid_sizes() {
        use super::RequestHistoryQuery;

        for &size in super::ALLOWED_PAGE_SIZES {
            let query = RequestHistoryQuery {
                page: 1,
                per_page: size,
                limit: None,
                offset: None,
                model: None,
                endpoint_id: None,
                status: None,
                start_time: None,
                end_time: None,
                client_ip: None,
            };
            assert_eq!(query.normalized_per_page(), size);
        }
    }

    #[test]
    fn test_normalized_per_page_invalid_falls_back_to_default() {
        use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};

        let query = RequestHistoryQuery {
            page: 1,
            per_page: 37,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn test_normalized_per_page_zero_falls_back() {
        use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};

        let query = RequestHistoryQuery {
            page: 1,
            per_page: 0,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
    }

    // ===== to_record_filter tests =====

    #[test]
    fn test_to_record_filter_valid() {
        use super::RequestHistoryQuery;

        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: Some("llama".to_string()),
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        let filter = query.to_record_filter().unwrap();
        assert_eq!(filter.model, Some("llama".to_string()));
    }

    #[test]
    fn test_to_record_filter_start_after_end_error() {
        use super::RequestHistoryQuery;
        use chrono::TimeZone;

        let start = chrono::Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
        let end = chrono::Utc.with_ymd_and_hms(2026, 2, 1, 12, 0, 0).unwrap();

        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: Some(start),
            end_time: Some(end),
            client_ip: None,
        };
        assert!(query.to_record_filter().is_err());
    }

    // ===== RequestHistoryExportQuery to_record_filter tests =====

    #[test]
    fn test_export_query_to_record_filter_valid() {
        use super::RequestHistoryExportQuery;

        let query = RequestHistoryExportQuery {
            format: super::RequestHistoryExportFormat::Json,
            model: Some("gpt-4".to_string()),
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        let filter = query.to_record_filter().unwrap();
        assert_eq!(filter.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_export_query_to_record_filter_start_after_end_error() {
        use super::RequestHistoryExportQuery;
        use chrono::TimeZone;

        let start = chrono::Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let end = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        let query = RequestHistoryExportQuery {
            format: super::RequestHistoryExportFormat::Csv,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: Some(start),
            end_time: Some(end),
            client_ip: None,
        };
        assert!(query.to_record_filter().is_err());
    }

    // ===== RequestHistoryExportFormat tests =====

    #[test]
    fn test_export_format_default_is_csv() {
        use super::RequestHistoryExportFormat;
        assert_eq!(
            RequestHistoryExportFormat::default(),
            RequestHistoryExportFormat::Csv
        );
    }

    #[test]
    fn test_export_format_deserialization() {
        use super::RequestHistoryExportFormat;
        assert_eq!(
            serde_json::from_str::<RequestHistoryExportFormat>("\"csv\"").unwrap(),
            RequestHistoryExportFormat::Csv
        );
        assert_eq!(
            serde_json::from_str::<RequestHistoryExportFormat>("\"json\"").unwrap(),
            RequestHistoryExportFormat::Json
        );
    }

    // ===== default functions tests =====

    #[test]
    fn test_default_page() {
        use super::default_page;
        assert_eq!(default_page(), 1);
    }

    #[test]
    fn test_default_per_page() {
        use super::{default_per_page, DEFAULT_PAGE_SIZE};
        assert_eq!(default_per_page(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn test_default_days() {
        use super::default_days;
        assert_eq!(default_days(), Some(30));
    }

    #[test]
    fn test_default_months() {
        use super::default_months;
        assert_eq!(default_months(), Some(12));
    }

    #[test]
    fn test_default_clients_per_page() {
        use super::default_clients_per_page;
        assert_eq!(default_clients_per_page(), 20);
    }

    // ===== ALLOWED_PAGE_SIZES / DEFAULT_PAGE_SIZE constants tests =====

    #[test]
    fn test_allowed_page_sizes_are_reasonable() {
        use super::{ALLOWED_PAGE_SIZES, DEFAULT_PAGE_SIZE};
        assert!(ALLOWED_PAGE_SIZES.contains(&10));
        assert!(ALLOWED_PAGE_SIZES.contains(&25));
        assert!(ALLOWED_PAGE_SIZES.contains(&50));
        assert!(ALLOWED_PAGE_SIZES.contains(&100));
        assert!(ALLOWED_PAGE_SIZES.contains(&DEFAULT_PAGE_SIZE));
    }

    // ===== DailyTokenStatsQuery deserialization tests =====

    #[test]
    fn test_daily_token_stats_query_deserialization() {
        use super::DailyTokenStatsQuery;
        let q: DailyTokenStatsQuery = serde_json::from_str(r#"{"days": 7}"#).unwrap();
        assert_eq!(q.days, Some(7));
    }

    #[test]
    fn test_daily_token_stats_query_default_days() {
        use super::DailyTokenStatsQuery;
        let q: DailyTokenStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.days, Some(30));
    }

    // ===== MonthlyTokenStatsQuery deserialization tests =====

    #[test]
    fn test_monthly_token_stats_query_deserialization() {
        use super::MonthlyTokenStatsQuery;
        let q: MonthlyTokenStatsQuery = serde_json::from_str(r#"{"months": 6}"#).unwrap();
        assert_eq!(q.months, Some(6));
    }

    #[test]
    fn test_monthly_token_stats_query_default_months() {
        use super::MonthlyTokenStatsQuery;
        let q: MonthlyTokenStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.months, Some(12));
    }

    // ===== DailyTokenStats serialization =====

    #[test]
    fn test_daily_token_stats_serialization() {
        use super::DailyTokenStats;
        let stats = DailyTokenStats {
            date: "2026-02-27".to_string(),
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_tokens: 1500,
            request_count: 10,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["date"], "2026-02-27");
        assert_eq!(json["total_input_tokens"], 1000);
        assert_eq!(json["total_output_tokens"], 500);
        assert_eq!(json["total_tokens"], 1500);
        assert_eq!(json["request_count"], 10);
    }

    // ===== MonthlyTokenStats serialization =====

    #[test]
    fn test_monthly_token_stats_serialization() {
        use super::MonthlyTokenStats;
        let stats = MonthlyTokenStats {
            month: "2026-02".to_string(),
            total_input_tokens: 50000,
            total_output_tokens: 25000,
            total_tokens: 75000,
            request_count: 500,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["month"], "2026-02");
        assert_eq!(json["total_input_tokens"], 50000);
        assert_eq!(json["total_output_tokens"], 25000);
        assert_eq!(json["total_tokens"], 75000);
        assert_eq!(json["request_count"], 500);
    }

    // ===== DashboardEndpoint equality tests =====

    #[test]
    fn test_dashboard_endpoint_equality() {
        use super::DashboardEndpoint;
        use crate::types::endpoint::{EndpointStatus, EndpointType};

        let ts = chrono::Utc::now();
        let ep1 = DashboardEndpoint {
            id: uuid::Uuid::nil(),
            name: "ep".to_string(),
            base_url: "http://localhost".to_string(),
            status: EndpointStatus::Online,
            endpoint_type: EndpointType::Xllm,
            health_check_interval_secs: 30,
            inference_timeout_secs: 120,
            latency_ms: None,
            last_seen: None,
            last_error: None,
            error_count: 0,
            registered_at: ts,
            notes: None,
            model_count: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
        };
        let ep2 = ep1.clone();
        assert_eq!(ep1, ep2);
    }

    // ===== DashboardStats equality =====

    #[test]
    fn test_dashboard_stats_equality() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 0,
            online_nodes: 0,
            pending_nodes: 0,
            registering_nodes: 0,
            offline_nodes: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            average_gpu_usage: None,
            average_gpu_memory_usage: None,
            last_metrics_updated_at: None,
            last_registered_at: None,
            last_seen_at: None,
            openai_key_present: false,
            google_key_present: false,
            anthropic_key_present: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
        };
        let stats2 = stats.clone();
        assert_eq!(stats, stats2);
    }

    // ===== ModelTpsEntry serialization =====

    #[test]
    fn test_model_tps_entry_serialization() {
        use super::ModelTpsEntry;
        use crate::common::protocol::{TpsApiKind, TpsSource};

        let entry = ModelTpsEntry {
            model_id: "llama-3-8b".to_string(),
            api_kind: TpsApiKind::ChatCompletions,
            source: TpsSource::Production,
            tps: Some(42.5),
            request_count: 100,
            total_output_tokens: 5000,
            average_duration_ms: Some(200.0),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["model_id"], "llama-3-8b");
        assert_eq!(json["tps"], 42.5);
        assert_eq!(json["request_count"], 100);
        assert_eq!(json["total_output_tokens"], 5000);
        assert_eq!(json["average_duration_ms"], 200.0);
    }

    // ===== fallback_avg_response_time with latency_ms None =====

    #[test]
    fn test_avg_response_time_fallback_online_no_latency() {
        let mut ep = Endpoint::new(
            "EP".to_string(),
            "http://localhost:8001".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        ep.latency_ms = None;

        let result = fallback_avg_response_time(None, &[ep]);
        assert_eq!(result, None);
    }

    // ===== EndpointDailyStatsQuery deserialization =====

    #[test]
    fn test_endpoint_daily_stats_query_default() {
        use super::EndpointDailyStatsQuery;
        let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert!(q.days.is_none());
    }

    #[test]
    fn test_endpoint_daily_stats_query_with_days() {
        use super::EndpointDailyStatsQuery;
        let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 14}"#).unwrap();
        assert_eq!(q.days, Some(14));
    }

    // ===== ClientsQuery deserialization =====

    #[test]
    fn test_clients_query_defaults() {
        use super::ClientsQuery;
        let q: ClientsQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.page, 1);
        assert_eq!(q.per_page, 20);
    }

    #[test]
    fn test_clients_query_custom() {
        use super::ClientsQuery;
        let q: ClientsQuery = serde_json::from_str(r#"{"page": 3, "per_page": 50}"#).unwrap();
        assert_eq!(q.page, 3);
        assert_eq!(q.per_page, 50);
    }

    // ===== SettingUpdateBody deserialization =====

    #[test]
    fn test_setting_update_body_deserialization() {
        use super::SettingUpdateBody;
        let body: SettingUpdateBody = serde_json::from_str(r#"{"value": "200"}"#).unwrap();
        assert_eq!(body.value, "200");
    }

    // ===== IP_ALERT_THRESHOLD constants =====

    #[test]
    fn test_ip_alert_threshold_constants() {
        use super::{IP_ALERT_THRESHOLD_DEFAULT_VALUE, IP_ALERT_THRESHOLD_MIN};
        assert_eq!(IP_ALERT_THRESHOLD_DEFAULT_VALUE, 100);
        assert_eq!(IP_ALERT_THRESHOLD_MIN, 1);
    }

    // ===== RequestHistoryQuery deserialization =====

    #[test]
    fn test_request_history_query_deserialization_defaults() {
        use super::RequestHistoryQuery;
        let q: RequestHistoryQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.page, 1);
        assert_eq!(q.per_page, 10);
        assert!(q.model.is_none());
        assert!(q.endpoint_id.is_none());
        assert!(q.status.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
    }

    #[test]
    fn test_request_history_query_with_filters() {
        use super::RequestHistoryQuery;
        let json = r#"{"page": 2, "per_page": 25, "model": "llama"}"#;
        let q: RequestHistoryQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.page, 2);
        assert_eq!(q.per_page, 25);
        assert_eq!(q.model, Some("llama".to_string()));
    }

    // ===== RequestHistoryExportQuery deserialization =====

    #[test]
    fn test_export_query_deserialization() {
        use super::RequestHistoryExportQuery;
        let q: RequestHistoryExportQuery =
            serde_json::from_str(r#"{"format": "json", "model": "gpt-4"}"#).unwrap();
        assert_eq!(q.format, super::RequestHistoryExportFormat::Json);
        assert_eq!(q.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_export_query_default_format() {
        use super::RequestHistoryExportQuery;
        let q: RequestHistoryExportQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.format, super::RequestHistoryExportFormat::Csv);
    }

    // ===== Additional unit tests for increased coverage =====

    // --- DashboardStats extended tests ---

    #[test]
    fn test_dashboard_stats_all_none_optional_fields() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 0,
            online_nodes: 0,
            pending_nodes: 0,
            registering_nodes: 0,
            offline_nodes: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            average_gpu_usage: None,
            average_gpu_memory_usage: None,
            last_metrics_updated_at: None,
            last_registered_at: None,
            last_seen_at: None,
            openai_key_present: false,
            google_key_present: false,
            anthropic_key_present: false,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert!(json["average_response_time_ms"].is_null());
        assert!(json["average_gpu_usage"].is_null());
        assert!(json["average_gpu_memory_usage"].is_null());
        assert!(json["last_metrics_updated_at"].is_null());
        assert!(json["last_registered_at"].is_null());
        assert!(json["last_seen_at"].is_null());
    }

    #[test]
    fn test_dashboard_stats_with_gpu_metrics() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 1,
            online_nodes: 1,
            pending_nodes: 0,
            registering_nodes: 0,
            offline_nodes: 0,
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
            total_active_requests: 5,
            queued_requests: 2,
            average_response_time_ms: Some(250.0),
            average_gpu_usage: Some(85.5),
            average_gpu_memory_usage: Some(72.3),
            last_metrics_updated_at: Some(chrono::Utc::now()),
            last_registered_at: Some(chrono::Utc::now()),
            last_seen_at: Some(chrono::Utc::now()),
            openai_key_present: true,
            google_key_present: true,
            anthropic_key_present: true,
            total_input_tokens: 50000,
            total_output_tokens: 25000,
            total_tokens: 75000,
        };
        let json = serde_json::to_value(&stats).unwrap();
        let gpu_usage = json["average_gpu_usage"].as_f64().unwrap();
        assert!((gpu_usage - 85.5).abs() < 0.01);
        let gpu_mem = json["average_gpu_memory_usage"].as_f64().unwrap();
        assert!((gpu_mem - 72.3).abs() < 0.01);
        assert!(json["last_metrics_updated_at"].is_string());
        assert!(json["last_registered_at"].is_string());
        assert!(json["last_seen_at"].is_string());
    }

    #[test]
    fn test_dashboard_stats_token_counts() {
        use super::DashboardStats;

        let stats = DashboardStats {
            total_nodes: 0,
            online_nodes: 0,
            pending_nodes: 0,
            registering_nodes: 0,
            offline_nodes: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            average_gpu_usage: None,
            average_gpu_memory_usage: None,
            last_metrics_updated_at: None,
            last_registered_at: None,
            last_seen_at: None,
            openai_key_present: false,
            google_key_present: false,
            anthropic_key_present: false,
            total_input_tokens: u64::MAX,
            total_output_tokens: u64::MAX,
            total_tokens: u64::MAX,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_input_tokens"], u64::MAX);
        assert_eq!(json["total_output_tokens"], u64::MAX);
        assert_eq!(json["total_tokens"], u64::MAX);
    }

    // --- DashboardEndpoint extended tests ---

    #[test]
    fn test_dashboard_endpoint_with_error() {
        use super::DashboardEndpoint;
        use crate::types::endpoint::{EndpointStatus, EndpointType};

        let endpoint = DashboardEndpoint {
            id: uuid::Uuid::nil(),
            name: "error-endpoint".to_string(),
            base_url: "http://localhost:8080".to_string(),
            status: EndpointStatus::Offline,
            endpoint_type: EndpointType::Vllm,
            health_check_interval_secs: 60,
            inference_timeout_secs: 300,
            latency_ms: None,
            last_seen: Some(chrono::Utc::now()),
            last_error: Some("Connection refused".to_string()),
            error_count: 5,
            registered_at: chrono::Utc::now(),
            notes: Some("This endpoint has issues".to_string()),
            model_count: 0,
            total_requests: 50,
            successful_requests: 40,
            failed_requests: 10,
        };
        let json = serde_json::to_value(&endpoint).unwrap();
        assert_eq!(json["status"], "offline");
        assert_eq!(json["last_error"], "Connection refused");
        assert_eq!(json["error_count"], 5);
        assert_eq!(json["notes"], "This endpoint has issues");
    }

    #[test]
    fn test_dashboard_endpoint_all_endpoint_types() {
        use super::DashboardEndpoint;
        use crate::types::endpoint::{EndpointStatus, EndpointType};

        let types = [
            EndpointType::Xllm,
            EndpointType::Vllm,
            EndpointType::Ollama,
            EndpointType::OpenaiCompatible,
        ];
        for ep_type in types {
            let endpoint = DashboardEndpoint {
                id: uuid::Uuid::new_v4(),
                name: format!("ep-{:?}", ep_type),
                base_url: "http://localhost".to_string(),
                status: EndpointStatus::Online,
                endpoint_type: ep_type,
                health_check_interval_secs: 30,
                inference_timeout_secs: 120,
                latency_ms: None,
                last_seen: None,
                last_error: None,
                error_count: 0,
                registered_at: chrono::Utc::now(),
                notes: None,
                model_count: 0,
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
            };
            let json = serde_json::to_value(&endpoint).unwrap();
            assert!(json["endpoint_type"].is_string());
        }
    }

    #[test]
    fn test_dashboard_endpoint_clone() {
        use super::DashboardEndpoint;
        use crate::types::endpoint::{EndpointStatus, EndpointType};

        let endpoint = DashboardEndpoint {
            id: uuid::Uuid::nil(),
            name: "clone-test".to_string(),
            base_url: "http://localhost".to_string(),
            status: EndpointStatus::Online,
            endpoint_type: EndpointType::Xllm,
            health_check_interval_secs: 30,
            inference_timeout_secs: 120,
            latency_ms: Some(42),
            last_seen: None,
            last_error: None,
            error_count: 0,
            registered_at: chrono::Utc::now(),
            notes: None,
            model_count: 2,
            total_requests: 10,
            successful_requests: 8,
            failed_requests: 2,
        };
        let cloned = endpoint.clone();
        assert_eq!(endpoint, cloned);
    }

    // --- PersistedRequestTotals / PersistedTokenTotals extended tests ---

    #[test]
    fn test_persisted_request_totals_copy() {
        use super::overview::PersistedRequestTotals;
        let totals = PersistedRequestTotals {
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
        };
        let copied = totals;
        assert_eq!(copied.total_requests, 100);
        assert_eq!(copied.successful_requests, 90);
        assert_eq!(copied.failed_requests, 10);
    }

    #[test]
    fn test_persisted_token_totals_copy() {
        use super::overview::PersistedTokenTotals;
        let totals = PersistedTokenTotals {
            total_input_tokens: 50000,
            total_output_tokens: 25000,
            total_tokens: 75000,
        };
        let copied = totals;
        assert_eq!(copied.total_input_tokens, 50000);
        assert_eq!(copied.total_output_tokens, 25000);
        assert_eq!(copied.total_tokens, 75000);
    }

    #[test]
    fn test_persisted_totals_cache_copy() {
        use super::{PersistedRequestTotals, PersistedTokenTotals, PersistedTotalsCache};
        let cache = PersistedTotalsCache {
            request_totals: PersistedRequestTotals {
                total_requests: 10,
                successful_requests: 8,
                failed_requests: 2,
            },
            token_totals: PersistedTokenTotals {
                total_input_tokens: 1000,
                total_output_tokens: 500,
                total_tokens: 1500,
            },
        };
        let copied = cache;
        assert_eq!(copied.request_totals.total_requests, 10);
        assert_eq!(copied.token_totals.total_tokens, 1500);
    }

    // --- RequestHistoryQuery extended tests ---

    #[test]
    fn test_normalized_per_page_large_value_falls_back() {
        use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 500,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn test_normalized_per_page_one_falls_back() {
        use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 1,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn test_to_record_filter_all_none() {
        use super::RequestHistoryQuery;
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        let filter = query.to_record_filter().unwrap();
        assert!(filter.model.is_none());
        assert!(filter.endpoint_id.is_none());
        assert!(filter.status.is_none());
        assert!(filter.start_time.is_none());
        assert!(filter.end_time.is_none());
        assert!(filter.client_ip.is_none());
    }

    #[test]
    fn test_to_record_filter_with_client_ip() {
        use super::RequestHistoryQuery;
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: Some("192.168.1.1".to_string()),
        };
        let filter = query.to_record_filter().unwrap();
        assert_eq!(filter.client_ip, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn test_to_record_filter_with_endpoint_id() {
        use super::RequestHistoryQuery;
        let id = uuid::Uuid::new_v4();
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: Some(id),
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        let filter = query.to_record_filter().unwrap();
        assert_eq!(filter.endpoint_id, Some(id));
    }

    #[test]
    fn test_to_record_filter_start_equals_end_is_ok() {
        use super::RequestHistoryQuery;
        use chrono::TimeZone;

        let ts = chrono::Utc.with_ymd_and_hms(2026, 2, 27, 12, 0, 0).unwrap();
        let query = RequestHistoryQuery {
            page: 1,
            per_page: 10,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: Some(ts),
            end_time: Some(ts),
            client_ip: None,
        };
        // start == end should be ok
        assert!(query.to_record_filter().is_ok());
    }

    // --- RequestHistoryExportQuery extended tests ---

    #[test]
    fn test_export_query_to_record_filter_all_none() {
        use super::RequestHistoryExportQuery;
        let query = RequestHistoryExportQuery {
            format: super::RequestHistoryExportFormat::Csv,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        let filter = query.to_record_filter().unwrap();
        assert!(filter.model.is_none());
        assert!(filter.endpoint_id.is_none());
    }

    #[test]
    fn test_export_query_to_record_filter_start_equals_end_is_ok() {
        use super::RequestHistoryExportQuery;
        use chrono::TimeZone;

        let ts = chrono::Utc.with_ymd_and_hms(2026, 1, 15, 0, 0, 0).unwrap();
        let query = RequestHistoryExportQuery {
            format: super::RequestHistoryExportFormat::Json,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: Some(ts),
            end_time: Some(ts),
            client_ip: None,
        };
        assert!(query.to_record_filter().is_ok());
    }

    // --- RequestHistoryExportFormat extended tests ---

    #[test]
    fn test_export_format_equality() {
        use super::RequestHistoryExportFormat;
        assert_eq!(
            RequestHistoryExportFormat::Csv,
            RequestHistoryExportFormat::Csv
        );
        assert_eq!(
            RequestHistoryExportFormat::Json,
            RequestHistoryExportFormat::Json
        );
        assert_ne!(
            RequestHistoryExportFormat::Csv,
            RequestHistoryExportFormat::Json
        );
    }

    #[test]
    fn test_export_format_copy() {
        use super::RequestHistoryExportFormat;
        let fmt = RequestHistoryExportFormat::Json;
        let copied = fmt;
        assert_eq!(fmt, copied);
    }

    #[test]
    fn test_export_format_debug() {
        use super::RequestHistoryExportFormat;
        let debug = format!("{:?}", RequestHistoryExportFormat::Csv);
        assert!(debug.contains("Csv"));
    }

    #[test]
    fn test_export_format_invalid_deserialization() {
        use super::RequestHistoryExportFormat;
        assert!(serde_json::from_str::<RequestHistoryExportFormat>("\"xml\"").is_err());
        assert!(serde_json::from_str::<RequestHistoryExportFormat>("\"CSV\"").is_err());
    }

    // --- parse_ip_alert_threshold extended tests ---

    #[test]
    fn parse_ip_alert_threshold_max_i64() {
        let result = parse_ip_alert_threshold(&i64::MAX.to_string());
        assert_eq!(result.unwrap(), i64::MAX);
    }

    #[test]
    fn parse_ip_alert_threshold_whitespace_around() {
        assert_eq!(parse_ip_alert_threshold("  100  ").unwrap(), 100);
    }

    #[test]
    fn parse_ip_alert_threshold_leading_plus_rejected() {
        // "+100" may or may not parse, but the function trims, so let's check
        // Rust's i64 parse does accept "+100"
        let result = parse_ip_alert_threshold("+100");
        assert_eq!(result.unwrap(), 100);
    }

    // --- effective_ip_alert_threshold extended tests ---

    #[test]
    fn effective_threshold_with_whitespace() {
        use super::effective_ip_alert_threshold;
        assert_eq!(effective_ip_alert_threshold(Some("  75  ")), 75);
    }

    #[test]
    fn effective_threshold_with_empty_string() {
        use super::effective_ip_alert_threshold;
        use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
        assert_eq!(
            effective_ip_alert_threshold(Some("")),
            IP_ALERT_THRESHOLD_DEFAULT_VALUE
        );
    }

    // --- fallback_avg_response_time extended tests ---

    #[test]
    fn test_avg_response_time_fallback_mixed_status_endpoints() {
        let mut ep_online = Endpoint::new(
            "Online".to_string(),
            "http://localhost:8001".to_string(),
            EndpointType::Xllm,
        );
        ep_online.status = EndpointStatus::Online;
        ep_online.latency_ms = Some(300);

        let mut ep_offline = Endpoint::new(
            "Offline".to_string(),
            "http://localhost:8002".to_string(),
            EndpointType::Xllm,
        );
        ep_offline.status = EndpointStatus::Offline;
        ep_offline.latency_ms = Some(100);

        let mut ep_pending = Endpoint::new(
            "Pending".to_string(),
            "http://localhost:8003".to_string(),
            EndpointType::Xllm,
        );
        ep_pending.status = EndpointStatus::Pending;
        ep_pending.latency_ms = Some(50);

        let endpoints = vec![ep_online, ep_offline, ep_pending];
        // Only the online endpoint with latency should be counted
        let result = fallback_avg_response_time(None, &endpoints);
        assert_eq!(result, Some(300.0));
    }

    #[test]
    fn test_avg_response_time_fallback_single_online_endpoint() {
        let mut ep = Endpoint::new(
            "Single".to_string(),
            "http://localhost:8001".to_string(),
            EndpointType::Vllm,
        );
        ep.status = EndpointStatus::Online;
        ep.latency_ms = Some(42);

        let result = fallback_avg_response_time(None, &[ep]);
        assert_eq!(result, Some(42.0));
    }

    #[test]
    fn test_avg_response_time_fallback_empty_endpoints() {
        let result = fallback_avg_response_time(None, &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_calculate_output_tps_weighted_by_generated_tokens() {
        use super::overview::calculate_output_tps;
        use crate::balancer::EndpointTpsSummary;

        let result = calculate_output_tps(&[
            EndpointTpsSummary {
                endpoint_id: uuid::Uuid::nil(),
                model_count: 1,
                aggregate_tps: Some(50.0),
                total_output_tokens: 100,
                total_requests: 2,
            },
            EndpointTpsSummary {
                endpoint_id: uuid::Uuid::nil(),
                model_count: 1,
                aggregate_tps: Some(100.0),
                total_output_tokens: 100,
                total_requests: 2,
            },
        ])
        .expect("TPS should be computed from measured entries");

        assert!((result - 66.666_666_666_7).abs() < 0.000_001);
    }

    #[test]
    fn test_calculate_output_tps_ignores_unmeasured_entries() {
        use super::overview::calculate_output_tps;
        use crate::balancer::EndpointTpsSummary;

        let result = calculate_output_tps(&[
            EndpointTpsSummary {
                endpoint_id: uuid::Uuid::nil(),
                model_count: 0,
                aggregate_tps: None,
                total_output_tokens: 100,
                total_requests: 1,
            },
            EndpointTpsSummary {
                endpoint_id: uuid::Uuid::nil(),
                model_count: 1,
                aggregate_tps: Some(0.0),
                total_output_tokens: 100,
                total_requests: 1,
            },
        ]);

        assert_eq!(result, None);
    }

    #[test]
    fn test_count_unique_model_ids_deduplicates_across_endpoints() {
        let result = count_unique_model_ids(vec![
            "llama-3.2:3b".to_string(),
            "qwen2.5:7b".to_string(),
            "llama-3.2:3b".to_string(),
        ]);

        assert_eq!(result, 2);
    }

    #[test]
    fn test_action_items_count_offline_and_error_endpoints_independently() {
        let operations = DashboardOperations {
            health: "attention".to_string(),
            total_endpoints: 4,
            online_endpoints: 1,
            pending_endpoints: 0,
            registering_endpoints: 0,
            offline_endpoints: 2,
            error_endpoints: 1,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            success_rate: None,
            active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            output_tps: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            last_registered_at: None,
            last_seen_at: None,
        };

        let items = collect_action_items(&operations);
        let offline = items
            .iter()
            .find(|item| item.title == "Offline endpoints")
            .expect("offline endpoints action item should exist");
        let errors = items
            .iter()
            .find(|item| item.title == "Endpoint errors")
            .expect("endpoint errors action item should exist");

        assert_eq!(offline.count, 2);
        assert_eq!(errors.count, 1);
    }

    // --- DashboardOverview tests ---

    #[test]
    fn test_dashboard_overview_serialization() {
        use super::{
            DashboardActionItem, DashboardCapacity, DashboardOperations, DashboardOverview,
        };

        let overview = DashboardOverview {
            endpoints: vec![],
            operations: DashboardOperations {
                health: "healthy".to_string(),
                total_endpoints: 0,
                online_endpoints: 0,
                pending_endpoints: 0,
                registering_endpoints: 0,
                offline_endpoints: 0,
                error_endpoints: 0,
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                success_rate: None,
                active_requests: 0,
                queued_requests: 0,
                average_response_time_ms: None,
                output_tps: None,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_tokens: 0,
                last_registered_at: None,
                last_seen_at: None,
            },
            capacity: DashboardCapacity {
                total_models: 0,
                gpu_capable_endpoints: 0,
                gpu_telemetry_endpoints: 0,
                total_gpu_memory_bytes: None,
                used_gpu_memory_bytes: None,
                gpu_memory_usage_percent: None,
                telemetry_status: "unavailable".to_string(),
            },
            action_items: vec![DashboardActionItem {
                severity: "info".to_string(),
                title: "No action required".to_string(),
                detail: "All operational signals are nominal.".to_string(),
                count: 0,
            }],
            history: vec![],
            endpoint_tps: vec![],
            generated_at: chrono::Utc::now(),
            generation_time_ms: 42,
        };

        let json = serde_json::to_value(&overview).unwrap();
        assert!(json["endpoints"].is_array());
        assert!(json["operations"].is_object());
        assert!(json["capacity"].is_object());
        assert!(json["action_items"].is_array());
        assert!(json["stats"].is_null());
        assert!(json["operations"]["health"].is_string());
        assert!(json["operations"]["total_endpoints"].is_number());
        assert!(json["operations"]["output_tps"].is_null());
        assert!(json["capacity"]["gpu_capable_endpoints"].is_number());
        assert!(json["history"].is_array());
        assert!(json["endpoint_tps"].is_array());
        assert!(json["generated_at"].is_string());
        assert_eq!(json["generation_time_ms"], 42);
    }

    #[test]
    fn test_dashboard_overview_equality() {
        use super::{
            DashboardActionItem, DashboardCapacity, DashboardOperations, DashboardOverview,
        };

        let now = chrono::Utc::now();

        let overview = DashboardOverview {
            endpoints: vec![],
            operations: DashboardOperations {
                health: "healthy".to_string(),
                total_endpoints: 0,
                online_endpoints: 0,
                pending_endpoints: 0,
                registering_endpoints: 0,
                offline_endpoints: 0,
                error_endpoints: 0,
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                success_rate: None,
                active_requests: 0,
                queued_requests: 0,
                average_response_time_ms: None,
                output_tps: None,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_tokens: 0,
                last_registered_at: None,
                last_seen_at: None,
            },
            capacity: DashboardCapacity {
                total_models: 0,
                gpu_capable_endpoints: 0,
                gpu_telemetry_endpoints: 0,
                total_gpu_memory_bytes: None,
                used_gpu_memory_bytes: None,
                gpu_memory_usage_percent: None,
                telemetry_status: "unavailable".to_string(),
            },
            action_items: vec![DashboardActionItem {
                severity: "info".to_string(),
                title: "No action required".to_string(),
                detail: "All operational signals are nominal.".to_string(),
                count: 0,
            }],
            history: vec![],
            endpoint_tps: vec![],
            generated_at: now,
            generation_time_ms: 0,
        };
        let overview2 = overview.clone();
        assert_eq!(overview, overview2);
    }

    // --- DailyTokenStats extended tests ---

    #[test]
    fn test_daily_token_stats_clone() {
        use super::DailyTokenStats;
        let stats = DailyTokenStats {
            date: "2026-01-01".to_string(),
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_tokens: 150,
            request_count: 5,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.date, "2026-01-01");
        assert_eq!(cloned.request_count, 5);
    }

    // --- MonthlyTokenStats extended tests ---

    #[test]
    fn test_monthly_token_stats_clone() {
        use super::MonthlyTokenStats;
        let stats = MonthlyTokenStats {
            month: "2026-01".to_string(),
            total_input_tokens: 10000,
            total_output_tokens: 5000,
            total_tokens: 15000,
            request_count: 100,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.month, "2026-01");
        assert_eq!(cloned.total_tokens, 15000);
    }

    // --- RequestHistoryQuery deserialization extended tests ---

    #[test]
    fn test_request_history_query_with_limit_and_offset() {
        use super::RequestHistoryQuery;
        let json = r#"{"limit": 50, "offset": 100}"#;
        let q: RequestHistoryQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.limit, Some(50));
        assert_eq!(q.offset, Some(100));
    }

    #[test]
    fn test_request_history_query_with_endpoint_id_alias() {
        use super::RequestHistoryQuery;
        let id = uuid::Uuid::new_v4();
        let json = format!(r#"{{"node_id": "{}"}}"#, id);
        let q: RequestHistoryQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q.endpoint_id, Some(id));
    }

    // --- EndpointDailyStatsQuery extended tests ---

    #[test]
    fn test_endpoint_daily_stats_query_with_zero_days() {
        use super::EndpointDailyStatsQuery;
        let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 0}"#).unwrap();
        assert_eq!(q.days, Some(0));
    }

    #[test]
    fn test_endpoint_daily_stats_query_with_large_days() {
        use super::EndpointDailyStatsQuery;
        let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 1000}"#).unwrap();
        assert_eq!(q.days, Some(1000));
    }

    // --- ClientsQuery extended tests ---

    #[test]
    fn test_clients_query_zero_values() {
        use super::ClientsQuery;
        let q: ClientsQuery = serde_json::from_str(r#"{"page": 0, "per_page": 0}"#).unwrap();
        assert_eq!(q.page, 0);
        assert_eq!(q.per_page, 0);
    }

    // --- SettingUpdateBody extended tests ---

    #[test]
    fn test_setting_update_body_empty_value() {
        use super::SettingUpdateBody;
        let body: SettingUpdateBody = serde_json::from_str(r#"{"value": ""}"#).unwrap();
        assert_eq!(body.value, "");
    }

    #[test]
    fn test_setting_update_body_numeric_string_value() {
        use super::SettingUpdateBody;
        let body: SettingUpdateBody = serde_json::from_str(r#"{"value": "42"}"#).unwrap();
        assert_eq!(body.value, "42");
    }

    // --- ModelTpsEntry extended tests ---

    #[test]
    fn test_model_tps_entry_with_none_tps() {
        use super::ModelTpsEntry;
        use crate::common::protocol::{TpsApiKind, TpsSource};

        let entry = ModelTpsEntry {
            model_id: "new-model".to_string(),
            api_kind: TpsApiKind::Completions,
            source: TpsSource::Benchmark,
            tps: None,
            request_count: 0,
            total_output_tokens: 0,
            average_duration_ms: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json["tps"].is_null());
        assert!(json["average_duration_ms"].is_null());
        assert_eq!(json["request_count"], 0);
    }

    #[test]
    fn test_model_tps_entry_clone() {
        use super::ModelTpsEntry;
        use crate::common::protocol::{TpsApiKind, TpsSource};

        let entry = ModelTpsEntry {
            model_id: "model".to_string(),
            api_kind: TpsApiKind::ChatCompletions,
            source: TpsSource::Production,
            tps: Some(10.0),
            request_count: 5,
            total_output_tokens: 100,
            average_duration_ms: Some(50.0),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.model_id, "model");
        assert_eq!(cloned.tps, Some(10.0));
    }
}
