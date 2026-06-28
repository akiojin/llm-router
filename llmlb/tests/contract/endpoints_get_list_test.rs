//! Contract Test: GET /api/endpoints
//!
//! SPEC-e8e9326e: エンドポイント一覧取得API契約テスト
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
use std::time::{Duration, Instant};
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

/// GET /api/endpoints - 正常系: 空の一覧
#[tokio::test]
#[serial]
async fn test_list_endpoints_empty() {
    let TestApp { app, admin_key } = build_app().await;

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

    assert!(body["endpoints"].is_array());
    assert_eq!(body["endpoints"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

/// GET /api/endpoints - 正常系: 複数エンドポイントの一覧
#[tokio::test]
#[serial]
async fn test_list_endpoints_multiple() {
    // 2つのMockServerを起動（各エンドポイント用）
    let mut mocks = Vec::new();
    for _ in 0..2 {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": "list",
                "data": [{"id": "test-model", "object": "model"}]
            })))
            .mount(&mock)
            .await;
        mocks.push(mock);
    }

    let TestApp { app, admin_key } = build_app().await;

    // 2つのエンドポイントを登録
    for (i, mock) in mocks.iter().enumerate() {
        let payload = json!({
            "name": format!("Endpoint {}", i + 1),
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
    }

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
    assert_eq!(endpoints.len(), 2);
    assert_eq!(body["total"], 2);

    // 各エンドポイントの構造検証
    for endpoint in endpoints {
        assert!(endpoint["id"].is_string());
        assert!(endpoint["name"].is_string());
        assert!(endpoint["base_url"].is_string());
        assert!(endpoint["status"].is_string());
        assert!(endpoint["health_check_interval_secs"].is_number());
        assert!(endpoint["registered_at"].is_string());
        // model_countが含まれる
        assert!(endpoint["model_count"].is_number());
    }
}

/// GET /api/endpoints - 正常系: ステータスフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_status() {
    let TestApp { app, admin_key } = build_app().await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"id": "stable-diffusion-xl", "object": "model"}
            ]
        })))
        .mount(&mock)
        .await;

    // エンドポイントを登録（初期状態はpending）
    let payload = json!({
        "name": "Test Endpoint",
        "base_url": mock.uri()
    });

    let _ = app
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

    // 接続チェック完了まで待機（オンラインになるのを確認）
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = app
            .clone()
            .oneshot(
                admin_request(&admin_key)
                    .method("GET")
                    .uri("/api/endpoints?status=online")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let online_count = body["endpoints"].as_array().unwrap().len();

        if online_count == 1 {
            break;
        }

        if Instant::now() > deadline {
            panic!("Timed out waiting for endpoint to become online");
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // pendingでフィルタ（該当なし）
    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?status=pending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["endpoints"].as_array().unwrap().len(), 0);

    // onlineでフィルタ（該当あり）
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?status=online")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["endpoints"].as_array().unwrap().len(), 1);
}

/// GET /api/endpoints - 異常系: 認証なし
#[tokio::test]
#[serial]
async fn test_list_endpoints_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
