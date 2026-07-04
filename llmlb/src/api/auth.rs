//! 認証API
//!
//! ログイン、ログアウト、認証情報確認、ユーザー登録、パスワード変更

use crate::common::auth::{Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};

use super::error::AppError;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// タイミング攻撃緩和用のダミー bcrypt ハッシュ。
///
/// 存在しないユーザー名でログイン試行された場合でも、実在ユーザーと同等の
/// bcrypt 検証コストを発生させることで、応答時間差から有効なユーザー名を
/// 列挙されるのを防ぐ。実際のパスワードと同じコスト係数で一度だけ生成する。
static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    crate::auth::password::hash_password("dummy_password_for_timing_mitigation")
        .expect("failed to generate dummy password hash for timing mitigation")
});

/// ログインリクエスト
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// ユーザー名
    pub username: String,
    /// パスワード
    pub password: String,
}

/// ログインレスポンス
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// JWTトークン
    pub token: String,
    /// トークン有効期限（秒）
    pub expires_in: usize,
    /// ユーザー情報
    pub user: UserInfo,
}

/// ユーザー情報（ログインレスポンス用）
#[derive(Debug, Serialize)]
pub struct UserInfo {
    /// ユーザーID
    pub id: String,
    /// ユーザー名
    pub username: String,
    /// ロール
    pub role: String,
    /// 初回パスワード変更が必要か
    pub must_change_password: bool,
}

/// 認証情報レスポンス
#[derive(Debug, Serialize)]
pub struct MeResponse {
    /// ユーザーID
    pub user_id: String,
    /// ユーザー名
    pub username: String,
    /// ロール
    pub role: String,
    /// 初回パスワード変更が必要か
    pub must_change_password: bool,
}

/// パスワード変更リクエスト
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    /// 新しいパスワード
    pub new_password: String,
}

/// POST /api/auth/login - ログイン
///
/// ユーザー名とパスワードで認証し、JWTトークンを発行
///
/// # Arguments
/// * `State(app_state)` - アプリケーション状態（db_pool, jwt_secret）
/// * `Json(request)` - ログインリクエスト（username, password）
///
/// # Returns
/// * `200 OK` - ログイン成功（JWT token）
/// * `401 Unauthorized` - 認証失敗
/// * `500 Internal Server Error` - サーバーエラー
pub async fn login(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, Response> {
    let is_secure = crate::auth::request_is_secure(&headers);

    // 開発モード: admin/test で固定ログイン可能
    #[cfg(debug_assertions)]
    if request.username == "admin" && request.password == "test" {
        let dev_user_id = uuid::Uuid::nil().to_string();
        let expires_in = 86400;
        let token = crate::auth::jwt::create_jwt(
            &dev_user_id,
            crate::common::auth::UserRole::Admin,
            &app_state.jwt_secret,
            false,
            0,
        )
        .map_err(|e| {
            tracing::error!("Failed to create JWT: {}", e);
            AppError(LbError::Jwt(format!("Failed to create JWT: {}", e))).into_response()
        })?;

        tracing::info!("Development mode login: admin/test");
        let cookie = crate::auth::build_jwt_cookie(&token, expires_in, is_secure);
        let csrf_cookie = crate::auth::build_csrf_cookie(
            &crate::auth::generate_random_token(32),
            expires_in,
            is_secure,
        );
        let mut headers = HeaderMap::new();
        headers.append(header::SET_COOKIE, cookie.parse().unwrap());
        headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());
        return Ok((
            StatusCode::OK,
            headers,
            Json(LoginResponse {
                token,
                expires_in,
                user: UserInfo {
                    id: dev_user_id,
                    username: "admin".to_string(),
                    role: "admin".to_string(),
                    must_change_password: false,
                },
            }),
        ));
    }

    // ユーザーを検索
    let user = crate::db::users::find_by_username(&app_state.db_pool, &request.username)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            AppError(LbError::Database(format!("Failed to find user: {}", e))).into_response()
        })?;

    // タイミング攻撃緩和: ユーザーが存在しない場合でもダミーハッシュに対して
    // bcrypt 検証を実行し、応答時間を実在ユーザーと均一化してから401を返す。
    // これにより応答時間差から有効なユーザー名を列挙されるのを防ぐ。結果は破棄する。
    let user = match user {
        Some(user) => user,
        None => {
            let _ = crate::auth::password::verify_password(
                &request.password,
                DUMMY_PASSWORD_HASH.as_str(),
            );
            return Err(AppError(LbError::Authentication(
                "Invalid username or password".to_string(),
            ))
            .into_response());
        }
    };

    // パスワードを検証
    let is_valid = crate::auth::password::verify_password(&request.password, &user.password_hash)
        .map_err(|e| {
        tracing::error!("Failed to verify password: {}", e);
        AppError(LbError::PasswordHash(format!(
            "Failed to verify password: {}",
            e
        )))
        .into_response()
    })?;

    if !is_valid {
        return Err(AppError(LbError::Authentication(
            "Invalid username or password".to_string(),
        ))
        .into_response());
    }

    // 最終ログイン時刻を更新
    crate::db::users::update_last_login(&app_state.db_pool, user.id)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to update last login: {}", e);
            // エラーだがログイン自体は成功させる
        })
        .ok();

    // JWTを生成（有効期限24時間）
    let expires_in = 86400; // 24時間（秒）
    let token = crate::auth::jwt::create_jwt(
        &user.id.to_string(),
        user.role,
        &app_state.jwt_secret,
        user.must_change_password,
        user.password_changed_at,
    )
    .map_err(|e| {
        tracing::error!("Failed to create JWT: {}", e);
        AppError(LbError::Jwt(format!("Failed to create JWT: {}", e))).into_response()
    })?;

    let cookie = crate::auth::build_jwt_cookie(&token, expires_in, is_secure);
    let csrf_cookie = crate::auth::build_csrf_cookie(
        &crate::auth::generate_random_token(32),
        expires_in,
        is_secure,
    );
    let mut headers = HeaderMap::new();
    headers.append(header::SET_COOKIE, cookie.parse().unwrap());
    headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());
    Ok((
        StatusCode::OK,
        headers,
        Json(LoginResponse {
            token,
            expires_in,
            user: UserInfo {
                id: user.id.to_string(),
                username: user.username.clone(),
                role: format!("{:?}", user.role).to_lowercase(),
                must_change_password: user.must_change_password,
            },
        }),
    ))
}

