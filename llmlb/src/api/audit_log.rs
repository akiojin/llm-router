//! 監査ログAPIハンドラー (SPEC-8301d106)
//!
//! `/api/dashboard/audit-logs` 系のエンドポイント

use super::error::AppError;
use crate::audit::hash_chain::{self, ChainVerificationResult};
use crate::audit::types::{AuditLogEntry, AuditLogFilter};
use crate::AuditState;
use axum::{
    extract::{Query, State},
    Json,
};
use chrono::Utc;

mod archive;
mod types;
use archive::query_with_archive;
#[cfg(test)]
use archive::{clamp_archive_total, MAX_AUDIT_FETCH_LIMIT};
pub use types::{
    ActorTypeCount, AuditLogListResponse, AuditLogQueryParams, AuditLogStatsResponse, MethodCount,
};

/// per_page の上限（監査ログテーブル全行のメモリ展開によるDoSを防ぐ）
const MAX_AUDIT_PER_PAGE: i64 = 200;
/// page の上限（offset = (page-1)*per_page の乗算オーバーフローを防ぐ）
const MAX_AUDIT_PAGE: i64 = 1_000_000;

/// GET /api/dashboard/audit-logs - 監査ログ一覧取得
pub async fn list_audit_logs(
    State(state): State<AuditState>,
    Query(params): Query<AuditLogQueryParams>,
) -> Result<Json<AuditLogListResponse>, AppError> {
    // フォーマット検証（現在はJSONのみ対応）
    if let Some(ref fmt) = params.format {
        if fmt != "json" {
            return Err(AppError(crate::common::error::LbError::Common(
                crate::common::error::CommonError::Validation(format!(
                    "Unsupported format: '{}'. Only 'json' is currently supported.",
                    fmt
                )),
            )));
        }
    }

    let filter: AuditLogFilter = params.into();
    // レスポンスにはクランプ後の実効値を返す（生入力ではなく実際に使用した値）
    let page = filter.page.unwrap_or(1);
    let per_page = filter.per_page.unwrap_or(50);
    let include_archive = filter.include_archive.unwrap_or(false);
    let search_text = filter.search_text.clone();
    let storage = &state.storage;

    let (items, total) = match (include_archive, state.archive_pool.as_ref()) {
        (true, Some(archive_pool)) => {
            query_with_archive(storage, archive_pool, &filter, search_text.as_deref()).await?
        }
        _ => {
            if let Some(ref query) = search_text {
                let items = storage.search_fts(query, &filter).await?;
                let total = storage.count_fts(query, &filter).await?;
                (items, total)
            } else {
                let items = storage.query(&filter).await?;
                let total = storage.count(&filter).await?;
                (items, total)
            }
        }
    };

    Ok(Json(AuditLogListResponse {
        items,
        total,
        page,
        per_page,
    }))
}

/// GET /api/dashboard/audit-logs/stats - 監査ログ統計取得
pub async fn get_audit_log_stats(
    State(state): State<AuditState>,
) -> Result<Json<AuditLogStatsResponse>, AppError> {
    let storage = &state.storage;

    let total_entries = storage.count(&AuditLogFilter::default()).await?;

    let last_24h_filter = AuditLogFilter {
        time_from: Some(Utc::now() - chrono::Duration::hours(24)),
        ..Default::default()
    };
    let last_24h = storage.count(&last_24h_filter).await?;

    let by_method = storage.count_by_method().await?;
    let by_actor_type = storage.count_by_actor_type().await?;

    Ok(Json(AuditLogStatsResponse {
        total_entries,
        by_method: by_method
            .into_iter()
            .map(|(method, count)| MethodCount { method, count })
            .collect(),
        by_actor_type: by_actor_type
            .into_iter()
            .map(|(actor_type, count)| ActorTypeCount { actor_type, count })
            .collect(),
        last_24h,
    }))
}

/// POST /api/dashboard/audit-logs/verify - ハッシュチェーン検証
pub async fn verify_hash_chain(
    State(state): State<AuditState>,
) -> Result<Json<ChainVerificationResult>, AppError> {
    let result = hash_chain::verify_chain(&state.storage).await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests;
