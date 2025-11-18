//! 認証フロー統合テスト
//!
//! T015-T017: ログイン成功/失敗、未認証アクセス拒否

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

/// T015: ログイン成功フローのテスト
#[tokio::test]
async fn test_login_success_flow() {
    let app = build_app().await;

    // Step 1: POST /api/auth/login で正しいユーザー名とパスワードを送信
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

    // Step 2: 200 OK とJWTトークンを受信
    assert_eq!(login_response.status(), StatusCode::OK);

    let login_body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();

    let token = login_data["token"].as_str().unwrap();
    assert!(!token.is_empty(), "Token should not be empty");

    // Step 3: 受信したトークンで GET /api/auth/me にアクセス
    let me_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 4: ユーザー情報が返される
    assert_eq!(me_response.status(), StatusCode::OK);

    let me_body = to_bytes(me_response.into_body(), usize::MAX).await.unwrap();
    let me_data: serde_json::Value = serde_json::from_slice(&me_body).unwrap();

    assert_eq!(me_data["username"], "admin");
    assert_eq!(me_data["role"], "admin");
}

/// T016: ログイン失敗フロー（間違ったパスワード）のテスト
#[tokio::test]
async fn test_login_failure_wrong_password() {
    let app = build_app().await;

    // Step 1: POST /api/auth/login で間違ったパスワードを送信
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "wrong_password"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 2: 401 Unauthorized を受信
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// T017: 未認証でのダッシュボードアクセス拒否テスト
#[tokio::test]
async fn test_unauthorized_dashboard_access() {
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
        "Request without token should be unauthorized"
    );

    // Step 3: 無効なトークンでアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", "Bearer invalid_token_12345")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 4: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Request with invalid token should be unauthorized"
    );
}
