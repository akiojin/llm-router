//! APIキーフローE2Eテスト
//!
//! T092: 完全なAPIキーフロー（発行 → 使用 → 削除）

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_common::auth::UserRole;
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use tower::ServiceExt;

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
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
        UserRole::Admin,
    )
    .await
    .ok();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool: db_pool.clone(),
        jwt_secret,
    };

    (api::create_router(state), db_pool)
}

#[tokio::test]
async fn test_complete_api_key_flow() {
    let (app, _db_pool) = build_app().await;

    // Step 1: ログイン
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

    let login_body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();

    let jwt_token = login_data["token"].as_str().unwrap();

    // Step 2: APIキーを発行
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
                        "name": "Test API Key",
                        "expires_at": null
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_key_response.status(), StatusCode::CREATED);

    let create_key_body = axum::body::to_bytes(create_key_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_key_data: serde_json::Value = serde_json::from_slice(&create_key_body).unwrap();

    let api_key = create_key_data["key"].as_str().unwrap();
    let api_key_id = create_key_data["id"].as_str().unwrap();

    assert!(!api_key.is_empty(), "API key should not be empty");

    // Step 3: APIキーを使ってエンドポイントにアクセス
    // Note: APIキーはOpenAI互換エンドポイント(/v1/*)とOllama APIで使用される
    // ここではOpenAI互換のchat/completionsエンドポイントをテスト
    let use_key_response = app
        .clone()
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

    // エージェントが登録されていないため503エラーが返されるが、認証は成功している
    assert!(
        use_key_response.status() == StatusCode::SERVICE_UNAVAILABLE
            || use_key_response.status() == StatusCode::OK,
        "API key should authenticate successfully (503 = no agents, OK = success)"
    );

    // Step 4: APIキーの一覧を取得
    let list_keys_response = app
        .clone()
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

    let list_keys_body = axum::body::to_bytes(list_keys_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let keys_list: serde_json::Value = serde_json::from_slice(&list_keys_body).unwrap();

    assert!(
        keys_list.get("api_keys").is_some(),
        "Response must have 'api_keys' field"
    );
    assert!(
        keys_list["api_keys"].is_array(),
        "'api_keys' must be an array"
    );
    assert_eq!(
        keys_list["api_keys"].as_array().unwrap().len(),
        1,
        "Should have one API key"
    );

    // Step 5: APIキーを削除
    let delete_key_response = app
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

    assert_eq!(delete_key_response.status(), StatusCode::NO_CONTENT);

    // Step 6: 削除後、APIキーは使用できない
    let invalid_key_response = app
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

    assert_eq!(
        invalid_key_response.status(),
        StatusCode::UNAUTHORIZED,
        "Deleted API key should not work"
    );
}

#[tokio::test]
async fn test_api_key_with_expiration() {
    let (app, _db_pool) = build_app().await;

    // ログイン
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

    let login_body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let jwt_token = login_data["token"].as_str().unwrap();

    // 有効期限付きAPIキーを発行（1時間後に期限切れ）
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);

    let create_key_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Expiring API Key",
                        "expires_at": expires_at.to_rfc3339()
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_key_response.status(), StatusCode::CREATED);

    let create_key_body = axum::body::to_bytes(create_key_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_key_data: serde_json::Value = serde_json::from_slice(&create_key_body).unwrap();

    let api_key = create_key_data["key"].as_str().unwrap();
    assert!(!api_key.is_empty());
}
