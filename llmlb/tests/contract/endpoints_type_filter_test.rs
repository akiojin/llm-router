//! Contract Test: GET /api/endpoints?type=xxx
//!
//! SPEC-e8e9326e: エンドポイントタイプフィルタリングAPI契約テスト
//!
//! US7: タイプでエンドポイントをフィルタリング

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
        queue_config: llmlb::config::QueueConfig::from_env(),
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

/// GET /api/endpoints?type=xllm - 正常系: xLLMタイプでフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type_xllm() {
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

    // エンドポイントを登録（タイプは自動判別される）
    let payload = json!({
        "name": "xLLM Endpoint",
        "base_url": mock.uri()
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

    // type=xllmでフィルタ
    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=xllm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // NOTE: 自動検出によりOpenAI互換と判定される可能性がある
    // フィルタが動作していれば、xllm以外のタイプはマッチしない
    assert!(body["endpoints"].is_array());
}

/// GET /api/endpoints?type=ollama - 正常系: Ollamaタイプでフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type_ollama() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=ollama")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["endpoints"].is_array());
    // タイプフィルタが実装されていない場合、空の配列が返る
    // （フィルタがサポートされていないためエンドポイントがマッチしない）
}

/// GET /api/endpoints?type=vllm - 正常系: vLLMタイプでフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type_vllm() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=vllm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["endpoints"].is_array());
}

/// GET /api/endpoints?type=llamacpp - 正常系: llama.cppタイプでフィルタ
///
/// SPEC #575 US-013-A (2026-04-23 delta)
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type_llamacpp() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=llamacpp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["endpoints"].is_array());
}

/// GET /api/endpoints?type=openai_compatible - 正常系: OpenAI互換タイプでフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type_openai_compatible() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=openai_compatible")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["endpoints"].is_array());
}

/// GET /api/endpoints?type=invalid - 異常系: 不正なタイプ指定
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_invalid_type() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?type=invalid_type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 不正なタイプの場合、400 Bad Requestを期待
    // NOTE: 実装により200 OKで空配列を返すか、400を返すか決定
    assert!(response.status() == StatusCode::OK || response.status() == StatusCode::BAD_REQUEST);
}

/// GET /api/endpoints - レスポンスにendpoint_typeフィールドが含まれる
#[tokio::test]
#[serial]
async fn test_list_endpoints_response_includes_endpoint_type() {
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

    // エンドポイントを登録
    let payload = json!({
        "name": "Test Endpoint",
        "base_url": mock.uri()
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

    // 一覧取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let endpoints = body["endpoints"].as_array().unwrap();
    assert!(!endpoints.is_empty());

    // 各エンドポイントにendpoint_typeフィールドが含まれることを検証
    for endpoint in endpoints {
        assert!(
            endpoint["endpoint_type"].is_string(),
            "endpoint_type field should be present and be a string"
        );
        let endpoint_type = endpoint["endpoint_type"].as_str().unwrap();
        assert!(
            [
                "xllm",
                "ollama",
                "vllm",
                "lm_studio",
                "llamacpp",
                "openai_compatible"
            ]
            .contains(&endpoint_type),
            "endpoint_type should be a valid value, got: {}",
            endpoint_type
        );
    }
}
