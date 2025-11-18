//! 認証API
//!
//! ログイン、ログアウト、認証情報確認

use crate::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use ollama_coordinator_common::auth::Claims;
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
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, Response> {
    // ユーザーを検索
    let user = crate::db::users::find_by_username(&app_state.db_pool, &request.username)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, "Invalid username or password").into_response()
        })?;

    // パスワードを検証
    let is_valid = crate::auth::password::verify_password(&request.password, &user.password_hash)
        .map_err(|e| {
        tracing::error!("Failed to verify password: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    if !is_valid {
        return Err((StatusCode::UNAUTHORIZED, "Invalid username or password").into_response());
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
    let token =
        crate::auth::jwt::create_jwt(&user.id.to_string(), user.role, &app_state.jwt_secret)
            .map_err(|e| {
                tracing::error!("Failed to create JWT: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            })?;

    Ok(Json(LoginResponse {
        token,
        expires_in,
        user: UserInfo {
            id: user.id.to_string(),
            username: user.username,
            role: format!("{:?}", user.role).to_lowercase(),
        },
    }))
}

/// POST /api/auth/logout - ログアウト
///
/// JWTはステートレスなのでクライアント側でトークンを破棄するだけ
/// このエンドポイントは主にログ記録用
///
/// # Returns
/// * `204 No Content` - ログアウト成功
pub async fn logout() -> impl IntoResponse {
    // JWT認証はステートレスなので、サーバー側では何もしない
    // クライアント側でトークンを破棄する
    StatusCode::NO_CONTENT
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
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    })?;

    // ユーザー情報を取得
    let user = crate::db::users::find_by_id(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to find user: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "User not found").into_response())?;

    Ok(Json(MeResponse {
        user_id: user.id.to_string(),
        username: user.username,
        role: format!("{:?}", user.role).to_lowercase(),
    }))
}
