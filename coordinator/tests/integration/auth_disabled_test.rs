//! 認証無効化モード統合テスト
//!
//! T023: 認証無効化モードでのアクセス許可

use crate::support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_common::auth::UserRole;
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use serial_test::serial;
use tower::ServiceExt;

async fn build_app() -> Router {
    // AUTH_DISABLED=trueで認証を無効化
    std::env::set_var("AUTH_DISABLED", "true");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = ollama_coordinator_coordinator::tasks::DownloadTaskManager::new();
    let db_pool = support::coordinator::create_test_db_pool().await;
    let jwt_secret = support::coordinator::test_jwt_secret();

    // テスト用の管理者ユーザーを作成（認証無効モードでは使用されないが、データベースの整合性のため）
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

/// T023: 認証無効化モードでのアクセス許可テスト
#[tokio::test]
#[serial]
async fn test_auth_disabled_mode_allows_access() {
    let app = build_app().await;

    // テスト終了後のクリーンアップ用
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // Step 1: AUTH_DISABLED=true環境変数を設定（build_appで設定済み）
    // Step 2: サーバーを起動（build_appで起動済み）

    // Step 3: 認証トークンなしで GET /api/users にアクセス
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

    // Step 4: 200 OK を受信
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/users without auth should succeed when AUTH_DISABLED=true"
    );

    // Step 5: 認証トークンなしで POST /api/users にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
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

    // Step 6: 201 Created を受信
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "POST /api/users without auth should succeed when AUTH_DISABLED=true"
    );

    // Step 7: すべてのエンドポイントが認証なしでアクセス可能
    // DELETE操作も認証なしで成功することを確認
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

    // 404 Not Found（ユーザーが存在しない）、400 Bad Request（無効なID）、または204 No Content（削除成功）を期待
    // 401 Unauthorizedでないことが重要（認証が無効化されていることの確認）
    assert_ne!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "DELETE /api/users/:id should not return 401 when AUTH_DISABLED=true"
    );
}

/// T023: 認証無効化モードでのOpenAI互換APIアクセステスト
#[tokio::test]
#[serial]
async fn test_auth_disabled_mode_openai_api() {
    let app = build_app().await;

    // テスト終了後のクリーンアップ用
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // Step 1: AUTH_DISABLED=true環境変数を設定（build_appで設定済み）

    // Step 2: 認証トークンなしで POST /v1/chat/completions にアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model": "test-model",
                        "messages": [
                            {"role": "user", "content": "Hello"}
                        ]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 3: リクエストが処理される（エージェントがないため503だが、401ではない）
    assert!(
        response.status() == StatusCode::SERVICE_UNAVAILABLE || response.status() == StatusCode::OK,
        "POST /v1/chat/completions should not return 401 when AUTH_DISABLED=true, got: {}",
        response.status()
    );
}
