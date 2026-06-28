//! 認証API Contract Tests
//!
//! POST /api/auth/login, POST /api/auth/logout, GET /api/auth/me,
//! PUT /api/auth/change-password

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::UserRole;
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn build_app() -> (Router, SqlitePool) {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
        .await
        .ok();

    (app, db_pool)
}

async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": username,
                        "password": password
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

async fn login_admin(app: &Router) -> String {
    let (status, body) = login(app, "admin", "password123").await;
    assert_eq!(status, StatusCode::OK);
    body["token"].as_str().unwrap().to_string()
}

fn bearer_request(jwt: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", jwt))
}

// ---------------------------------------------------------------------------
// POST /api/auth/login
// ---------------------------------------------------------------------------

/// DB登録ユーザーでのログイン成功
#[tokio::test]
#[serial]
async fn test_login_success_with_db_user() {
    let (app, _db_pool) = build_app().await;
    let (status, body) = login(&app, "admin", "password123").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].is_string());
    assert_eq!(body["expires_in"], 86400);
    assert_eq!(body["user"]["username"], "admin");
    assert_eq!(body["user"]["role"], "admin");
}

/// 開発モード固定ユーザー（admin/test）でログイン成功
#[tokio::test]
#[serial]
async fn test_login_success_dev_mode() {
    let (app, _db_pool) = build_app().await;
    let (status, body) = login(&app, "admin", "test").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].is_string());
    assert_eq!(body["user"]["username"], "admin");
}

/// 存在しないユーザーでログイン失敗
#[tokio::test]
#[serial]
async fn test_login_failure_unknown_user() {
    let (app, _db_pool) = build_app().await;
    let (status, _body) = login(&app, "unknown", "password123").await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// パスワード誤りでログイン失敗
#[tokio::test]
#[serial]
async fn test_login_failure_wrong_password() {
    let (app, _db_pool) = build_app().await;
    let (status, _body) = login(&app, "admin", "wrongpassword").await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// ユーザー名なしのリクエストは422
#[tokio::test]
#[serial]
async fn test_login_missing_username_returns_422() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "password": "pass" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// ログイン成功レスポンスにSet-Cookieヘッダーが含まれる
#[tokio::test]
#[serial]
async fn test_login_sets_cookie_headers() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let set_cookies: Vec<_> = response.headers().get_all("set-cookie").iter().collect();
    assert!(
        set_cookies.len() >= 2,
        "Should set jwt and csrf cookies, got {} cookies",
        set_cookies.len()
    );
}

/// must_change_passwordフラグがレスポンスに含まれる
#[tokio::test]
#[serial]
async fn test_login_must_change_password_flag() {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let password_hash = llmlb::auth::password::hash_password("changeme").unwrap();
    llmlb::db::users::create(&db_pool, "newuser", &password_hash, UserRole::Viewer, true)
        .await
        .ok();

    let (status, body) = login(&app, "newuser", "changeme").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["must_change_password"], true);
}

// ---------------------------------------------------------------------------
// POST /api/auth/logout
// ---------------------------------------------------------------------------

/// ログアウトは204を返す
#[tokio::test]
#[serial]
async fn test_logout_returns_no_content() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/auth/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// ログアウト時にCookieクリアヘッダーが含まれる
#[tokio::test]
#[serial]
async fn test_logout_clears_cookies() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/auth/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let set_cookies: Vec<_> = response.headers().get_all("set-cookie").iter().collect();
    assert!(
        !set_cookies.is_empty(),
        "Logout should set cookie-clearing headers"
    );
}

// ---------------------------------------------------------------------------
// GET /api/auth/me
// ---------------------------------------------------------------------------

/// 認証済みユーザー情報を取得
#[tokio::test]
#[serial]
async fn test_me_returns_user_info() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    assert!(data["user_id"].is_string());
    assert!(data["username"].is_string());
    assert!(data["role"].is_string());
}

/// 認証なしで /api/auth/me は 401
#[tokio::test]
#[serial]
async fn test_me_requires_auth() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 無効なJWTで /api/auth/me は 401
#[tokio::test]
#[serial]
async fn test_me_invalid_jwt_returns_401() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            bearer_request("invalid-jwt-token")
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// PUT /api/auth/change-password
// ---------------------------------------------------------------------------

/// パスワード変更成功
#[tokio::test]
#[serial]
async fn test_change_password_success() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    let response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("PUT")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "new_password": "Newpassword123" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// パスワードが短すぎると400
#[tokio::test]
#[serial]
async fn test_change_password_too_short() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    let response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("PUT")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "new_password": "abc" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// 認証なしでパスワード変更は401
#[tokio::test]
#[serial]
async fn test_change_password_requires_auth() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "new_password": "Newpassword123" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// パスワード変更後に新パスワードでログイン可能
#[tokio::test]
#[serial]
async fn test_change_password_then_login_with_new() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_admin(&app).await;

    // パスワード変更
    let response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("PUT")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "new_password": "Newpassword456" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 新パスワードでログイン
    let (status, _body) = login(&app, "admin", "Newpassword456").await;
    assert_eq!(status, StatusCode::OK);
}

/// must_change_passwordユーザーがパスワード変更後にフラグが解除される
#[tokio::test]
#[serial]
async fn test_change_password_clears_must_change_flag() {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let password_hash = llmlb::auth::password::hash_password("temppass1").unwrap();
    llmlb::db::users::create(&db_pool, "forced", &password_hash, UserRole::Admin, true)
        .await
        .ok();

    // must_change_password = true でログイン
    let (status, body) = login(&app, "forced", "temppass1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["must_change_password"], true);
    let jwt = body["token"].as_str().unwrap();

    // パスワード変更
    let response = app
        .clone()
        .oneshot(
            bearer_request(jwt)
                .method("PUT")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "new_password": "Newpass123" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 再ログインでフラグ解除を確認
    let (status, body) = login(&app, "forced", "Newpass123").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["must_change_password"], false);
}
