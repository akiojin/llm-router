//! 認証API
//!
//! ログイン、ログアウト、認証情報確認、招待キー管理（T-0009, T-0011）

use crate::common::auth::{Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;

use super::error::AppError;
use serde::{Deserialize, Serialize};

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

/// 招待キー生成リクエスト（T-0009）
#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    /// 招待対象ユーザー名（メールアドレス形式）
    pub username: String,
    /// ロール（"admin" または "viewer"）
    pub role: String,
}

/// 招待キー生成レスポンス（T-0009）
#[derive(Debug, Serialize)]
pub struct CreateInvitationResponse {
    /// 招待キー（8文字英数字）
    pub invitation_key: String,
    /// QRコード（SVG形式）
    pub qr_code: String,
    /// 有効期限
    pub expires_at: String,
}

/// 招待受け入れリクエスト（T-0011）
#[derive(Debug, Deserialize)]
pub struct AcceptInvitationRequest {
    /// 招待キー
    pub invitation_key: String,
    /// 生体認証完了フラグ
    pub biometric_verified: bool,
}

/// 招待受け入れレスポンス（T-0011）
#[derive(Debug, Serialize)]
pub struct AcceptInvitationResponse {
    /// ユーザー名
    pub username: String,
    /// セットアップトークン（1時間有効）
    pub setup_token: String,
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
    let is_secure = is_request_secure(&headers);

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
        })?
        .ok_or_else(|| {
            AppError(LbError::Authentication(
                "Invalid username or password".to_string(),
            ))
            .into_response()
        })?;

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
    let is_secure = is_request_secure(&headers);
    let cookie = crate::auth::clear_jwt_cookie(is_secure);
    let csrf_cookie = crate::auth::clear_csrf_cookie(is_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.append(header::SET_COOKIE, cookie.parse().unwrap());
    response_headers.append(header::SET_COOKIE, csrf_cookie.parse().unwrap());
    (StatusCode::NO_CONTENT, response_headers)
}

fn is_request_secure(headers: &HeaderMap) -> bool {
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        if proto.eq_ignore_ascii_case("https") {
            return true;
        }
    }
    if let Some(forwarded) = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())
    {
        let lowered = forwarded.to_ascii_lowercase();
        if lowered.contains("proto=https") {
            return true;
        }
    }
    false
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

    // 招待コードを使用済みにする
    crate::db::invitations::mark_as_used(&app_state.db_pool, invitation.id, user.id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to mark invitation as used: {}", e);
            // ユーザーは既に作成されているため、エラーでもロールバックしない
            // ここでのエラーはログに記録するが、ユーザー作成は成功として扱う
        })
        .ok();

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
    Json(request): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, Response> {
    let user_id: uuid::Uuid = claims.sub.parse().map_err(|_| {
        AppError(LbError::Authentication("Invalid user ID".to_string())).into_response()
    })?;

    // パスワード長バリデーション（6文字以上）
    if request.new_password.len() < 6 {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Password must be at least 6 characters".to_string(),
        )))
        .into_response());
    }

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

    // パスワードを更新
    crate::db::users::update(
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

    Ok(StatusCode::OK)
}

/// POST /api/admin/invitations - 招待キー生成（T-0009）
///
/// admin ロールのユーザーが招待キーを生成
///
/// # Arguments
/// * `Extension(claims)` - JWT クレーム（admin ロール確認用）
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - リクエスト（username, role）
///
/// # Returns
/// * `200 OK` - 招待キー、QRコード、有効期限
/// * `401 Unauthorized` - 認証失敗
/// * `403 Forbidden` - admin ロール権限がない
pub async fn create_invitation(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, Response> {
    // admin ロール確認
    if claims.role != UserRole::Admin {
        return Err(AppError(LbError::Authorization(
            "Only admin can create invitations".to_string(),
        ))
        .into_response());
    }

    // ロール値検証
    if request.role != "admin" && request.role != "viewer" {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Role must be 'admin' or 'viewer'".to_string(),
        )))
        .into_response());
    }

    // 招待キー生成（DB に保存）
    let invitation =
        crate::db::users::create_invitation(&app_state.db_pool, &request.username, &request.role)
            .await
            .map_err(|e| {
                tracing::error!("Failed to create invitation: {}", e);
                AppError(e).into_response()
            })?;

    // QRコード生成
    let qr_code = generate_qr_code(&invitation.key).map_err(|e| {
        tracing::error!("Failed to generate QR code: {}", e);
        AppError(LbError::Common(CommonError::Config(format!(
            "Failed to generate QR code: {}",
            e
        ))))
        .into_response()
    })?;

    let response = CreateInvitationResponse {
        invitation_key: invitation.key,
        qr_code,
        expires_at: invitation.expires_at.to_rfc3339(),
    };

    tracing::info!(
        "Invitation created for user: {} by admin: {}",
        request.username,
        claims.sub
    );

    Ok((StatusCode::OK, Json(response)))
}

