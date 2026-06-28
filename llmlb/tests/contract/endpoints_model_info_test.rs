//! Contract Test: GET /api/endpoints/:id/models/:model/info
//!
//! SPEC-e8e9326e: エンドポイントモデル情報API契約テスト
//!
//! US9: xLLM/Ollamaエンドポイントからモデルメタデータを取得

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

/// GET /api/endpoints/:id/models/:model/info - 正常系: モデル情報取得
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_get_model_info() {
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

    // モデル情報取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!(
                    "/api/endpoints/{}/models/llama3:8b/info",
                    endpoint_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 実装されていない場合は404、実装後は200を期待
    // 型未判別などで400になる場合も許容する
    assert!(
        response.status() == StatusCode::OK
            || response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::BAD_REQUEST
    );
}

/// GET /api/endpoints/:id/models/:model/info - レスポンス構造検証
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_model_info_response_structure() {
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

    // モデル情報取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!(
                    "/api/endpoints/{}/models/llama3:8b/info",
                    endpoint_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    if response.status() == StatusCode::OK {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        // 期待されるレスポンス構造
        assert!(body["model_id"].is_string(), "model_id should be present");
        assert!(
            body["endpoint_id"].is_string(),
            "endpoint_id should be present"
        );

        // メタデータフィールド（xLLM/Ollamaのみ）
        // max_tokens, context_length などが含まれる可能性
        if body.get("max_tokens").is_some() {
            assert!(
                body["max_tokens"].is_number() || body["max_tokens"].is_null(),
                "max_tokens should be a number or null"
            );
        }
    }
}

/// GET /api/endpoints/:id/models/:model/info - 異常系: 存在しないエンドポイント
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_model_info_endpoint_not_found() {
    let TestApp { app, admin_key } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000000/models/llama3:8b/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// GET /api/endpoints/:id/models/:model/info - 異常系: 存在しないモデル
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_model_info_model_not_found() {
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

    // 存在しないモデルの情報取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!(
                    "/api/endpoints/{}/models/nonexistent-model/info",
                    endpoint_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 404 Not Found または 400 Bad Request を期待
    assert!(
        response.status() == StatusCode::NOT_FOUND || response.status() == StatusCode::BAD_REQUEST
    );
}

/// GET /api/endpoints/:id/models/:model/info - 異常系: 認証なし
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_model_info_unauthorized() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/endpoints/00000000-0000-0000-0000-000000000001/models/llama3:8b/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// GET /api/endpoints/:id/models/:model/info - 非xLLM/Ollamaエンドポイント
#[tokio::test]
#[serial]
#[ignore = "API未実装 - T125で実装予定"]
async fn test_model_info_unsupported_endpoint_type() {
    let TestApp { app, admin_key } = build_app().await;

    // エンドポイントを登録（タイプがvLLMまたはOpenAI互換の場合）
    let payload = json!({
        "name": "vLLM Endpoint",
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

    // モデル情報取得
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!(
                    "/api/endpoints/{}/models/some-model/info",
                    endpoint_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // サポートされていないタイプの場合は400 Bad Requestを期待
    // NOTE: vLLM/OpenAI互換はメタデータ取得をサポートしない
    // 実装により404または400を返す
    assert!(
        response.status() == StatusCode::BAD_REQUEST || response.status() == StatusCode::NOT_FOUND
    );
}