/// POST /api/auth/logout - ログアウト
///
/// JWTはステートレスなのでクライアント側でトークンを破棄するだけ
/// Cookieの削除ヘッダーも返す
///
/// # Returns
/// * `204 No Content` - ログアウト成功
pub async fn logout(headers: HeaderMap) -> impl IntoResponse {
    let is_secure = crate::auth::request_is_secure(&headers);
    let cookie = crate::auth::clear_jwt_cookie(is_secure);
    let csrf_cookie = crate::auth::clear_csrf_cookie(is_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.append(header::SET_COOKIE, cookie.parse().unwrap());
    response_headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());
    (StatusCode::NO_CONTENT, response_headers)
}

/// GET /api/auth/me - 認証情報確認
///
/// 現在の認証済みユーザー情報を返す
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
///
/// # Returns
/// * `200 OK` - ユーザー情報
/// * `401 Unauthorized` - 認証されていない
/// * `404 Not Found` - ユーザーが見つからない
/// * `500 Internal Server Error` - サーバーエラー
pub async fn me(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
) -> Result<Json<MeResponse>, Response> {
    // ユーザーIDをパース
    let user_id = claims.sub.parse::<uuid::Uuid>().map_err(|e| {
        tracing::error!("Failed to parse user ID: {}", e);
        AppError(LbError::Internal(format!("Failed to parse user ID: {}", e))).into_response()
    })?;

    // 開発モード: nil UUIDの場合は開発ユーザー情報を返す
    #[cfg(debug_assertions)]
    if user_id.is_nil() {
        return Ok(Json(MeResponse {
            user_id: user_id.to_string(),
            username: "admin".to_string(),
            role: "admin".to_string(),
            must_change_password: false,
        }));
    }

    // ユーザー情報を取得
    let user = crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            AppError(LbError::Database(format!("Failed to find user: {}", e))).into_response()
        })?
        .ok_or_else(|| AppError(LbError::NotFound("User not found".to_string())).into_response())?;

    Ok(Json(MeResponse {
        user_id: user.id.to_string(),
        username: user.username,
        role: format!("{:?}", user.role).to_lowercase(),
        must_change_password: user.must_change_password,
    }))
}

/// ユーザー登録リクエスト
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    /// 招待コード
    pub invitation_code: String,
    /// ユーザー名
    pub username: String,
    /// パスワード
    pub password: String,
}

/// ユーザー登録レスポンス
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    /// ユーザーID
    pub id: String,
    /// ユーザー名
    pub username: String,
    /// ロール
    pub role: String,
    /// 作成日時
    pub created_at: String,
}

