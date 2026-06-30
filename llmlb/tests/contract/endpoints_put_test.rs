//! Contract Test: PUT /api/endpoints/:id
//!
//! SPEC-e8e9326e: エンドポイント更新API契約テスト
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
use uuid::Uuid;
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

async fn start_detectable_endpoint_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model"}]
        })))
        .mount(&server)
        .await;
    server
}

/// PUT /api/endpoints/:id - 正常系: 名前の更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_name() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録
    let payload = json!({
        "name": "Original Name",
        "base_url": mock.uri()
    });

    let create_response = app
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

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let endpoint_id = create_body["id"].as_str().unwrap();

    // 名前を更新
    let update_payload = json!({
        "name": "Updated Name"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["id"], endpoint_id);
    assert_eq!(body["name"], "Updated Name");
    assert_eq!(body["base_url"], mock.uri());
}

/// PUT /api/endpoints/:id - 正常系: ヘルスチェック間隔の更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_health_check_interval() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録
    let payload = json!({
        "name": "Test",
        "base_url": mock.uri()
    });

    let create_response = app
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

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let endpoint_id = create_body["id"].as_str().unwrap();

    // ヘルスチェック間隔を更新
    let update_payload = json!({
        "health_check_interval_secs": 120
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["health_check_interval_secs"], 120);
}

/// PUT /api/endpoints/:id - 正常系: notesの更新（nullで削除）
#[tokio::test]
#[serial]
async fn test_update_endpoint_notes() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録（notes付き）
    let payload = json!({
        "name": "Test",
        "base_url": mock.uri(),
        "notes": "Initial notes"
    });

    let create_response = app
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

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let endpoint_id = create_body["id"].as_str().unwrap();

    // notesをnullで削除
    let update_payload = json!({
        "notes": null
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["notes"].is_null());
}

/// PUT /api/endpoints/:id - 異常系: 存在しないID
#[tokio::test]
#[serial]
async fn test_update_endpoint_not_found() {
    let TestApp { app, admin_key } = build_app().await;

    let update_payload = json!({
        "name": "Updated"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", Uuid::new_v4()))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// PUT /api/endpoints/:id - 異常系: バリデーションエラー（空の名前）
#[tokio::test]
#[serial]
async fn test_update_endpoint_validation_error() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録
    let payload = json!({
        "name": "Test",
        "base_url": mock.uri()
    });

    let create_response = app
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

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let endpoint_id = create_body["id"].as_str().unwrap();

    // 空の名前で更新
    let update_payload = json!({
        "name": ""
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// PUT /api/endpoints/:id - 異常系: 認証なし
#[tokio::test]
#[serial]
async fn test_update_endpoint_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let update_payload = json!({
        "name": "Updated"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/endpoints/{}", Uuid::new_v4()))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
