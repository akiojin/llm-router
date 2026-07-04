//! ユーザー管理API
//!
//! Admin専用のユーザーCRUD操作

use crate::common::auth::{Claims, User, UserRole};
use crate::common::error::LbError;
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

/// ユーザー作成リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    /// ユーザー名
    pub username: String,
    /// ロール
    pub role: UserRole,
}

/// ユーザー作成レスポンス（生成パスワード付き）
#[derive(Debug, Serialize)]
pub struct CreateUserResponse {
    /// ユーザー情報
    pub user: UserResponse,
    /// 自動生成されたパスワード（管理者に一度だけ表示）
    pub generated_password: String,
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
    super::check_admin(&claims)?;

    let users = crate::db::users::list(&app_state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list users: {}", e);
            AppError(LbError::Database(format!("Failed to list users: {}", e))).into_response()
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
) -> Result<(StatusCode, Json<CreateUserResponse>), Response> {
    super::check_admin(&claims)?;

    // ユーザー名の重複チェック
    let existing = crate::db::users::find_by_username(&app_state.db_pool, &request.username)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check username: {}", e);
            AppError(LbError::Database(format!(
                "Failed to check username: {}",
                e
            )))
            .into_response()
        })?;

    if existing.is_some() {
        return Err(
            AppError(LbError::Conflict("Username already exists".to_string())).into_response(),
        );
    }

    // パスワードを自動生成
    let generated_password = crate::auth::generate_random_token(16);

    // パスワードをハッシュ化
    let password_hash = crate::auth::password::hash_password(&generated_password).map_err(|e| {
        tracing::error!("Failed to hash password: {}", e);
        AppError(LbError::PasswordHash(format!(
            "Failed to hash password: {}",
            e
        )))
        .into_response()
    })?;

    // ユーザーを作成（初回パスワード変更必須）
    let user = crate::db::users::create(
        &app_state.db_pool,
        &request.username,
        &password_hash,
        request.role,
        true,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create user: {}", e);
        AppError(LbError::Database(format!("Failed to create user: {}", e))).into_response()
    })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse {
            user: UserResponse::from(user),
            generated_password,
        }),
    ))
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
    super::check_admin(&claims)?;

    // ユーザーの存在確認
    crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            AppError(LbError::Database(format!("Failed to find user: {}", e))).into_response()
        })?
        .ok_or_else(|| AppError(LbError::NotFound("User not found".to_string())).into_response())?;

    // ユーザー名の重複チェック
    if let Some(ref username) = request.username {
        if let Some(existing) = crate::db::users::find_by_username(&app_state.db_pool, username)
            .await
            .map_err(|e| {
                tracing::error!("Failed to check username: {}", e);
                AppError(LbError::Database(format!(
                    "Failed to check username: {}",
                    e
                )))
                .into_response()
            })?
        {
            if existing.id != user_id {
                return Err(
                    AppError(LbError::Conflict("Username already exists".to_string()))
                        .into_response(),
                );
            }
        }
    }

    // パスワードをハッシュ化（指定された場合）。要件検証を経てからハッシュ化する。
    let password_hash = if let Some(ref password) = request.password {
        crate::auth::password::validate_password(password)
            .map_err(|e| AppError(e).into_response())?;
        Some(crate::auth::password::hash_password(password).map_err(|e| {
            tracing::error!("Failed to hash password: {}", e);
            AppError(LbError::PasswordHash(format!(
                "Failed to hash password: {}",
                e
            )))
            .into_response()
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
        AppError(LbError::Database(format!("Failed to update user: {}", e))).into_response()
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
    super::check_admin(&claims)?;

    // ユーザーの存在確認
    crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            AppError(LbError::Database(format!("Failed to find user: {}", e))).into_response()
        })?
        .ok_or_else(|| AppError(LbError::NotFound("User not found".to_string())).into_response())?;

    // 最後の管理者チェック
    let is_last_admin = crate::db::users::is_last_admin(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check if last admin: {}", e);
            AppError(LbError::Database(format!(
                "Failed to check if last admin: {}",
                e
            )))
            .into_response()
        })?;

    if is_last_admin {
        return Err(AppError(LbError::Common(
            crate::common::error::CommonError::Validation(
                "Cannot delete the last administrator".to_string(),
            ),
        ))
        .into_response());
    }

    // ユーザーを削除
    crate::db::users::delete(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete user: {}", e);
            AppError(LbError::Database(format!("Failed to delete user: {}", e))).into_response()
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // --- CreateUserRequest deserialization ---

    #[test]
    fn create_user_request_deserializes_admin() {
        let json = r#"{"username":"alice","role":"admin"}"#;
        let req: CreateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "alice");
        assert_eq!(req.role, UserRole::Admin);
    }

    #[test]
    fn create_user_request_deserializes_viewer() {
        let json = r#"{"username":"bob","role":"viewer"}"#;
        let req: CreateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "bob");
        assert_eq!(req.role, UserRole::Viewer);
    }

    #[test]
    fn create_user_request_missing_username_fails() {
        let json = r#"{"role":"admin"}"#;
        let result = serde_json::from_str::<CreateUserRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn create_user_request_missing_role_fails() {
        let json = r#"{"username":"alice"}"#;
        let result = serde_json::from_str::<CreateUserRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn create_user_request_invalid_role_fails() {
        let json = r#"{"username":"alice","role":"superuser"}"#;
        let result = serde_json::from_str::<CreateUserRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn create_user_request_empty_username() {
        let json = r#"{"username":"","role":"admin"}"#;
        let req: CreateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "");
    }

    // --- UpdateUserRequest deserialization ---

    #[test]
    fn update_user_request_all_fields() {
        let json = r#"{"username":"new_name","password":"new_pass","role":"viewer"}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, Some("new_name".to_string()));
        assert_eq!(req.password, Some("new_pass".to_string()));
        assert_eq!(req.role, Some(UserRole::Viewer));
    }

    #[test]
    fn update_user_request_empty_body() {
        let json = r#"{}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert!(req.username.is_none());
        assert!(req.password.is_none());
        assert!(req.role.is_none());
    }

    #[test]
    fn update_user_request_username_only() {
        let json = r#"{"username":"updated"}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, Some("updated".to_string()));
        assert!(req.password.is_none());
        assert!(req.role.is_none());
    }

    #[test]
    fn update_user_request_password_only() {
        let json = r#"{"password":"secret123"}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert!(req.username.is_none());
        assert_eq!(req.password, Some("secret123".to_string()));
    }

    #[test]
    fn update_user_request_role_only() {
        let json = r#"{"role":"admin"}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert!(req.username.is_none());
        assert!(req.password.is_none());
        assert_eq!(req.role, Some(UserRole::Admin));
    }

    #[test]
    fn update_user_request_null_fields() {
        let json = r#"{"username":null,"password":null,"role":null}"#;
        let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
        assert!(req.username.is_none());
        assert!(req.password.is_none());
        assert!(req.role.is_none());
    }

    // --- UserResponse serialization ---

    #[test]
    fn user_response_serialization() {
        let resp = UserResponse {
            id: "123".to_string(),
            username: "alice".to_string(),
            role: "admin".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_login: Some("2025-06-01T12:00:00Z".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":\"123\""));
        assert!(json.contains("\"username\":\"alice\""));
        assert!(json.contains("\"role\":\"admin\""));
        assert!(json.contains("\"last_login\":\"2025-06-01T12:00:00Z\""));
    }

    #[test]
    fn user_response_no_last_login() {
        let resp = UserResponse {
            id: "456".to_string(),
            username: "bob".to_string(),
            role: "viewer".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_login: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"last_login\":null"));
    }

    // --- From<User> for UserResponse ---

    #[test]
    fn user_response_from_admin_user() {
        let user = User {
            id: Uuid::new_v4(),
            username: "admin_user".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::Admin,
            created_at: Utc::now(),
            last_login: Some(Utc::now()),
            must_change_password: false,
            password_changed_at: 0,
        };
        let resp = UserResponse::from(user.clone());
        assert_eq!(resp.id, user.id.to_string());
        assert_eq!(resp.username, "admin_user");
        assert_eq!(resp.role, "admin");
        assert!(resp.last_login.is_some());
    }

    #[test]
    fn user_response_from_viewer_user() {
        let user = User {
            id: Uuid::new_v4(),
            username: "viewer_user".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::Viewer,
            created_at: Utc::now(),
            last_login: None,
            must_change_password: true,
            password_changed_at: 0,
        };
        let resp = UserResponse::from(user.clone());
        assert_eq!(resp.username, "viewer_user");
        assert_eq!(resp.role, "viewer");
        assert!(resp.last_login.is_none());
    }

    #[test]
    fn user_response_created_at_is_rfc3339() {
        let user = User {
            id: Uuid::new_v4(),
            username: "test".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::Admin,
            created_at: Utc::now(),
            last_login: None,
            must_change_password: false,
            password_changed_at: 0,
        };
        let resp = UserResponse::from(user);
        // RFC 3339 timestamps contain "T" and "+" or "Z"
        assert!(resp.created_at.contains('T'));
    }

    #[test]
    fn user_response_last_login_rfc3339() {
        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            username: "test".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::Admin,
            created_at: now,
            last_login: Some(now),
            must_change_password: false,
            password_changed_at: 0,
        };
        let resp = UserResponse::from(user);
        let ll = resp.last_login.unwrap();
        assert!(ll.contains('T'));
    }

    // --- CreateUserResponse serialization ---

    #[test]
    fn create_user_response_serialization() {
        let resp = CreateUserResponse {
            user: UserResponse {
                id: "1".to_string(),
                username: "alice".to_string(),
                role: "admin".to_string(),
                created_at: "2025-01-01T00:00:00Z".to_string(),
                last_login: None,
            },
            generated_password: "random_password_123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"generated_password\":\"random_password_123\""));
        assert!(json.contains("\"username\":\"alice\""));
    }

    // --- ListUsersResponse serialization ---

    #[test]
    fn list_users_response_empty() {
        let resp = ListUsersResponse { users: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"users\":[]"));
    }

    #[test]
    fn list_users_response_multiple_users() {
        let resp = ListUsersResponse {
            users: vec![
                UserResponse {
                    id: "1".to_string(),
                    username: "alice".to_string(),
                    role: "admin".to_string(),
                    created_at: "2025-01-01T00:00:00Z".to_string(),
                    last_login: None,
                },
                UserResponse {
                    id: "2".to_string(),
                    username: "bob".to_string(),
                    role: "viewer".to_string(),
                    created_at: "2025-06-01T00:00:00Z".to_string(),
                    last_login: Some("2025-06-15T00:00:00Z".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("alice"));
        assert!(json.contains("bob"));
    }

    // --- UserRole serde in context of requests ---

    #[test]
    fn user_role_admin_lowercase_in_json() {
        let json = serde_json::to_string(&UserRole::Admin).unwrap();
        assert_eq!(json, "\"admin\"");
    }

    #[test]
    fn user_role_viewer_lowercase_in_json() {
        let json = serde_json::to_string(&UserRole::Viewer).unwrap();
        assert_eq!(json, "\"viewer\"");
    }

    #[test]
    fn user_role_unknown_value_fails() {
        let result = serde_json::from_str::<UserRole>("\"moderator\"");
        assert!(result.is_err());
    }
}
