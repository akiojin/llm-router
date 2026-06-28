//! Contract Test: POST /api/endpoints/:id/download
//!
//! SPEC-e8e9326e: エンドポイントモデルダウンロードAPI契約テスト
//!
//! US8: xLLMエンドポイントでモデルダウンロードをリクエスト

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::{ApiKeyPermission, UserRole};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use tower::ServiceExt;

struct TestApp {
    app: Router,
    admin_key: String,
}

async fn build_app() -> TestApp {
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

    let db_pool = crate::support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = "test-secret".to_string();
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

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin_user = llmlb::db::users::create(
        &state.db_pool,
        "admin",
        &password_hash,
        UserRole::Admin,
        false,
    )
    .await
    .expect("create admin user");
    let admin_key = llmlb::db::api_keys::create(
        &state.db_pool,
        "admin-key",
        admin_user.id,
        None,
        ApiKeyPermission::all(),
    )
    .await
    .expect("create admin api key")
    .key;

    let app = api::create_app(state);
    TestApp { app, admin_key }
}

fn admin_request(admin_key: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", admin_key))
}

/// POST /api/endpoints/:id/download - 正常系: ダウンロードリクエスト
/// NOTE: この機能はxLLMタイプのエンドポイントでのみ利用可能
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_request() {
    let TestApp { app, admin_key } = build_app().await;

    // エンドポイントを登録
    let payload = json!({
        "name": "xLLM Endpoint",
        "base_url": "http://localhost:8080"
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let endpoint_id = body["id"].as_str().unwrap();

    // ダウンロードリクエスト
    let download_payload = json!({
        "model": "llama-3.2-1b"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/download", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // xLLMタイプでない場合は400を返す（タイプチェックが実装されるまでは404）
    // 実装後は202 Acceptedを期待
    assert!(
        response.status() == StatusCode::ACCEPTED
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::BAD_REQUEST
    );
}

/// POST /api/endpoints/:id/download - 異常系: 存在しないエンドポイント
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_endpoint_not_found() {
    let TestApp { app, admin_key } = build_app().await;

    let download_payload = json!({
        "model": "llama-3.2-1b"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000000/download")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// POST /api/endpoints/:id/download - 異常系: モデル名未指定
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_missing_model_name() {
    let TestApp { app, admin_key } = build_app().await;

    // エンドポイントを登録
    let payload = json!({
        "name": "xLLM Endpoint",
        "base_url": "http://localhost:8080"
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let endpoint_id = body["id"].as_str().unwrap();

    // モデル名なしでリクエスト
    let download_payload = json!({});

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/download", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // 400 Bad Request または 422 Unprocessable Entity を期待
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// POST /api/endpoints/:id/download - 異常系: 非xLLMエンドポイント
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_non_xllm_endpoint() {
    let TestApp { app, admin_key } = build_app().await;

    // エンドポイントを登録（タイプがunknownの場合はダウンロード不可）
    let payload = json!({
        "name": "Generic Endpoint",
        "base_url": "http://localhost:11434"
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let endpoint_id = body["id"].as_str().unwrap();

    // ダウンロードリクエスト
    let download_payload = json!({
        "model": "llama-3.2-1b"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/download", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // 非xLLMエンドポイントでは400 Bad Requestを期待
    // エラーメッセージ: "Model download is only supported for xLLM endpoints"
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// POST /api/endpoints/:id/download - 異常系: 認証なし
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let download_payload = json!({
        "model": "llama-3.2-1b"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000001/download")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/endpoints/:id/download - レスポンス構造検証
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T123で実装予定"]
async fn test_download_model_response_structure() {
    let TestApp { app, admin_key } = build_app().await;

    // エンドポイントを登録（xLLMタイプと仮定）
    let payload = json!({
        "name": "xLLM Endpoint",
        "base_url": "http://localhost:8080"
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let endpoint_id = body["id"].as_str().unwrap();

    // ダウンロードリクエスト
    let download_payload = json!({
        "model": "llama-3.2-1b"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/download", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&download_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    if response.status() == StatusCode::ACCEPTED {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        // 期待されるレスポンス構造
        assert!(body["task_id"].is_string(), "task_id should be present");
        assert!(body["model"].is_string(), "model should be present");
        assert!(body["status"].is_string(), "status should be present");
        assert_eq!(
            body["status"], "pending",
            "initial status should be pending"
        );
    }
}
