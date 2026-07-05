//! APIキー管理API
//!
//! 認証済みユーザーが自分自身のAPIキーを管理するためのAPI。

use crate::common::auth::{ApiKey, ApiKeyPermission, ApiKeyWithPlaintext, Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};

use super::error::AppError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// APIキー作成リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
    /// 付与する権限（adminのみ指定可）
    #[serde(default)]
    pub permissions: Option<Vec<ApiKeyPermission>>,
    /// 旧互換: `scopes` は廃止
    #[serde(default)]
    pub scopes: Option<serde_json::Value>,
}

/// APIキーレスポンス（key_hash除外）
#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    /// APIキーID
    pub id: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: Option<String>,
    /// キーの名前
    pub name: String,
    /// 作成者のユーザーID
    pub created_by: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(api_key: ApiKey) -> Self {
        ApiKeyResponse {
            id: api_key.id.to_string(),
            key_prefix: api_key.key_prefix,
            name: api_key.name,
            created_by: api_key.created_by.to_string(),
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            permissions: api_key.permissions,
        }
    }
}

/// APIキー作成レスポンス（平文キー含む）
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    /// APIキーID
    pub id: String,
    /// 平文のAPIキー（発行時のみ表示）
    pub key: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: String,
    /// キーの名前
    pub name: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

impl From<ApiKeyWithPlaintext> for CreateApiKeyResponse {
    fn from(api_key: ApiKeyWithPlaintext) -> Self {
        CreateApiKeyResponse {
            id: api_key.id.to_string(),
            key: api_key.key,
            key_prefix: api_key.key_prefix,
            name: api_key.name,
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            permissions: api_key.permissions,
        }
    }
}

/// APIキー一覧レスポンス
#[derive(Debug, Serialize)]
pub struct ListApiKeysResponse {
    /// APIキー一覧
    pub api_keys: Vec<ApiKeyResponse>,
}

/// APIキー更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
}

fn default_viewer_api_key_permissions() -> Vec<ApiKeyPermission> {
    vec![
        ApiKeyPermission::OpenaiInference,
        ApiKeyPermission::OpenaiModelsRead,
    ]
}

#[allow(clippy::result_large_err)]
fn resolve_permissions_for_role(
    role: UserRole,
    requested_permissions: Option<Vec<ApiKeyPermission>>,
) -> Result<Vec<ApiKeyPermission>, Response> {
    match role {
        UserRole::Admin => {
            let permissions = requested_permissions.ok_or_else(|| {
                AppError(LbError::Common(CommonError::Validation(
                    "Field 'permissions' is required for admin users.".to_string(),
                )))
                .into_response()
            })?;

            if permissions.is_empty() {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "Field 'permissions' must contain at least one permission.".to_string(),
                )))
                .into_response());
            }

            Ok(permissions)
        }
        UserRole::Viewer => {
            if requested_permissions.is_some() {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "Viewer users cannot provide 'permissions'; viewer keys always use fixed OpenAI permissions."
                        .to_string(),
                )))
                .into_response());
            }

            Ok(default_viewer_api_key_permissions())
        }
    }
}

#[allow(clippy::result_large_err)]
fn parse_user_id_from_claims(claims: &Claims) -> Result<Uuid, Response> {
    claims.sub.parse::<Uuid>().map_err(|e| {
        tracing::error!("Failed to parse user ID: {}", e);
        AppError(LbError::Internal(format!("Failed to parse user ID: {}", e))).into_response()
    })
}

#[allow(clippy::result_large_err)]
fn parse_expires_at(
    expires_at: Option<&String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, Response> {
    match expires_at {
        Some(expires_at_str) => Ok(Some(
            chrono::DateTime::parse_from_rfc3339(expires_at_str)
                .map_err(|e| {
                    tracing::warn!("Invalid expires_at format: {}", e);
                    AppError(LbError::Common(CommonError::Validation(
                        "Invalid expires_at format".to_string(),
                    )))
                    .into_response()
                })?
                .with_timezone(&chrono::Utc),
        )),
        None => Ok(None),
    }
}

/// GET /api/me/api-keys - 自分のAPIキー一覧取得
pub async fn list_api_keys(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
) -> Result<Json<ListApiKeysResponse>, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;

    let api_keys = crate::db::api_keys::list_by_creator(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list API keys: {}", e);
            AppError(LbError::Database(format!("Failed to list API keys: {}", e))).into_response()
        })?;

    Ok(Json(ListApiKeysResponse {
        api_keys: api_keys.into_iter().map(ApiKeyResponse::from).collect(),
    }))
}

/// POST /api/me/api-keys - APIキー発行
pub async fn create_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), Response> {
    if request.scopes.is_some() {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Field 'scopes' is deprecated and not accepted.".to_string(),
        )))
        .into_response());
    }

    let permissions = resolve_permissions_for_role(claims.role, request.permissions)?;
    let user_id = parse_user_id_from_claims(&claims)?;
    let expires_at = parse_expires_at(request.expires_at.as_ref())?;

    let api_key = crate::db::api_keys::create(
        &app_state.db_pool,
        &request.name,
        user_id,
        expires_at,
        permissions,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create API key: {}", e);
        AppError(e).into_response()
    })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse::from(api_key)),
    ))
}

/// PUT /api/me/api-keys/:id - 自分のAPIキー更新
pub async fn update_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;
    let expires_at = parse_expires_at(request.expires_at.as_ref())?;

    let updated = crate::db::api_keys::update_by_creator(
        &app_state.db_pool,
        key_id,
        user_id,
        &request.name,
        expires_at,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to update API key: {}", e);
        AppError(e).into_response()
    })?;

    match updated {
        Some(api_key) => Ok(Json(ApiKeyResponse::from(api_key))),
        None => Err(AppError(LbError::NotFound("API key not found".to_string())).into_response()),
    }
}

/// DELETE /api/me/api-keys/:id - 自分のAPIキー削除
pub async fn delete_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;

    let deleted = crate::db::api_keys::delete_by_creator(&app_state.db_pool, key_id, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete API key: {}", e);
            AppError(LbError::Database(format!(
                "Failed to delete API key: {}",
                e
            )))
            .into_response()
        })?;

    if !deleted {
        return Err(AppError(LbError::NotFound("API key not found".to_string())).into_response());
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
