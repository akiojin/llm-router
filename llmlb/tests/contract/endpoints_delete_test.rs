//! Contract Test: DELETE /api/endpoints/:id
//!
//! SPEC-e8e9326e: エンドポイント削除API契約テスト
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

/// DELETE /api/endpoints/:id - 正常系: エンドポイント削除成功
#[tokio::test]
#[serial]
async fn test_delete_endpoint_success() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録
    let payload = json!({
        "name": "To Be Deleted",
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

    // 削除
    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 削除後に取得できないことを確認
    let get_response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/endpoints/:id - 異常系: 存在しないID
#[tokio::test]
#[serial]
async fn test_delete_endpoint_not_found() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/endpoints/:id - 異常系: 認証なし
#[tokio::test]
#[serial]
async fn test_delete_endpoint_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// DELETE /api/endpoints/:id - 正常系: 削除後に一覧から消えることを確認
#[tokio::test]
#[serial]
async fn test_delete_endpoint_removes_from_list() {
    let TestApp { app, admin_key } = build_app().await;
    let mock = start_detectable_endpoint_server().await;

    // エンドポイント登録
    let payload = json!({
        "name": "To Be Deleted",
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

    // 一覧に存在することを確認
    let list_response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let list_body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_body: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_body["total"], 1);

    // 削除
    let _ = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 一覧から消えていることを確認
    let list_response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let list_body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_body: Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(list_body["total"], 0);
}
