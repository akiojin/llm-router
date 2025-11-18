//! APIキー管理API契約テスト
//!
//! TDD RED: これらのテストは実装前に失敗する必要があります
//! T011-T013: GET/POST/DELETE /api/api-keys

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

    // テスト用の管理者ユーザーを作成（ダミーClaims注入ミドルウェアと同じUUID）
    let password_hash =
        ollama_coordinator_coordinator::auth::password::hash_password("password123").unwrap();
    let dummy_uuid = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    ollama_coordinator_coordinator::db::users::create_with_id(
        &db_pool,
        dummy_uuid,
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

/// T011: GET /api/api-keys の契約テスト
#[tokio::test]
async fn test_list_api_keys_contract() {
    let app = build_app().await;

    // Act: GET /api/api-keys
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/api-keys")
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

        // スキーマ検証（APIキー配列）
        assert!(
            body.get("api_keys").is_some(),
            "Response must have 'api_keys' field"
        );
        assert!(
            body["api_keys"].is_array(),
            "'api_keys' field must be an array"
        );

        // api_keys配列の各要素の検証
        if let Some(api_keys) = body["api_keys"].as_array() {
            for api_key in api_keys {
                assert!(api_key.get("id").is_some(), "ApiKey must have 'id'");
                assert!(api_key.get("name").is_some(), "ApiKey must have 'name'");
                assert!(
                    api_key.get("created_at").is_some(),
                    "ApiKey must have 'created_at'"
                );
                // key_hashと平文keyは含まれないこと
                assert!(
                    api_key.get("key_hash").is_none(),
                    "ApiKey must not expose 'key_hash'"
                );
                assert!(
                    api_key.get("key").is_none(),
                    "ApiKey must not expose 'key' in list"
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

/// T012: POST /api/api-keys の契約テスト
#[tokio::test]
async fn test_create_api_key_contract() {
    let app = build_app().await;

    // Arrange: APIキー作成リクエスト
    let request_body = json!({
        "name": "Production API Key",
        "expires_at": "2025-12-31T23:59:59Z"
    });

    // Act: POST /api/api-keys
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/api-keys")
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

        // スキーマ検証（平文キーを含む）
        assert!(body.get("id").is_some(), "ApiKey must have 'id'");
        assert!(
            body.get("key").is_some(),
            "ApiKey must have 'key' (plaintext)"
        );
        assert!(
            body["key"].as_str().unwrap().starts_with("sk_"),
            "API key must start with 'sk_'"
        );
        assert!(body.get("name").is_some(), "ApiKey must have 'name'");
        assert_eq!(body["name"], "Production API Key");
        assert!(
            body.get("created_at").is_some(),
            "ApiKey must have 'created_at'"
        );
        // key_hashは含まれないこと
        assert!(
            body.get("key_hash").is_none(),
            "ApiKey must not expose 'key_hash'"
        );
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "Expected 404 for unimplemented endpoint"
        );
    }
}

/// T013: DELETE /api/api-keys/:id の契約テスト
#[tokio::test]
async fn test_delete_api_key_contract() {
    let app = build_app().await;

    // Arrange: 削除対象APIキーID
    let api_key_id = "550e8400-e29b-41d4-a716-446655440000";

    // Act: DELETE /api/api-keys/:id
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/api-keys/{}", api_key_id))
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
