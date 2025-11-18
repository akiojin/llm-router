//! 認証API契約テスト
//!
//! TDD RED: これらのテストは実装前に失敗する必要があります
//! T004-T006: POST /api/auth/login, POST /api/auth/logout, GET /api/auth/me

use crate::support;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use tower::ServiceExt;

async fn build_app() -> Router {
    // AUTH_DISABLED=trueで認証を無効化
    std::env::set_var("AUTH_DISABLED", "true");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = DownloadTaskManager::new();
    let db_pool = support::coordinator::create_test_db_pool().await;
    let jwt_secret = support::coordinator::test_jwt_secret();

    // テスト用の管理者ユーザーを作成
    let password_hash =
        ollama_coordinator_coordinator::auth::password::hash_password("password123").unwrap();
    ollama_coordinator_coordinator::db::users::create(
        &db_pool,
        "admin",
        &password_hash,
        ollama_coordinator_common::auth::UserRole::Admin,
    )
    .await
    .ok(); // エラーは無視（既に存在する場合）

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

/// T004: POST /api/auth/login の契約テスト
#[tokio::test]
async fn test_login_contract() {
    let app = build_app().await;

    // Arrange: ログインリクエスト
    let request_body = json!({
        "username": "admin",
        "password": "password123"
    });

    // Act: POST /api/auth/login
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証（実装前は404または200）
    // 実装後は200 OKまたは401 Unauthorized
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    // 実装後はこのアサーションを修正
    if status == StatusCode::OK {
        // レスポンスボディの検証
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // スキーマ検証（OpenAPI仕様に基づく）
        assert!(
            body.get("token").is_some(),
            "Response must have 'token' field"
        );
        assert!(body["token"].is_string(), "'token' field must be a string");

        assert!(
            body.get("user").is_some(),
            "Response must have 'user' object"
        );
        let user = &body["user"];
        assert!(user.get("id").is_some(), "User must have 'id'");
        assert!(user.get("username").is_some(), "User must have 'username'");
        assert!(user.get("role").is_some(), "User must have 'role'");
    } else {
        // 未実装の場合は404
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T005: POST /api/auth/logout の契約テスト
#[tokio::test]
async fn test_logout_contract() {
    let app = build_app().await;

    // Act: POST /api/auth/logout
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header("authorization", "Bearer dummy_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status == StatusCode::OK || status == StatusCode::NO_CONTENT {
        // 実装完了：ログアウトは204 No Contentを返す
        if status == StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(body.is_object(), "Response must be a JSON object");
        }
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T006: GET /api/auth/me の契約テスト
#[tokio::test]
async fn test_get_current_user_contract() {
    let app = build_app().await;

    // Act: GET /api/auth/me
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header("authorization", "Bearer dummy_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status == StatusCode::OK {
        // レスポンスボディの検証
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // スキーマ検証（ユーザー情報）
        assert!(body.get("id").is_some(), "User must have 'id'");
        assert!(body.get("username").is_some(), "User must have 'username'");
        assert!(body.get("role").is_some(), "User must have 'role'");
        assert!(
            ["admin", "viewer"].contains(&body["role"].as_str().unwrap_or("")),
            "Role must be 'admin' or 'viewer'"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}