/// POST /api/auth/register - 招待コードでユーザー登録
///
/// 有効な招待コードを使ってユーザー登録する（認証不要）
///
/// # Arguments
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - 登録リクエスト（招待コード、ユーザー名、パスワード）
///
/// # Returns
/// * `201 Created` - 登録成功
/// * `400 Bad Request` - 招待コードが無効/期限切れ/使用済み
/// * `409 Conflict` - ユーザー名が既に存在
/// * `500 Internal Server Error` - サーバーエラー
pub async fn register(
    State(app_state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), Response> {
    // パスワード要件を検証（8文字以上・大文字・数字）
    crate::auth::password::validate_password(&request.password).map_err(|e| {
        tracing::info!("Password validation failed during registration");
        AppError(e).into_response()
    })?;

    // 招待コードを検索
    let invitation =
        crate::db::invitations::find_by_code(&app_state.db_pool, &request.invitation_code)
            .await
            .map_err(|e| {
                tracing::error!("Failed to find invitation code: {}", e);
                AppError(LbError::Database(format!(
                    "Failed to find invitation code: {}",
                    e
                )))
                .into_response()
            })?
            .ok_or_else(|| {
                AppError(LbError::Common(CommonError::Validation(
                    "Invalid invitation code".to_string(),
                )))
                .into_response()
            })?;

    // 招待コードが有効かチェック
    crate::db::invitations::validate_invitation(&invitation).map_err(|e| {
        tracing::info!("Invalid invitation code: {}", e);
        AppError(LbError::Common(CommonError::Validation(e.to_string()))).into_response()
    })?;

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

    // パスワードをハッシュ化
    let password_hash = crate::auth::password::hash_password(&request.password).map_err(|e| {
        tracing::error!("Failed to hash password: {}", e);
        AppError(LbError::PasswordHash(format!(
            "Failed to hash password: {}",
            e
        )))
        .into_response()
    })?;

    // ユーザーを作成（招待登録は常にviewer）
    let user = crate::db::users::create(
        &app_state.db_pool,
        &request.username,
        &password_hash,
        UserRole::Viewer,
        false,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create user: {}", e);
        AppError(LbError::Database(format!("Failed to create user: {}", e))).into_response()
    })?;

    // 招待コードを使用済みにする（atomic conditional update: WHERE status = 'active'）。
    // 失敗（並行登録で既に消費済み＝rows_affected 0、または DB エラー）の場合は、
    // 作成済みユーザーを補償削除して登録を拒否する。これにより招待コードの再利用や
    // 「招待が消費されたのにユーザーだけ残る」孤立を防ぐ。
    if let Err(e) =
        crate::db::invitations::mark_as_used(&app_state.db_pool, invitation.id, user.id).await
    {
        tracing::warn!(
            "Failed to consume invitation {} for user {}: {}; rolling back user creation",
            invitation.id,
            user.id,
            e
        );
        if let Err(del) = crate::db::users::delete(&app_state.db_pool, user.id).await {
            tracing::error!(
                "Failed to roll back user {} after invitation consume failure: {}",
                user.id,
                del
            );
        }
        return Err(AppError(LbError::Conflict(
            "Invitation code is no longer valid".to_string(),
        ))
        .into_response());
    }

    tracing::info!(
        "User registered via invitation: {} (id={})",
        user.username,
        user.id
    );

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            id: user.id.to_string(),
            username: user.username,
            role: format!("{:?}", UserRole::Viewer).to_lowercase(),
            created_at: user.created_at.to_rfc3339(),
        }),
    ))
}

