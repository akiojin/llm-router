//! APIキーフローE2Eテスト
//!
//! T092: 完全なAPIキーフロー（発行 → 使用 → 削除）
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::UserRole;
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    let db_pool = support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = support::lb::test_jwt_secret();

    // テスト用の管理者ユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
        .await
        .ok();

    let http_client = reqwest::Client::new();
    let inference_gate = llmlb::inference_gate::InferenceGate::default();
    let shutdown = llmlb::shutdown::ShutdownController::default();
    let update_manager = llmlb::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to create update manager");

    let state = AppState {
        load_manager,
        request_history,
        db_pool: db_pool.clone(),
        jwt_secret,
        http_client,
        event_bus: llmlb::events::create_shared_event_bus(),
        endpoint_registry,
        inference_gate,
        shutdown,
        update_manager,
        audit_log_writer: llmlb::audit::writer::AuditLogWriter::new(
            llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone()),
            llmlb::audit::writer::AuditLogWriterConfig::default(),
        ),
        audit_log_storage: std::sync::Arc::new(llmlb::db::audit_log::AuditLogStorage::new(
            db_pool.clone(),
        )),
        audit_archive_pool: None,
    };

    (api::create_app(state), db_pool)
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
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Test API Key",
                        "expires_at": null,
                        "permissions": ["openai.inference", "openai.models.read"]
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
    // Note: APIキーはOpenAI互換エンドポイント(/v1/*)で使用される
    // ここではOpenAI互換のchat/completionsエンドポイントをテストする
    let use_key_response = app
        .clone()
        .oneshot(support::lb::with_connect_info(
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
        ))
        .await
        .unwrap();

    // 認証は成功している（401/403以外が返れば認証OK）
    // - 404: モデルが登録されていない
    // - 503: エンドポイントが登録されていない
    // - 200: 成功
    assert!(
        use_key_response.status() == StatusCode::NOT_FOUND
            || use_key_response.status() == StatusCode::SERVICE_UNAVAILABLE
            || use_key_response.status() == StatusCode::OK,
        "API key should authenticate successfully (404 = model not found, 503 = no endpoints, OK = success), got: {}",
        use_key_response.status()
    );

    // Step 4: APIキーの一覧を取得
    let list_keys_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
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
                .uri(format!("/api/me/api-keys/{}", api_key_id))
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(delete_key_response.status(), StatusCode::NO_CONTENT);

    // Step 6: 削除後、APIキーは使用できない
    let invalid_key_response = app
        .oneshot(support::lb::with_connect_info(
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
        ))
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
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "Expiring API Key",
                        "expires_at": expires_at.to_rfc3339(),
                        "permissions": ["openai.inference", "openai.models.read"]
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
