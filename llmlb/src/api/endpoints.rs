//! エンドポイント管理API
//!
//! SPEC-e8e9326e: llmlb主導エンドポイント登録システム

use super::error::AppError;
use crate::common::auth::{Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::db::endpoints as db;
use crate::detection::{detect_endpoint_type_with_client, DetectionError};
use crate::types::endpoint::Endpoint;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use reqwest::Url;
use uuid::Uuid;

mod connection;
mod dto;
pub use connection::test_endpoint;
pub use dto::*;
mod proxy;
pub use proxy::proxy_chat_completions;
mod downloads;
pub use downloads::*;
mod models;
pub use models::{list_endpoint_models, sync_endpoint_models};
mod registration;
pub use registration::create_endpoint;

/// Admin権限を確認
fn ensure_admin(claims: &Claims) -> Result<(), AppError> {
    if claims.role != UserRole::Admin {
        return Err(AppError(LbError::Authorization(
            "Admin permission required".to_string(),
        )));
    }
    Ok(())
}

// --- Handlers ---

/// GET /api/endpoints - エンドポイント一覧
pub async fn list_endpoints(
    State(state): State<AppState>,
    Query(query): Query<ListEndpointsQuery>,
) -> impl IntoResponse {
    match db::list_endpoints(&state.db_pool).await {
        Ok(endpoints) => {
            // ステータスでフィルタ
            let mut filtered_endpoints: Vec<Endpoint> = if let Some(ref status) = query.status {
                endpoints
                    .into_iter()
                    .filter(|ep| ep.status.as_str() == status)
                    .collect()
            } else {
                endpoints
            };

            // SPEC-e8e9326e: タイプでフィルタ
            if let Some(ref endpoint_type) = query.endpoint_type {
                filtered_endpoints.retain(|ep| ep.endpoint_type.as_str() == endpoint_type);
            }

            let total = filtered_endpoints.len();
            let mut response_endpoints = Vec::with_capacity(total);

            for ep in filtered_endpoints {
                let ep_id = ep.id;
                let mut response = EndpointResponse::from(ep);

                // モデル数を取得
                if let Ok(models) = db::list_endpoint_models(&state.db_pool, ep_id).await {
                    response.model_count = Some(models.len());
                } else {
                    response.model_count = Some(0);
                }

                response_endpoints.push(response);
            }

            let response = ListEndpointsResponse {
                endpoints: response_endpoints,
                total,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list endpoints: {}", e);
            AppError(LbError::Database("Failed to list endpoints".to_string())).into_response()
        }
    }
}

/// GET /api/endpoints/:id - エンドポイント詳細
pub async fn get_endpoint(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(endpoint)) => {
            // モデル一覧も取得して詳細レスポンスに含める
            let models = match db::list_endpoint_models(&state.db_pool, id).await {
                Ok(m) => Some(m.into_iter().map(EndpointModelResponse::from).collect()),
                Err(_) => None,
            };
            let mut response = EndpointResponse::from(endpoint);
            response.models = models;
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint: {}", e);
            AppError(LbError::Database("Failed to get endpoint".to_string())).into_response()
        }
    }
}

