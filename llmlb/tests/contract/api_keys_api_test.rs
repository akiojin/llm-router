//! APIキー管理API Contract Tests
//!
//! GET /api/me/api-keys, POST /api/me/api-keys,
//! PUT /api/me/api-keys/{id}, DELETE /api/me/api-keys/{id}

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

async fn build_app_with_viewer() -> (Router, SqlitePool) {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let admin_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(&db_pool, "admin", &admin_hash, UserRole::Admin, false)
        .await
        .ok();

    let viewer_hash = llmlb::auth::password::hash_password("viewerpass").unwrap();
    llmlb::db::users::create(&db_pool, "viewer1", &viewer_hash, UserRole::Viewer, false)
        .await
        .ok();

    (app, db_pool)
}

async fn login_as(app: &Router, username: &str, password: &str) -> String {
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

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    data["token"].as_str().unwrap().to_string()
}

fn bearer_request(jwt: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", jwt))
}

// ---------------------------------------------------------------------------
// US-013: 認証必須化・匿名アクセス廃止（500バグ修正）
// ---------------------------------------------------------------------------

/// 既定構成で未認証のAPIキー作成は 401 になること。
///
/// 旧 bypass モードでは匿名admin(nil UUID)が注入され、`created_by`=nil の INSERT が
/// FK 違反で 500 になっていた（本不具合の原因）。認証必須化後は 401 を返す。
#[tokio::test]
#[serial]
async fn test_unauthenticated_create_api_key_returns_401() {
    let (app, _db_pool) = crate::support::lb::create_test_lb_default_auth().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "unauth-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 既定構成で未認証のAPIキー一覧取得も 401 になること（匿名adminを注入しない）。
#[tokio::test]
#[serial]
async fn test_unauthenticated_list_api_keys_returns_401() {
    let (app, _db_pool) = crate::support::lb::create_test_lb_default_auth().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// POST /api/me/api-keys
// ---------------------------------------------------------------------------

/// Adminがpermissions指定でAPIキーを発行
#[tokio::test]
#[serial]
async fn test_create_api_key_admin_with_permissions() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-key",
                        "permissions": ["openai.inference", "openai.models.read"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    assert!(data["key"].is_string());
    assert!(data["id"].is_string());
    assert_eq!(data["name"], "test-key");
    assert!(data["permissions"].is_array());
    assert_eq!(data["permissions"].as_array().unwrap().len(), 2);
}

/// Adminがpermissionsなしで発行は400
#[tokio::test]
#[serial]
async fn test_create_api_key_admin_missing_permissions() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "no-perms-key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Adminが空permissions配列で発行は400
#[tokio::test]
#[serial]
async fn test_create_api_key_admin_empty_permissions() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "empty-perms-key",
                        "permissions": []
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Viewerがpermissions指定なしでAPIキー発行（固定permissions）
#[tokio::test]
#[serial]
async fn test_create_api_key_viewer_default_permissions() {
    let (app, _db_pool) = build_app_with_viewer().await;
    let jwt = login_as(&app, "viewer1", "viewerpass").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "viewer-key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    let perms = data["permissions"].as_array().unwrap();
    assert_eq!(perms.len(), 2);
    assert!(perms.iter().any(|p| p == "openai.inference"));
    assert!(perms.iter().any(|p| p == "openai.models.read"));
}

/// Viewerがpermissions指定すると400
#[tokio::test]
#[serial]
async fn test_create_api_key_viewer_with_permissions_rejected() {
    let (app, _db_pool) = build_app_with_viewer().await;
    let jwt = login_as(&app, "viewer1", "viewerpass").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "viewer-custom-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// 廃止されたscopes指定は400
#[tokio::test]
#[serial]
async fn test_create_api_key_deprecated_scopes_rejected() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "scopes-key",
                        "scopes": ["openai"],
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// 有効期限付きAPIキーの発行
#[tokio::test]
#[serial]
async fn test_create_api_key_with_expiry() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let expires = "2099-12-31T23:59:59Z";
    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "expiring-key",
                        "expires_at": expires,
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    assert!(data["expires_at"].is_string());
}

/// 認証なしでAPIキー発行は401
#[tokio::test]
#[serial]
async fn test_create_api_key_requires_auth() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "noauth-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 同一ユーザー内で同名APIキーの重複作成は400
#[tokio::test]
#[serial]
async fn test_create_api_key_duplicate_name_rejected() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let first = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "duplicate-name",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    let duplicate = app
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "duplicate-name",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
}

/// APIキー更新で他キーと同名へのリネームは400
#[tokio::test]
#[serial]
async fn test_update_api_key_duplicate_name_rejected() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let first = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "first-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "second-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CREATED);

    let second_body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
    let second_json: Value = serde_json::from_slice(&second_body).unwrap();
    let second_id = second_json["id"].as_str().unwrap();

    let update = app
        .oneshot(
            bearer_request(&jwt)
                .method("PUT")
                .uri(format!("/api/me/api-keys/{}", second_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "first-key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(update.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// GET /api/me/api-keys
// ---------------------------------------------------------------------------

/// APIキー一覧を取得
#[tokio::test]
#[serial]
async fn test_list_api_keys() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    // キーを作成
    let _ = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "list-test-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("GET")
                .uri("/api/me/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let data: Value = serde_json::from_slice(&body).unwrap();
    assert!(data["api_keys"].is_array());
    assert!(!data["api_keys"].as_array().unwrap().is_empty());
}

/// 認証なしでAPIキー一覧は401
#[tokio::test]
#[serial]
async fn test_list_api_keys_requires_auth() {
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// DELETE /api/me/api-keys/{id}
// ---------------------------------------------------------------------------

/// APIキーを削除
#[tokio::test]
#[serial]
async fn test_delete_api_key() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    // キーを作成
    let create_response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "delete-me-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: Value = serde_json::from_slice(&create_body).unwrap();
    let key_id = create_data["id"].as_str().unwrap();

    // 削除
    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("DELETE")
                .uri(format!("/api/me/api-keys/{}", key_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// 存在しないAPIキーの削除は404
#[tokio::test]
#[serial]
async fn test_delete_nonexistent_api_key() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    let fake_id = uuid::Uuid::new_v4();
    let response = app
        .oneshot(
            bearer_request(&jwt)
                .method("DELETE")
                .uri(format!("/api/me/api-keys/{}", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 認証なしでAPIキー削除は401
#[tokio::test]
#[serial]
async fn test_delete_api_key_requires_auth() {
    let (app, _db_pool) = build_app().await;

    let fake_id = uuid::Uuid::new_v4();
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/me/api-keys/{}", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// APIキーによるOpenAI API認証テスト
// ---------------------------------------------------------------------------

/// 発行したAPIキーでOpenAI互換APIにアクセス可能
#[tokio::test]
#[serial]
async fn test_api_key_used_for_openai_models_api() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    // APIキーを発行
    let create_response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "openai-test-key",
                        "permissions": ["openai.inference", "openai.models.read"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: Value = serde_json::from_slice(&create_body).unwrap();
    let api_key = create_data["key"].as_str().unwrap();

    // APIキーで /v1/models にアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("authorization", format!("Bearer {}", api_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// 権限不足のAPIキーではアクセス拒否
#[tokio::test]
#[serial]
async fn test_api_key_without_models_read_permission_rejected() {
    let (app, _db_pool) = build_app().await;
    let jwt = login_as(&app, "admin", "password123").await;

    // openai.models.read なしのAPIキー
    let create_response = app
        .clone()
        .oneshot(
            bearer_request(&jwt)
                .method("POST")
                .uri("/api/me/api-keys")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "inference-only-key",
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: Value = serde_json::from_slice(&create_body).unwrap();
    let api_key = create_data["key"].as_str().unwrap();

    // /v1/models は openai.models.read が必要
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("authorization", format!("Bearer {}", api_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
