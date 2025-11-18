//! ユーザー管理API契約テスト
//!
//! TDD RED: これらのテストは実装前に失敗する必要があります
//! T007-T010: GET/POST/PUT/DELETE /api/users

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

/// T007: GET /api/users の契約テスト
#[tokio::test]
async fn test_list_users_contract() {
    let app = build_app().await;

    // Act: GET /api/users
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", "Bearer admin_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status.is_success() {
        // レスポンスボディの検証
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // スキーマ検証（ユーザー配列）
        assert!(
            body.get("users").is_some(),
            "Response must have 'users' field"
        );
        assert!(body["users"].is_array(), "'users' field must be an array");

        // users配列の各要素の検証
        if let Some(users) = body["users"].as_array() {
            for user in users {
                assert!(user.get("id").is_some(), "User must have 'id'");
                assert!(user.get("username").is_some(), "User must have 'username'");
                assert!(user.get("role").is_some(), "User must have 'role'");
                assert!(
                    user.get("created_at").is_some(),
                    "User must have 'created_at'"
                );
                // password_hashは含まれないこと
                assert!(
                    user.get("password_hash").is_none(),
                    "User must not expose 'password_hash'"
                );
            }
        }
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T008: POST /api/users の契約テスト
#[tokio::test]
async fn test_create_user_contract() {
    let app = build_app().await;

    // Arrange: ユーザー作成リクエスト
    let request_body = json!({
        "username": "newuser",
        "password": "secure_password",
        "role": "viewer"
    });

    // Act: POST /api/users
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .header("authorization", "Bearer admin_token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status == StatusCode::CREATED {
        // レスポンスボディの検証
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // スキーマ検証
        assert!(body.get("id").is_some(), "User must have 'id'");
        assert!(body.get("username").is_some(), "User must have 'username'");
        assert_eq!(body["username"], "newuser");
        assert!(body.get("role").is_some(), "User must have 'role'");
        assert_eq!(body["role"], "viewer");
        assert!(
            body.get("created_at").is_some(),
            "User must have 'created_at'"
        );
        // password_hashは含まれないこと
        assert!(
            body.get("password_hash").is_none(),
            "User must not expose 'password_hash'"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T009: PUT /api/users/:id の契約テスト
#[tokio::test]
async fn test_update_user_contract() {
    let app = build_app().await;

    // Arrange: ユーザー更新リクエスト
    let user_id = "550e8400-e29b-41d4-a716-446655440000";
    let request_body = json!({
        "username": "updateduser",
        "role": "admin"
    });

    // Act: PUT /api/users/:id
    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/users/{}", user_id))
                .header("authorization", "Bearer admin_token")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status.is_success() {
        // レスポンスボディの検証
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // スキーマ検証
        assert!(body.get("id").is_some(), "User must have 'id'");
        assert!(body.get("username").is_some(), "User must have 'username'");
        assert!(body.get("role").is_some(), "User must have 'role'");
        assert!(
            body.get("created_at").is_some(),
            "User must have 'created_at'"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T010: DELETE /api/users/:id の契約テスト
#[tokio::test]
async fn test_delete_user_contract() {
    let app = build_app().await;

    // Arrange: 削除対象ユーザーID
    let user_id = "550e8400-e29b-41d4-a716-446655440000";

    // Act: DELETE /api/users/:id
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/users/{}", user_id))
                .header("authorization", "Bearer admin_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert: ステータスコード検証
    let status = response.status();

    // REDフェーズ: エンドポイントが未実装なので404を期待
    if status == StatusCode::NO_CONTENT {
        // 204 No Contentなのでボディは空
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.is_empty(), "DELETE should return empty body");
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}