/// POST /api/auth/accept-invitation - 招待受け入れ（T-0011）
///
/// ユーザーが招待キーで招待を受け入れ、setup_token を取得
///
/// # Arguments
/// * `State(app_state)` - アプリケーション状態
/// * `Json(request)` - リクエスト（invitation_key, biometric_verified）
///
/// # Returns
/// * `200 OK` - username, setup_token
/// * `400 Bad Request` - 無効な招待キー、期限切れ、2回目使用
pub async fn accept_invitation(
    State(app_state): State<AppState>,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<impl IntoResponse, Response> {
    // 招待キー検索
    let invitation =
        crate::db::users::get_invitation_by_key(&app_state.db_pool, &request.invitation_key)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch invitation: {}", e);
                AppError(e).into_response()
            })?
            .ok_or_else(|| {
                AppError(LbError::Common(CommonError::Validation(
                    "Invalid invitation key".to_string(),
                )))
                .into_response()
            })?;

    // 既に使用されているか確認
    if invitation.is_used {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Invitation key already used".to_string(),
        )))
        .into_response());
    }

    // 有効期限確認
    if Utc::now() > invitation.expires_at {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Invitation key expired".to_string(),
        )))
        .into_response());
    }

    // setup_token 生成（1時間有効期限）
    // 一時的に user_id は新規ユーザー生成用のプレースホルダーUUID を使用
    let temp_user_id = uuid::Uuid::new_v4().to_string();
    let setup_token = crate::auth::jwt::create_jwt_with_expiration(
        &temp_user_id,
        UserRole::Admin, // 一時的
        &app_state.jwt_secret,
        false,
        3600, // 1時間
    )
    .map_err(|e| {
        tracing::error!("Failed to generate setup token: {}", e);
        AppError(e).into_response()
    })?;

    // 招待キーを使用済みにマーク
    crate::db::users::mark_invitation_used(&app_state.db_pool, invitation.id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to mark invitation as used: {}", e);
            AppError(e).into_response()
        })?;

    let response = AcceptInvitationResponse {
        username: invitation.username,
        setup_token,
    };

    tracing::info!("Invitation accepted: key={}", request.invitation_key);

    Ok((StatusCode::OK, Json(response)))
}

/// QRコード生成（招待キーを含む）
fn generate_qr_code(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    // qrcode クレートを使用してQRコードを生成
    use qrcode::QrCode;

    let _qr = QrCode::new(key)?;

    // NOTE: 簡易版 - テキストベースのプレースホルダーSVGを返す
    // 本実装ではqrcode-svgなどでSVG形式に変換するのが望ましい
    Ok("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"200\"><rect fill=\"white\" width=\"200\" height=\"200\"/></svg>".to_string())
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
    // is_request_secure tests
    // =========================================================================

    #[test]
    fn is_request_secure_returns_false_for_empty_headers() {
        let headers = HeaderMap::new();
        assert!(!is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_returns_true_for_x_forwarded_proto_https() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_returns_false_for_x_forwarded_proto_http() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "http".parse().unwrap());
        assert!(!is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_case_insensitive_https() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "HTTPS".parse().unwrap());
        assert!(is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_forwarded_header_proto_https() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", "proto=https".parse().unwrap());
        assert!(is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_forwarded_header_proto_http() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", "proto=http".parse().unwrap());
        assert!(!is_request_secure(&headers));
    }

    #[test]
    fn is_request_secure_forwarded_complex_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            "for=192.0.2.60;proto=https;by=203.0.113.43"
                .parse()
                .unwrap(),
        );
        assert!(is_request_secure(&headers));
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
