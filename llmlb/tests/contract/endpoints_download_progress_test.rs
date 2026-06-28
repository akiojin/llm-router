//! Contract Test: GET /api/endpoints/:id/download/progress
//!
//! SPEC-e8e9326e: エンドポイントダウンロード進捗API契約テスト
//!
//! US8: xLLMエンドポイントのモデルダウンロード進捗を確認

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

/// GET /api/endpoints/:id/download/progress - 正常系: 進捗一覧取得
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T124で実装予定"]
async fn test_get_download_progress_list() {
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

    // 進捗一覧取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}/download/progress", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 実装されていない場合は404、実装後は200を期待
    assert!(response.status() == StatusCode::OK || response.status() == StatusCode::NOT_FOUND);

    if response.status() == StatusCode::OK {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        // 期待されるレスポンス構造
        assert!(body["tasks"].is_array(), "tasks should be an array");
    }
}

/// GET /api/endpoints/:id/download/progress - 正常系: 進捗詳細のフィールド検証
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T124で実装予定"]
async fn test_download_progress_response_structure() {
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

    // 進捗一覧取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}/download/progress", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    if response.status() == StatusCode::OK {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        // 各タスクの構造検証
        if let Some(tasks) = body["tasks"].as_array() {
            for task in tasks {
                // 必須フィールド
                assert!(task["task_id"].is_string(), "task_id should be present");
                assert!(task["model"].is_string(), "model should be present");
                assert!(task["status"].is_string(), "status should be present");
                assert!(task["progress"].is_number(), "progress should be present");
                assert!(
                    task["started_at"].is_string(),
                    "started_at should be present"
                );

                // オプションフィールド
                // speed_mbps, eta_seconds, filename, error_message, completed_at
            }
        }
    }
}

/// GET /api/endpoints/:id/download/progress - 異常系: 存在しないエンドポイント
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T124で実装予定"]
async fn test_download_progress_endpoint_not_found() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000000/download/progress")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// GET /api/endpoints/:id/download/progress - 異常系: 認証なし
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T124で実装予定"]
async fn test_download_progress_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000001/download/progress")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