/// PUT /api/endpoints/:id - エンドポイント更新
pub async fn update_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateEndpointRequest>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // 既存のエンドポイントを取得
    let existing = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for update: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    // 名前のバリデーション（空文字列は不許可）
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return AppError(LbError::Common(CommonError::Validation(
                "Name cannot be empty".to_string(),
            )))
            .into_response();
        }
    }

    // URL形式チェック
    if let Some(ref url) = req.base_url {
        if Url::parse(url).is_err() {
            return AppError(LbError::Common(CommonError::Validation(
                "Invalid URL format".to_string(),
            )))
            .into_response();
        }
    }

    // 名前変更時の重複チェック（他のエンドポイントと重複していないか）
    if let Some(ref new_name) = req.name {
        if new_name != &existing.name {
            match db::find_by_name(&state.db_pool, new_name).await {
                Ok(Some(_)) => {
                    return AppError(LbError::Common(CommonError::Validation(format!(
                        "Endpoint with name '{}' already exists",
                        new_name
                    ))))
                    .into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to check name uniqueness: {}", e);
                    return AppError(LbError::Database(
                        "Failed to check name uniqueness".to_string(),
                    ))
                    .into_response();
                }
                Ok(None) => {} // OK - 名前は一意
            }
        }
    }

    // 更新内容を適用
    let original_base_url = existing.base_url.clone();
    let mut updated = existing;
    if let Some(name) = req.name {
        updated.name = name;
    }
    if let Some(base_url) = req.base_url {
        updated.base_url = base_url;
    }
    if let Some(api_key) = req.api_key {
        updated.api_key = Some(api_key);
    }
    if let Some(interval) = req.health_check_interval_secs {
        updated.health_check_interval_secs = interval;
    }
    if let Some(timeout) = req.inference_timeout_secs {
        updated.inference_timeout_secs = timeout;
    }
    // notes: None=未指定(そのまま), Some(None)=削除, Some(Some(v))=設定
    if let Some(notes_value) = req.notes {
        updated.notes = notes_value;
    }

    // SPEC-e8e9326e: base_url変更時はタイプを再検出
    if updated.base_url != original_base_url {
        let detection_result = detect_endpoint_type_with_client(
            &state.http_client,
            &updated.base_url,
            updated.api_key.as_deref(),
        )
        .await;

        match detection_result {
            Ok(result) => {
                updated.endpoint_type = result.endpoint_type;
            }
            Err(DetectionError::Unreachable(msg)) => {
                return AppError(LbError::Http(format!("Endpoint unreachable: {}", msg)))
                    .into_response();
            }
            Err(DetectionError::UnsupportedType(msg)) => {
                return AppError(LbError::Common(CommonError::Validation(format!(
                    "Unsupported endpoint type: {}",
                    msg
                ))))
                .into_response();
            }
        }
    }

    match state.endpoint_registry.update(updated.clone()).await {
        Ok(true) => (StatusCode::OK, Json(EndpointResponse::from(updated))).into_response(),
        Ok(false) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("UNIQUE constraint failed") {
                AppError(LbError::Conflict(
                    "Endpoint with this name or URL already exists".to_string(),
                ))
                .into_response()
            } else {
                tracing::error!("Failed to update endpoint: {}", e);
                AppError(LbError::Database("Failed to update endpoint".to_string())).into_response()
            }
        }
    }
}

/// DELETE /api/endpoints/:id - エンドポイント削除
pub async fn delete_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // arch-review [L12]: 削除は registry と LoadManager の状態掃除を単一の調整メソッドに
    // 集約した remove_endpoint_fully を通す。
    match remove_endpoint_fully(&state, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to delete endpoint: {}", e);
            AppError(LbError::Database("Failed to delete endpoint".to_string())).into_response()
        }
    }
}

/// エンドポイントをレジストリ・DB・LoadManager 状態から一括削除する調整メソッド。
///
/// arch-review [L12]: エンドポイントの runtime 状態が EndpointRegistry と LoadManager に
/// 分散し、削除時に呼び出し側が両方を手で掃除する必要があった。EndpointRegistry::remove
/// は LoadManager の負荷/TPS 状態までは掃除しないため、両者の掃除を本メソッドへ集約し、
/// 削除経路が LoadManager 状態の破棄を取りこぼさないようにする。
async fn remove_endpoint_fully(state: &AppState, id: Uuid) -> Result<bool, sqlx::Error> {
    let removed = state.endpoint_registry.remove(id).await?;
    if removed {
        // 負荷状態・TPS状態がリークしないよう明示的に破棄する。
        state.load_manager.forget_endpoint(id).await;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests;
