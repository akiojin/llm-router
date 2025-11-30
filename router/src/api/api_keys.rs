//! APIキー管理API
//!
//! Admin専用のAPIキーCRUD操作

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use llm_router_common::auth::{ApiKey, ApiKeyWithPlaintext, Claims, UserRole};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// APIキー作成リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
}

/// APIキーレスポンス（key_hash除外）
#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    /// APIキーID
    pub id: String,
    /// キーの名前
    pub name: String,
    /// 作成者のユーザーID
    pub created_by: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(api_key: ApiKey) -> Self {
        ApiKeyResponse {
            id: api_key.id.to_string(),
            name: api_key.name,
            created_by: api_key.created_by.to_string(),
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
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
    /// キーの名前
    pub name: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
}

impl From<ApiKeyWithPlaintext> for CreateApiKeyResponse {
    fn from(api_key: ApiKeyWithPlaintext) -> Self {
        CreateApiKeyResponse {
            id: api_key.id.to_string(),
            key: api_key.key,
            name: api_key.name,
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// APIキー一覧レスポンス
#[derive(Debug, Serialize)]
pub struct ListApiKeysResponse {
    /// APIキー一覧
    pub api_keys: Vec<ApiKeyResponse>,
}

/// Admin権限チェックヘルパー
#[allow(clippy::result_large_err)]
fn check_admin(claims: &Claims) -> Result<(), Response> {
    if claims.role != UserRole::Admin {
        return Err((StatusCode::FORBIDDEN, "Admin access required").into_response());
    }
    Ok(())
}

/// GET /api/api-keys - APIキー一覧取得
///
/// Admin専用。全APIキーの一覧を返す（key_hashは除外）
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
///
/// # Returns
/// * `200 OK` - APIキー一覧
/// * `403 Forbidden` - Admin権限なし
/// * `500 Internal Server Error` - サーバーエラー
pub async fn list_api_keys(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
) -> Result<Json<ListApiKeysResponse>, Response> {
    check_admin(&claims)?;

    let api_keys = crate::db::api_keys::list(&app_state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list API keys: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    Ok(Json(ListApiKeysResponse {
        api_keys: api_keys.into_iter().map(ApiKeyResponse::from).collect(),
    }))
}

/// POST /api/api-keys - APIキー発行
///
/// Admin専用。新しいAPIキーを発行する。平文キーは発行時のみ返却
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - APIキー作成リクエスト
///
/// # Returns
/// * `201 Created` - 作成されたAPIキー（平文キー含む）
/// * `400 Bad Request` - 有効期限のフォーマットエラー
/// * `403 Forbidden` - Admin権限なし
/// * `500 Internal Server Error` - サーバーエラー
pub async fn create_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), Response> {
    check_admin(&claims)?;

    // ユーザーIDをパース
    let user_id = claims.sub.parse::<Uuid>().map_err(|e| {
        tracing::error!("Failed to parse user ID: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    // 有効期限をパース（指定された場合）
    let expires_at = if let Some(ref expires_at_str) = request.expires_at {
        Some(
            chrono::DateTime::parse_from_rfc3339(expires_at_str)
                .map_err(|e| {
                    tracing::warn!("Invalid expires_at format: {}", e);
                    (StatusCode::BAD_REQUEST, "Invalid expires_at format").into_response()
                })?
                .with_timezone(&chrono::Utc),
        )
    } else {
        None
    };

    // APIキーを作成
    let api_key =
        crate::db::api_keys::create(&app_state.db_pool, &request.name, user_id, expires_at)
            .await
            .map_err(|e| {
                tracing::error!("Failed to create API key: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse::from(api_key)),
    ))
}

/// APIキー更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
}

/// PUT /api/api-keys/:id - APIキー更新
///
/// Admin専用。APIキーの名前と有効期限を更新する
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Path(key_id)` - APIキーID
/// * `Json(request)` - APIキー更新リクエスト
///
/// # Returns
/// * `200 OK` - 更新後のAPIキー
/// * `400 Bad Request` - 有効期限のフォーマットエラー
/// * `403 Forbidden` - Admin権限なし
/// * `404 Not Found` - APIキーが見つからない
/// * `500 Internal Server Error` - サーバーエラー
pub async fn update_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, Response> {
    check_admin(&claims)?;

    // 有効期限をパース（指定された場合）
    let expires_at = if let Some(ref expires_at_str) = request.expires_at {
        Some(
            chrono::DateTime::parse_from_rfc3339(expires_at_str)
                .map_err(|e| {
                    tracing::warn!("Invalid expires_at format: {}", e);
                    (StatusCode::BAD_REQUEST, "Invalid expires_at format").into_response()
                })?
                .with_timezone(&chrono::Utc),
        )
    } else {
        None
    };

    // APIキーを更新
    let updated =
        crate::db::api_keys::update(&app_state.db_pool, key_id, &request.name, expires_at)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update API key: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            })?;

    match updated {
        Some(api_key) => Ok(Json(ApiKeyResponse::from(api_key))),
        None => Err((StatusCode::NOT_FOUND, "API key not found").into_response()),
    }
}

/// DELETE /api/api-keys/:id - APIキー削除
///
/// Admin専用。APIキーを削除する
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Path(key_id)` - APIキーID
///
/// # Returns
/// * `204 No Content` - 削除成功
/// * `403 Forbidden` - Admin権限なし
/// * `404 Not Found` - APIキーが見つからない
/// * `500 Internal Server Error` - サーバーエラー
pub async fn delete_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, Response> {
    check_admin(&claims)?;

    // APIキーを削除（存在しない場合はエラー）
    crate::db::api_keys::delete(&app_state.db_pool, key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete API key: {}", e);
            // SQLiteの場合、削除対象が存在しない場合でもエラーにならないため、
            // ここでは500エラーとして扱う
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    Ok(StatusCode::NO_CONTENT)
}
