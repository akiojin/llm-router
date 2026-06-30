//! Contract Test: POST /api/endpoints/:id/sync
//!
//! SPEC-e8e9326e: エンドポイントモデル同期API契約テスト
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
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestApp {
    app: Router,
    admin_key: String,
    db_pool: SqlitePool,
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
    TestApp {
        app,
        admin_key,
        db_pool,
    }
}

fn admin_request(admin_key: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", admin_key))
}

/// POST /api/endpoints/:id/sync - 正常系: モデル同期成功
#[tokio::test]
#[serial]
async fn test_endpoint_sync_success() {
    let mock = MockServer::start().await;

    // モックエンドポイントのレスポンス設定
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"id": "gpt-4", "object": "model"},
                {"id": "gpt-3.5-turbo", "object": "model"},
                {"id": "text-embedding-ada-002", "object": "model"}
            ]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key, .. } = build_app().await;

    // エンドポイント登録
    let payload = json!({
        "name": "Mock Endpoint",
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

    // モデル同期
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/sync", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 契約に基づくレスポンス検証
    assert!(body["synced_models"].is_array());
    assert!(body["added"].is_number());
    assert!(body["removed"].is_number());
    assert!(body["updated"].is_number());

    // 同期されたモデルの検証
    let synced_models = body["synced_models"].as_array().unwrap();
    assert!(!synced_models.is_empty());

    for model in synced_models {
        assert!(model["model_id"].is_string());
        // capabilitiesは配列またはnull
        assert!(model["capabilities"].is_array() || model["capabilities"].is_null());
    }
}

/// POST /api/endpoints/:id/sync - 異常系: 存在しないエンドポイント
#[tokio::test]
#[serial]
async fn test_endpoint_sync_not_found() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/sync", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// POST /api/endpoints/:id/sync - 異常系: エンドポイントがオフライン
#[tokio::test]
#[serial]
async fn test_endpoint_sync_offline() {
    // MockServerで登録を成功させた後、DBのbase_urlを到達不能アドレスに直接変更
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let TestApp {
        app,
        admin_key,
        db_pool,
    } = build_app().await;

    // エンドポイント登録（MockServerが稼働中なので成功する）
    let payload = json!({
        "name": "Offline Endpoint",
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

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let create_body = to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let endpoint_id = create_body["id"].as_str().unwrap().to_string();

    // DBのbase_urlを到達不能なアドレスに直接変更（API経由だと再検出が走るため）
    sqlx::query("UPDATE endpoints SET base_url = ? WHERE id = ?")
        .bind("http://127.0.0.1:59999")
        .bind(&endpoint_id)
        .execute(&db_pool)
        .await
        .expect("Failed to update base_url in DB");

    // モデル同期（失敗が予想される）
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/sync", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 503 Service Unavailableが期待される
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// POST /api/endpoints/:id/sync - 異常系: 認証なし
#[tokio::test]
#[serial]
async fn test_endpoint_sync_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/endpoints/{}/sync", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/endpoints/:id/sync - 正常系: 空のモデル一覧
#[tokio::test]
#[serial]
async fn test_endpoint_sync_empty_models() {
    let mock = MockServer::start().await;

    // 空のモデル一覧を返すモック
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": []
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key, .. } = build_app().await;

    // エンドポイント登録
    let payload = json!({
        "name": "Empty Models Endpoint",
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

    // モデル同期
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{}/sync", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["synced_models"].is_array());
    assert_eq!(body["synced_models"].as_array().unwrap().len(), 0);
}
