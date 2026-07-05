//! リクエスト応答履歴のページング/フィルタ一覧・詳細・CSV/JSON エクスポート
//!
//! arch-review [H6]: api/dashboard.rs から履歴閲覧/エクスポートのハンドラと
//! クエリ DTO を分離。ページ番号デフォルトは親の default_page を super 経由で参照。

use crate::api::error::AppError;
use crate::common::error::{CommonError, LbError};
use crate::db::request_history::{FilterStatus, RecordFilter};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tracing::warn;
use uuid::Uuid;

/// 許可されたページサイズ
pub const ALLOWED_PAGE_SIZES: &[usize] = &[10, 25, 50, 100];

/// デフォルトのページサイズ
pub const DEFAULT_PAGE_SIZE: usize = 10;

/// リクエスト履歴一覧のクエリパラメータ
#[derive(Debug, Clone, Deserialize)]
pub struct RequestHistoryQuery {
    /// ページ番号（1始まり）
    #[serde(default = "super::default_page")]
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

pub(crate) fn default_per_page() -> usize {
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

    pub(crate) fn to_record_filter(&self) -> Result<RecordFilter, AppError> {
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
    pub(crate) fn to_record_filter(&self) -> Result<RecordFilter, AppError> {
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
