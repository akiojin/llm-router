//! APIキーフロー統合テスト
//!
//! T018-T020: APIキー発行、認証成功/失敗

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

/// T018: APIキー発行フローのテスト
#[tokio::test]
async fn test_api_key_issuance_flow() {
    let app = build_app().await;

    // Step 1: JWT認証でログイン
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

    assert_eq!(login_response.status(), StatusCode::OK);

    let login_body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();

    let jwt_token = login_data["token"].as_str().unwrap();

    // Step 2: POST /api/api-keys にアクセスしてAPIキーを発行
    let create_key_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Test API Key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_key_response.status(), StatusCode::CREATED);

    // Step 3: APIキーと平文keyを受信
    let create_key_body = to_bytes(create_key_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_key_data: serde_json::Value = serde_json::from_slice(&create_key_body).unwrap();

    let api_key = create_key_data["key"].as_str().unwrap();
    assert!(!api_key.is_empty(), "API key should not be empty");
    assert!(api_key.starts_with("sk_"), "API key should start with sk_");

    // Step 4: GET /api/api-keys で発行したキーが一覧に表示される
    let list_keys_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(list_keys_response.status(), StatusCode::OK);

    let list_keys_body = to_bytes(list_keys_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let keys_list: serde_json::Value = serde_json::from_slice(&list_keys_body).unwrap();

    assert!(
        keys_list.get("api_keys").is_some(),
        "Response must have 'api_keys' field"
    );
    let keys_array = keys_list["api_keys"].as_array().unwrap();
    assert_eq!(keys_array.len(), 1, "Should have one API key");
    assert_eq!(keys_array[0]["name"], "Test API Key");
}

/// T019: APIキー認証成功フローのテスト
#[tokio::test]
async fn test_api_key_auth_success() {
    let app = build_app().await;

    // Step 1: APIキーを発行
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
    let jwt_token = login_data["token"].as_str().unwrap();

    let create_key_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Auth Test Key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_key_body = to_bytes(create_key_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_key_data: serde_json::Value = serde_json::from_slice(&create_key_body).unwrap();
    let api_key = create_key_data["key"].as_str().unwrap();

    // Step 2: APIキーでOpenAI互換エンドポイントにアクセス
    let use_key_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", api_key))
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

    // Step 3: 認証が成功し、リクエストが処理される（エージェントがないため503）
    assert!(
        use_key_response.status() == StatusCode::SERVICE_UNAVAILABLE
            || use_key_response.status() == StatusCode::OK,
        "API key should authenticate successfully"
    );
}

/// T020: 無効なAPIキーでの認証失敗テスト
#[tokio::test]
async fn test_api_key_auth_failure() {
    let app = build_app().await;

    // Step 1: 無効なAPIキーで /v1/chat/completions にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", "Bearer sk_invalid_key_12345")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model": "test-model",
                        "messages": [
                            {"role": "user", "content": "Test"}
                        ]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 2: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid API key should be rejected"
    );

    // Step 3: 削除されたAPIキーでアクセス
    // まず、APIキーを発行
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
    let jwt_token = login_data["token"].as_str().unwrap();

    let create_key_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "To Be Deleted Key"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let create_key_body = to_bytes(create_key_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_key_data: serde_json::Value = serde_json::from_slice(&create_key_body).unwrap();
    let api_key = create_key_data["key"].as_str().unwrap();
    let api_key_id = create_key_data["id"].as_str().unwrap();

    // APIキーを削除
    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/api-keys/{}", api_key_id))
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    // 削除されたAPIキーでアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model": "test-model",
                        "messages": [
                            {"role": "user", "content": "Test"}
                        ]
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
        "Deleted API key should be rejected"
    );
}