/// PUT /api/auth/change-password - パスワード変更
///
/// 認証済みユーザーが自身のパスワードを変更する。
/// `must_change_password`フラグもクリアされる。
///
/// # Arguments
/// * `Extension(claims)` - JWTクレーム（ミドルウェアで注入）
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - パスワード変更リクエスト
///
/// # Returns
/// * `200 OK` - パスワード変更成功
/// * `400 Bad Request` - バリデーションエラー
/// * `401 Unauthorized` - 未認証
/// * `500 Internal Server Error` - サーバーエラー
pub async fn change_password(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, Response> {
    let user_id: uuid::Uuid = claims.sub.parse().map_err(|_| {
        AppError(LbError::Authentication("Invalid user ID".to_string())).into_response()
    })?;

    // パスワード要件を検証（register と同一ポリシー: 8文字以上・大文字・数字）
    crate::auth::password::validate_password(&request.new_password)
        .map_err(|e| AppError(e).into_response())?;

    // 新パスワードをハッシュ化
    let password_hash =
        crate::auth::password::hash_password(&request.new_password).map_err(|e| {
            tracing::error!("Failed to hash password: {}", e);
            AppError(LbError::PasswordHash(format!(
                "Failed to hash password: {}",
                e
            )))
            .into_response()
        })?;

    // パスワードを更新（password_changed_at が bump され、他の既存セッションは無効化される）
    let updated_user = crate::db::users::update(
        &app_state.db_pool,
        user_id,
        None,
        Some(&password_hash),
        None,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to update password: {}", e);
        AppError(LbError::Database(format!(
            "Failed to update password: {}",
            e
        )))
        .into_response()
    })?;

    // must_change_passwordフラグをクリア
    crate::db::users::clear_must_change_password(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to clear must_change_password: {}", e);
            AppError(LbError::Database(format!(
                "Failed to clear must_change_password: {}",
                e
            )))
            .into_response()
        })?;

    tracing::info!("Password changed for user: {}", user_id);

    // 自分自身のセッションが無効化されないよう、新しい password_changed_at を持つ
    // 新JWTを発行してレスポンスで返す（旧トークンは以降 401 になる）。
    let expires_in = 86400;
    let token = crate::auth::jwt::create_jwt(
        &user_id.to_string(),
        claims.role,
        &app_state.jwt_secret,
        false,
        updated_user.password_changed_at,
    )
    .map_err(|e| {
        tracing::error!("Failed to create JWT: {}", e);
        AppError(LbError::Jwt(format!("Failed to create JWT: {}", e))).into_response()
    })?;

    let is_secure = crate::auth::request_is_secure(&headers);
    let cookie = crate::auth::build_jwt_cookie(&token, expires_in, is_secure);
    let csrf_cookie = crate::auth::build_csrf_cookie(
        &crate::auth::generate_random_token(32),
        expires_in,
        is_secure,
    );
    let mut response_headers = HeaderMap::new();
    response_headers.append(header::SET_COOKIE, cookie.parse().unwrap());
    response_headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());

    Ok((
        StatusCode::OK,
        response_headers,
        Json(serde_json::json!({ "token": token, "expires_in": expires_in })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_logout_returns_no_content() {
        let response = logout(HeaderMap::new()).await.into_response();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[test]
    fn test_login_request_deserialize() {
        let json = r#"{"username": "admin", "password": "secret"}"#;
        let request: LoginRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.username, "admin");
        assert_eq!(request.password, "secret");
    }

    #[test]
    fn test_login_response_serialize() {
        let response = LoginResponse {
            token: "jwt_token".to_string(),
            expires_in: 86400,
            user: UserInfo {
                id: "user-id".to_string(),
                username: "admin".to_string(),
                role: "admin".to_string(),
                must_change_password: false,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("jwt_token"));
        assert!(json.contains("86400"));
        assert!(json.contains("admin"));
    }

    #[test]
    fn test_me_response_serialize() {
        let response = MeResponse {
            user_id: "user-123".to_string(),
            username: "testuser".to_string(),
            role: "user".to_string(),
            must_change_password: false,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("user-123"));
        assert!(json.contains("testuser"));
        assert!(json.contains("user"));
    }

    #[test]
    fn test_user_info_serialize() {
        let info = UserInfo {
            id: "id-456".to_string(),
            username: "bob".to_string(),
            role: "admin".to_string(),
            must_change_password: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("id-456"));
        assert!(json.contains("bob"));
        assert!(json.contains("admin"));
    }

    // =========================================================================
    // logout tests (additional)
    // =========================================================================

    #[tokio::test]
    async fn test_logout_clears_cookies() {
        let response = logout(HeaderMap::new()).await.into_response();
        let set_cookies: Vec<&str> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert!(
            set_cookies.len() >= 2,
            "logout should set at least 2 Set-Cookie headers"
        );
        let has_jwt_clear = set_cookies
            .iter()
            .any(|c| c.contains(crate::auth::DASHBOARD_JWT_COOKIE) && c.contains("Max-Age=0"));
        let has_csrf_clear = set_cookies
            .iter()
            .any(|c| c.contains(crate::auth::DASHBOARD_CSRF_COOKIE) && c.contains("Max-Age=0"));
        assert!(has_jwt_clear, "should clear JWT cookie");
        assert!(has_csrf_clear, "should clear CSRF cookie");
    }

    #[tokio::test]
    async fn test_logout_sets_secure_flag_behind_https() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        let response = logout(headers).await.into_response();
        let set_cookies: Vec<&str> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert!(
            set_cookies.iter().all(|c| c.contains("Secure")),
            "all cookies should have Secure flag behind HTTPS"
        );
    }

    #[tokio::test]
    async fn test_logout_no_secure_flag_for_http() {
        let response = logout(HeaderMap::new()).await.into_response();
        let set_cookies: Vec<&str> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        assert!(
            set_cookies.iter().all(|c| !c.contains("Secure")),
            "cookies should NOT have Secure flag for HTTP"
        );
    }

    // =========================================================================
    // LoginRequest deserialization tests
    // =========================================================================

    #[test]
    fn test_login_request_missing_field_fails() {
        let json = r#"{"username": "admin"}"#;
        let result = serde_json::from_str::<LoginRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_login_request_empty_strings() {
        let json = r#"{"username": "", "password": ""}"#;
        let request: LoginRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.username, "");
        assert_eq!(request.password, "");
    }

    // =========================================================================
    // ChangePasswordRequest deserialization tests
    // =========================================================================

    #[test]
    fn test_change_password_request_deserialize() {
        let json = r#"{"new_password": "newpass123"}"#;
        let request: ChangePasswordRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.new_password, "newpass123");
    }

    #[test]
    fn test_change_password_request_missing_field_fails() {
        let json = r#"{}"#;
        let result = serde_json::from_str::<ChangePasswordRequest>(json);
        assert!(result.is_err());
    }

    // =========================================================================
    // RegisterRequest deserialization tests
    // =========================================================================

    #[test]
    fn test_register_request_deserialize() {
        let json = r#"{"invitation_code": "ABC123", "username": "newuser", "password": "secret"}"#;
        let request: RegisterRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.invitation_code, "ABC123");
        assert_eq!(request.username, "newuser");
        assert_eq!(request.password, "secret");
    }

    #[test]
    fn test_register_request_missing_field_fails() {
        let json = r#"{"username": "newuser", "password": "secret"}"#;
        let result = serde_json::from_str::<RegisterRequest>(json);
        assert!(result.is_err());
    }

    // =========================================================================
    // RegisterResponse serialization tests
    // =========================================================================

    #[test]
    fn test_register_response_serialize() {
        let response = RegisterResponse {
            id: "user-id-1".to_string(),
            username: "testuser".to_string(),
            role: "viewer".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("user-id-1"));
        assert!(json.contains("testuser"));
        assert!(json.contains("viewer"));
        assert!(json.contains("2024-01-01T00:00:00Z"));
    }

    // =========================================================================
    // LoginResponse serialization tests (additional)
    // =========================================================================

    #[test]
    fn test_login_response_contains_all_fields() {
        let response = LoginResponse {
            token: "eyJhbGciOiJIUzI1NiJ9.payload.sig".to_string(),
            expires_in: 3600,
            user: UserInfo {
                id: "uuid-1".to_string(),
                username: "alice".to_string(),
                role: "viewer".to_string(),
                must_change_password: true,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("token").is_some());
        assert!(parsed.get("expires_in").is_some());
        assert!(parsed.get("user").is_some());
        let user = parsed.get("user").unwrap();
        assert_eq!(user["must_change_password"], true);
    }

    // =========================================================================
    // MeResponse serialization tests (additional)
    // =========================================================================

    #[test]
    fn test_me_response_must_change_password_true() {
        let response = MeResponse {
            user_id: "uid".to_string(),
            username: "bob".to_string(),
            role: "admin".to_string(),
            must_change_password: true,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["must_change_password"], true);
    }

    #[test]
    fn test_me_response_all_fields_present() {
        let response = MeResponse {
            user_id: "uid-abc".to_string(),
            username: "charlie".to_string(),
            role: "viewer".to_string(),
            must_change_password: false,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("user_id").is_some());
        assert!(parsed.get("username").is_some());
        assert!(parsed.get("role").is_some());
        assert!(parsed.get("must_change_password").is_some());
    }

    // =========================================================================
    // UserInfo serialization tests (additional)
    // =========================================================================

    #[test]
    fn test_user_info_must_change_password_true() {
        let info = UserInfo {
            id: "id-1".to_string(),
            username: "alice".to_string(),
            role: "admin".to_string(),
            must_change_password: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["must_change_password"], true);
    }

    #[test]
    fn test_user_info_all_fields_present() {
        let info = UserInfo {
            id: "id-2".to_string(),
            username: "user".to_string(),
            role: "viewer".to_string(),
            must_change_password: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("id").is_some());
        assert!(parsed.get("username").is_some());
        assert!(parsed.get("role").is_some());
        assert!(parsed.get("must_change_password").is_some());
    }
}
