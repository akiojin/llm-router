//! Contract Test: POST /api/endpoints
//!
//! SPEC-e8e9326e: エンドポイント登録API契約テスト
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

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
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// POST /api/endpoints - 正常系: エンドポイント登録成功
#[tokio::test]
#[serial]
async fn test_create_endpoint_success() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key } = build_app().await;

    let payload = json!({
        "name": "Test Ollama",
        "base_url": mock.uri()
    });

    let response = app
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

    // 契約に基づくレスポンス検証
    assert!(body["id"].is_string(), "id should be a UUID string");
    assert_eq!(body["name"], "Test Ollama");
    assert_eq!(body["base_url"], mock.uri());
    assert_eq!(body["status"], "pending");
    assert_eq!(body["health_check_interval_secs"], 30);
    assert!(body["last_seen"].is_null());
    assert!(body["last_error"].is_null());
    assert_eq!(body["error_count"], 0);
    assert!(body["registered_at"].is_string());
}

/// POST /api/endpoints - 正常系: オプションフィールド付き登録
#[tokio::test]
#[serial]
async fn test_create_endpoint_with_optional_fields() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key } = build_app().await;

    let payload = json!({
        "name": "Production vLLM",
        "base_url": mock.uri(),
        "api_key": "sk-secret-key",
        "health_check_interval_secs": 60,
        "notes": "Production server"
    });

    let response = app
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

    assert_eq!(body["name"], "Production vLLM");
    assert_eq!(body["health_check_interval_secs"], 60);
    assert_eq!(body["notes"], "Production server");
    // api_keyはシリアライズされない（セキュリティ）
    assert!(body.get("api_key").is_none() || body["api_key"].is_null());
}

/// POST /api/endpoints - 異常系: 名前が空
#[tokio::test]
#[serial]
async fn test_create_endpoint_empty_name() {
    let TestApp { app, admin_key } = build_app().await;

    let payload = json!({
        "name": "",
        "base_url": "http://localhost:11434"
    });

    let response = app
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// POST /api/endpoints - 異常系: 不正なURL
#[tokio::test]
#[serial]
async fn test_create_endpoint_invalid_url() {
    let TestApp { app, admin_key } = build_app().await;

    let payload = json!({
        "name": "Invalid",
        "base_url": "not-a-valid-url"
    });

    let response = app
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// POST /api/endpoints - 異常系: URL重複
#[tokio::test]
#[serial]
#[ignore = "TDD RED: URL重複チェック未実装"]
async fn test_create_endpoint_duplicate_url() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key } = build_app().await;

    let payload = json!({
        "name": "First",
        "base_url": mock.uri()
    });

    // 最初の登録
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

    // 重複登録
    let dup_payload = json!({
        "name": "Second",
        "base_url": mock.uri()
    });

    let dup_response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&dup_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(dup_response.status(), StatusCode::CONFLICT);

    let body = to_bytes(dup_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let code = body
        .get("code")
        .or_else(|| body.get("error").and_then(|e| e.get("code")));
    assert!(
        code.is_some() && code.unwrap().is_string(),
        "error code should be present either at body.code or body.error.code"
    );
}

/// POST /api/endpoints - 異常系: 認証なし
#[tokio::test]
#[serial]
async fn test_create_endpoint_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let payload = json!({
        "name": "Test",
        "base_url": "http://localhost:11434"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/endpoints - 異常系: ヘルスチェック間隔の範囲外
#[tokio::test]
#[serial]
async fn test_create_endpoint_invalid_health_check_interval() {
    let TestApp { app, admin_key } = build_app().await;

    // 範囲下限（10未満）
    let payload = json!({
        "name": "Test",
        "base_url": "http://localhost:11434",
        "health_check_interval_secs": 5
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // 範囲上限（300超）
    let payload = json!({
        "name": "Test2",
        "base_url": "http://localhost:11435",
        "health_check_interval_secs": 500
    });

    let response = app
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
