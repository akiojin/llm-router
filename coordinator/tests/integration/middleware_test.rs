//! 認証ミドルウェア統合テスト
//!
//! T021-T022: 未認証での管理API拒否、JWT認証での許可

use crate::support;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_common::auth::UserRole;
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use tower::ServiceExt;

async fn build_app() -> Router {
    // 認証を有効化（他のテストの影響を受けないように明示的に設定）
    std::env::remove_var("AUTH_DISABLED");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = ollama_coordinator_coordinator::tasks::DownloadTaskManager::new();
    let db_pool = support::coordinator::create_test_db_pool().await;
    let jwt_secret = support::coordinator::test_jwt_secret();

    // テスト用の管理者ユーザーを作成
    let password_hash =
        ollama_coordinator_coordinator::auth::password::hash_password("password123").unwrap();
    ollama_coordinator_coordinator::db::users::create(
        &db_pool,
        "admin",
        &password_hash,
        UserRole::Admin,
    )
    .await
    .ok();

    // テスト用のViewerユーザーを作成
    let viewer_password_hash =
        ollama_coordinator_coordinator::auth::password::hash_password("viewer123").unwrap();
    ollama_coordinator_coordinator::db::users::create(
        &db_pool,
        "viewer",
        &viewer_password_hash,
        UserRole::Viewer,
    )
    .await
    .ok();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };

    api::create_router(state)
}

/// T021: 未認証での管理API拒否テスト
#[tokio::test]
async fn test_unauthorized_management_api_rejection() {
    let app = build_app().await;

    // Step 1: 認証トークンなしで GET /api/users にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 2: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET /api/users without token should be unauthorized"
    );

    // Step 3: 認証トークンなしで POST /api/users にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "newuser",
                        "password": "pass123",
                        "role": "viewer"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 4: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "POST /api/users without token should be unauthorized"
    );

    // Step 5: 認証トークンなしで DELETE /api/users/:id にアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/users/some-user-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 6: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "DELETE /api/users/:id without token should be unauthorized"
    );
}

/// T022: JWT認証での管理API許可テスト
#[tokio::test]
async fn test_jwt_auth_management_api_allowed() {
    let app = build_app().await;

    // 管理者としてログイン
    let login_response = app
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

    let login_body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let admin_token = login_data["token"].as_str().unwrap();

    // Step 1: 有効なJWTトークンで GET /api/users にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", format!("Bearer {}", admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 2: 200 OK を受信
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/users with admin token should succeed"
    );

    // Step 3: 有効なJWTトークンで POST /api/users にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .header("authorization", format!("Bearer {}", admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "testuser",
                        "password": "testpass123",
                        "role": "viewer"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 4: 201 Created を受信
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "POST /api/users with admin token should succeed"
    );

    // Step 5: Viewerロールで管理操作（POST）を試みる
    // まず、Viewerとしてログイン
    let viewer_login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "viewer",
                        "password": "viewer123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let viewer_login_body = to_bytes(viewer_login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let viewer_login_data: serde_json::Value = serde_json::from_slice(&viewer_login_body).unwrap();
    let viewer_token = viewer_login_data["token"].as_str().unwrap();

    // Viewerロールで POST /api/users を試みる
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .header("authorization", format!("Bearer {}", viewer_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "anotheruser",
                        "password": "pass123",
                        "role": "viewer"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 6: 403 Forbidden を受信
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "POST /api/users with viewer token should be forbidden"
    );
}
