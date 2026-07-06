//! エンドポイントのモデルカタログ操作。
//!
//! エンドポイントが提供するモデル一覧の同期（sync）と取得（list）ハンドラ。

use super::dto::{EndpointModelResponse, EndpointModelsResponse, SyncModelsResponse};
use super::ensure_admin;
use crate::api::error::AppError;
use crate::common::auth::Claims;
use crate::common::error::LbError;
use crate::db::endpoints as db;
use crate::sync::{self, SyncError};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use uuid::Uuid;

/// POST /api/endpoints/:id/sync - モデル一覧同期
pub async fn sync_endpoint_models(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // エンドポイントを取得
    let endpoint = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for sync: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    let result = sync::sync_models_with_type(
        &state.db_pool,
        &state.http_client,
        id,
        &endpoint.base_url,
        endpoint.api_key.as_deref(),
        endpoint.inference_timeout_secs as u64,
        Some(endpoint.endpoint_type),
    )
    .await;

    match result {
        Ok(result) => {
            // EndpointRegistryキャッシュをリロードしてモデルマッピングを更新
            let _ = state.endpoint_registry.reload().await;

            let synced_models = result
                .models
                .into_iter()
                .map(EndpointModelResponse::from)
                .collect();

            (
                StatusCode::OK,
                Json(SyncModelsResponse {
                    synced_models,
                    added: result.added,
                    removed: result.removed,
                    updated: result.updated,
                }),
            )
                .into_response()
        }
        Err(err) => {
            let lb_error = match err {
                SyncError::ConnectionError(msg) => {
                    LbError::ServiceUnavailable(format!("Failed to connect: {}", msg))
                }
                SyncError::HttpError(status, _) => {
                    LbError::Http(format!("Endpoint returned HTTP {}", status))
                }
                SyncError::ParseError(msg) => {
                    LbError::Http(format!("Failed to parse response: {}", msg))
                }
                SyncError::DbError(msg) => {
                    LbError::Database(format!("Failed to update models: {}", msg))
                }
            };

            AppError(lb_error).into_response()
        }
    }
}

/// GET /api/endpoints/:id/models - エンドポイントのモデル一覧
pub async fn list_endpoint_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // エンドポイント存在確認
    match db::get_endpoint(&state.db_pool, id).await {
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
        Ok(Some(_)) => {}
    }

    match db::list_endpoint_models(&state.db_pool, id).await {
        Ok(models) => (
            StatusCode::OK,
            Json(EndpointModelsResponse {
                endpoint_id: id,
                models: models
                    .into_iter()
                    .map(EndpointModelResponse::from)
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to list endpoint models: {}", e);
            AppError(LbError::Database("Failed to list models".to_string())).into_response()
        }
    }
}
