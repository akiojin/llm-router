//! ユーザー管理API
//!
//! Admin専用のユーザーCRUD操作

use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use ollama_coordinator_common::auth::{Claims, User, UserRole};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// ユーザー作成リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    /// ユーザー名
    pub username: String,
    /// パスワード
    pub password: String,
    /// ロール
    pub role: UserRole,
}

/// ユーザー更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    /// ユーザー名（オプション）
    pub username: Option<String>,
    /// パスワード（オプション）
    pub password: Option<String>,
    /// ロール（オプション）
    pub role: Option<UserRole>,
}

/// ユーザーレスポンス（password_hash除外）
#[derive(Debug, Serialize)]
pub struct UserResponse {
    /// ユーザーID
    pub id: String,
    /// ユーザー名
    pub username: String,
    /// ロール
    pub role: String,
    /// 作成日時
    pub created_at: String,
    /// 最終ログイン日時
    pub last_login: Option<String>,
}

/// ユーザー一覧レスポンス
#[derive(Debug, Serialize)]
pub struct ListUsersResponse {
    /// ユーザー一覧
    pub users: Vec<UserResponse>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id.to_string(),
            username: user.username,
            role: format!("{:?}", user.role).to_lowercase(),
            created_at: user.created_at.to_rfc3339(),
            last_login: user.last_login.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Admin権限チェックヘルパー
#[allow(clippy::result_large_err)]
fn check_admin(claims: &Claims) -> Result<(), Response> {
    if claims.role != UserRole::Admin {
        return Err((StatusCode::FORBIDDEN, "Admin access required").into_response());
    }
    Ok(())
}

/// GET /api/users - ユーザー一覧取得
///
/// Admin専用。全ユーザーの一覧を返す（パスワードハッシュは除外）
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
///
/// # Returns
/// * `200 OK` - ユーザー一覧
/// * `403 Forbidden` - Admin権限なし
/// * `500 Internal Server Error` - サーバーエラー
pub async fn list_users(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
) -> Result<Json<ListUsersResponse>, Response> {
    check_admin(&claims)?;

    let users = crate::db::users::list(&app_state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list users: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    Ok(Json(ListUsersResponse {
        users: users.into_iter().map(UserResponse::from).collect(),
    }))
}

/// POST /api/users - ユーザー作成
///
/// Admin専用。新しいユーザーを作成する
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - ユーザー作成リクエスト
///
/// # Returns
/// * `201 Created` - 作成されたユーザー
/// * `400 Bad Request` - ユーザー名重複等
/// * `403 Forbidden` - Admin権限なし
/// * `500 Internal Server Error` - サーバーエラー
pub async fn create_user(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), Response> {
    check_admin(&claims)?;

    // ユーザー名の重複チェック
    let existing = crate::db::users::find_by_username(&app_state.db_pool, &request.username)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check username: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    if existing.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Username already exists").into_response());
    }

    // パスワードをハッシュ化
    let password_hash = crate::auth::password::hash_password(&request.password).map_err(|e| {
        tracing::error!("Failed to hash password: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    // ユーザーを作成
    let user = crate::db::users::create(
        &app_state.db_pool,
        &request.username,
        &password_hash,
        request.role,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create user: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

/// PUT /api/users/:id - ユーザー更新
///
/// Admin専用。既存ユーザーの情報を更新する
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Path(user_id)` - ユーザーID
/// * `Json(request)` - ユーザー更新リクエスト
///
/// # Returns
/// * `200 OK` - 更新されたユーザー
/// * `400 Bad Request` - ユーザー名重複等
/// * `403 Forbidden` - Admin権限なし
/// * `404 Not Found` - ユーザーが見つからない
/// * `500 Internal Server Error` - サーバーエラー
pub async fn update_user(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, Response> {
    check_admin(&claims)?;

    // ユーザーの存在確認
    crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "User not found").into_response())?;

    // ユーザー名の重複チェック
    if let Some(ref username) = request.username {
        if let Some(existing) = crate::db::users::find_by_username(&app_state.db_pool, username)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check username: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            })?
        {
            if existing.id != user_id {
                return Err((StatusCode::BAD_REQUEST, "Username already exists").into_response());
            }
        }
    }

    // パスワードをハッシュ化（指定された場合）
    let password_hash = if let Some(ref password) = request.password {
        Some(crate::auth::password::hash_password(password).map_err(|e| {
            tracing::error!("Failed to hash password: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?)
    } else {
        None
    };

    // ユーザーを更新
    let user = crate::db::users::update(
        &app_state.db_pool,
        user_id,
        request.username.as_deref(),
        password_hash.as_deref(),
        request.role,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to update user: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    Ok(Json(UserResponse::from(user)))
}

/// DELETE /api/users/:id - ユーザー削除
///
/// Admin専用。ユーザーを削除する。最後の管理者は削除不可
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Path(user_id)` - ユーザーID
///
/// # Returns
/// * `204 No Content` - 削除成功
/// * `400 Bad Request` - 最後の管理者
/// * `403 Forbidden` - Admin権限なし
/// * `404 Not Found` - ユーザーが見つからない
/// * `500 Internal Server Error` - サーバーエラー
pub async fn delete_user(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, Response> {
    check_admin(&claims)?;

    // ユーザーの存在確認
    crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "User not found").into_response())?;

    // 最後の管理者チェック
    let is_last_admin = crate::db::users::is_last_admin(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check if last admin: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    if is_last_admin {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot delete the last administrator",
        )
            .into_response());
    }

    // ユーザーを削除
    crate::db::users::delete(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete user: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?;

    Ok(StatusCode::NO_CONTENT)
}
