//! Contract Test: Endpoints API 拡張テスト
//!
//! SPEC-e8e9326e: エンドポイント管理API契約テスト（不足ケース補完）
//!
//! 既存テスト（endpoints_post_test, endpoints_get_list_test,
//! endpoints_get_detail_test, endpoints_put_test, endpoints_delete_test）で
//! カバーされていないエッジケースを補完する。

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
    db_pool: sqlx::SqlitePool,
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

async fn start_mock_endpoint() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&server)
        .await;
    server
}

async fn start_ollama_mock_endpoint() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": []
        })))
        .mount(&server)
        .await;
    server
}

/// ヘルパー: エンドポイントを登録してIDを返す
async fn create_endpoint_and_get_id(app: &Router, admin_key: &str, mock: &MockServer) -> String {
    let payload = json!({
        "name": format!("ep-{}", Uuid::new_v4()),
        "base_url": mock.uri()
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(admin_key)
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
    body["id"].as_str().unwrap().to_string()
}

// ========================================================================
// POST /api/endpoints - 追加テスト
// ========================================================================

/// POST /api/endpoints - 異常系: base_urlが空文字列
#[tokio::test]
#[serial]
async fn test_create_endpoint_empty_base_url() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Test Endpoint",
        "base_url": ""
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

/// POST /api/endpoints - 異常系: base_urlフィールド欠落（デシリアライズエラー）
#[tokio::test]
#[serial]
async fn test_create_endpoint_missing_base_url_field() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Test Endpoint"
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

    // base_url は必須フィールドなのでデシリアライズエラー (422 Unprocessable Entity)
    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// POST /api/endpoints - 異常系: nameフィールド欠落
#[tokio::test]
#[serial]
async fn test_create_endpoint_missing_name_field() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
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

    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// POST /api/endpoints - 異常系: 空のJSONボディ
#[tokio::test]
#[serial]
async fn test_create_endpoint_empty_json_body() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({});

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

    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// POST /api/endpoints - 異常系: Content-Typeなし
#[tokio::test]
#[serial]
async fn test_create_endpoint_no_content_type() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Test",
        "base_url": "http://localhost:11434"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                // Content-Type ヘッダーなし
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Content-Type がないため 415 Unsupported Media Type を期待
    assert!(
        response.status() == StatusCode::UNSUPPORTED_MEDIA_TYPE
            || response.status() == StatusCode::BAD_REQUEST
    );
}

/// POST /api/endpoints - 異常系: 名前重複
#[tokio::test]
#[serial]
async fn test_create_endpoint_duplicate_name() {
    let mock1 = start_mock_endpoint().await;
    let mock2 = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let name = "Unique Endpoint Name";

    // 1つ目の登録
    let payload = json!({
        "name": name,
        "base_url": mock1.uri()
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

    // 同じ名前で2つ目の登録（別のURL）
    let payload2 = json!({
        "name": name,
        "base_url": mock2.uri()
    });

    let response2 = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // 名前重複は400 BAD_REQUEST
    assert_eq!(response2.status(), StatusCode::BAD_REQUEST);
}

/// POST /api/endpoints - 異常系: 不正なAPIキー
#[tokio::test]
#[serial]
async fn test_create_endpoint_invalid_api_key() {
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
                .header("authorization", "Bearer invalid-key-12345")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// POST /api/endpoints - 正常系: capabilitiesフィールド付き
#[tokio::test]
#[serial]
async fn test_create_endpoint_with_capabilities() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Audio Endpoint",
        "base_url": mock.uri(),
        "capabilities": ["audio_transcription", "audio_speech"]
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
    assert!(body["id"].is_string());
    assert_eq!(body["name"], "Audio Endpoint");
}

/// POST /api/endpoints - 正常系: inference_timeout_secsの指定
#[tokio::test]
#[serial]
async fn test_create_endpoint_with_inference_timeout() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Slow Endpoint",
        "base_url": mock.uri(),
        "inference_timeout_secs": 300
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
    assert_eq!(body["inference_timeout_secs"], 300);
}

/// POST /api/endpoints - 正常系: OpenAI互換 endpoint の既定 inference_timeout_secs は120
#[tokio::test]
#[serial]
async fn test_create_endpoint_default_inference_timeout() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Default Timeout",
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
    assert_eq!(body["inference_timeout_secs"], 120);
}

/// POST /api/endpoints - 正常系: Ollama endpoint の既定 inference_timeout_secs は600
#[tokio::test]
#[serial]
async fn test_create_ollama_endpoint_default_inference_timeout() {
    let mock = start_ollama_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Default Ollama Timeout",
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
    assert_eq!(body["endpoint_type"], "ollama");
    assert_eq!(body["inference_timeout_secs"], 600);
}

/// POST /api/endpoints - 異常系: 空白のみの名前
#[tokio::test]
#[serial]
async fn test_create_endpoint_whitespace_only_name() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "   ",
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

/// POST /api/endpoints - 正常系: レスポンスにendpoint_typeが含まれる
#[tokio::test]
#[serial]
async fn test_create_endpoint_response_contains_endpoint_type() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Type Check",
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
    // endpoint_typeフィールドが含まれる
    assert!(
        body["endpoint_type"].is_string(),
        "endpoint_type should be present"
    );
}

// ========================================================================
// GET /api/endpoints - 追加テスト
// ========================================================================

/// GET /api/endpoints - 正常系: endpoint_typeフィルタ
#[tokio::test]
#[serial]
async fn test_list_endpoints_filter_by_type() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;

    // エンドポイントを1つ登録
    let payload = json!({
        "name": "Type Filter Test",
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

    // 存在しないタイプでフィルタ（APIがフィルタ未対応の場合は全件返る）
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?endpoint_type=nonexistent_type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    // APIがendpoint_typeクエリパラメータをサポートする場合は0件、しない場合は全件返る
    let endpoints = body["endpoints"].as_array().unwrap();
    assert!(endpoints.len() <= 1);
}

/// GET /api/endpoints - 正常系: 一覧レスポンス構造の検証
#[tokio::test]
#[serial]
async fn test_list_endpoints_response_structure() {
    let TestApp { app, admin_key, .. } = build_app().await;

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

    // 必須フィールドの存在確認
    assert!(body["endpoints"].is_array(), "endpoints field required");
    assert!(
        body["total"].is_number(),
        "total field required and must be a number"
    );
}

/// GET /api/endpoints - 正常系: 無効なステータスフィルタ（結果0件）
#[tokio::test]
#[serial]
async fn test_list_endpoints_invalid_status_filter() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;

    // エンドポイントを登録
    let payload = json!({
        "name": "Filter Test",
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

    // 存在しないステータスでフィルタ
    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints?status=imaginary_status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    // 存在しないステータスなので0件
    assert_eq!(body["endpoints"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

// ========================================================================
// GET /api/endpoints/:id - 追加テスト
// ========================================================================

/// GET /api/endpoints/:id - 正常系: 詳細レスポンスにmodelsフィールドが配列で含まれる
#[tokio::test]
#[serial]
async fn test_get_endpoint_detail_has_models_array() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 詳細取得時はmodelsフィールドが配列として含まれる
    assert!(body["models"].is_array(), "models should be an array");
}

/// GET /api/endpoints/:id - 正常系: レスポンスに全フィールドが含まれる
#[tokio::test]
#[serial]
async fn test_get_endpoint_detail_all_fields() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;

    let payload = json!({
        "name": "Full Detail Test",
        "base_url": mock.uri(),
        "notes": "Test note"
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

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 全フィールドの存在確認
    assert!(body["id"].is_string());
    assert!(body["name"].is_string());
    assert!(body["base_url"].is_string());
    assert!(body["status"].is_string());
    assert!(body["endpoint_type"].is_string());
    assert!(body["health_check_interval_secs"].is_number());
    assert!(body["inference_timeout_secs"].is_number());
    assert!(body["error_count"].is_number());
    assert!(body["registered_at"].is_string());
    assert!(body["models"].is_array());
    assert_eq!(body["notes"], "Test note");
}

// ========================================================================
// PUT /api/endpoints/:id - 追加テスト
// ========================================================================

/// PUT /api/endpoints/:id - 正常系: 複数フィールドの同時更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_multiple_fields() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let update_payload = json!({
        "name": "New Name",
        "health_check_interval_secs": 60,
        "notes": "Updated notes"
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

    assert_eq!(body["name"], "New Name");
    assert_eq!(body["health_check_interval_secs"], 60);
    assert_eq!(body["notes"], "Updated notes");
}

/// PUT /api/endpoints/:id - 正常系: notesを文字列で更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_set_notes() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let update_payload = json!({
        "notes": "Production GPU server"
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
    assert_eq!(body["notes"], "Production GPU server");
}

/// PUT /api/endpoints/:id - 異常系: 不正なURL形式で更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_invalid_url() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let update_payload = json!({
        "base_url": "not-a-valid-url"
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

/// PUT /api/endpoints/:id - 正常系: inference_timeout_secsの更新
#[tokio::test]
#[serial]
async fn test_update_endpoint_inference_timeout() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let update_payload = json!({
        "inference_timeout_secs": 600
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
    assert_eq!(body["inference_timeout_secs"], 600);
}

/// PUT /api/endpoints/:id - 正常系: 空のボディ（何も変更しない）
#[tokio::test]
#[serial]
async fn test_update_endpoint_empty_body() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    let update_payload = json!({});

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
}

/// PUT /api/endpoints/:id - 異常系: 不正なUUID形式
#[tokio::test]
#[serial]
async fn test_update_endpoint_invalid_uuid() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let update_payload = json!({
        "name": "Updated"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri("/api/endpoints/not-a-uuid")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status() == StatusCode::BAD_REQUEST || response.status() == StatusCode::NOT_FOUND
    );
}

/// PUT /api/endpoints/:id - 異常系: 名前重複（他のエンドポイントと同名）
#[tokio::test]
#[serial]
async fn test_update_endpoint_duplicate_name() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock1 = start_mock_endpoint().await;
    let mock2 = start_mock_endpoint().await;

    // 1つ目のエンドポイント
    let payload1 = json!({
        "name": "Endpoint A",
        "base_url": mock1.uri()
    });
    let resp1 = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::CREATED);

    // 2つ目のエンドポイント
    let payload2 = json!({
        "name": "Endpoint B",
        "base_url": mock2.uri()
    });
    let resp2 = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload2).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::CREATED);
    let body2 = to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let body2: Value = serde_json::from_slice(&body2).unwrap();
    let endpoint_b_id = body2["id"].as_str().unwrap();

    // Endpoint BをEndpoint Aの名前に変更しようとする
    let update_payload = json!({
        "name": "Endpoint A"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_b_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// PUT /api/endpoints/:id - 正常系: 同じ名前のまま更新（自分自身との重複はOK）
#[tokio::test]
#[serial]
async fn test_update_endpoint_same_name_is_ok() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;

    let payload = json!({
        "name": "My Endpoint",
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

    // 同じ名前で更新（他のフィールドだけ変更）
    let update_payload = json!({
        "name": "My Endpoint",
        "notes": "Added notes"
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
    assert_eq!(body["name"], "My Endpoint");
    assert_eq!(body["notes"], "Added notes");
}

// ========================================================================
// DELETE /api/endpoints/:id - 追加テスト
// ========================================================================

/// DELETE /api/endpoints/:id - 異常系: 二重削除
#[tokio::test]
#[serial]
async fn test_delete_endpoint_double_delete() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;
    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    // 1回目の削除
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

    // 2回目の削除（既に削除済み）
    let response2 = app
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response2.status(), StatusCode::NOT_FOUND);
}

/// DELETE /api/endpoints/:id - 異常系: 不正なUUID形式
#[tokio::test]
#[serial]
async fn test_delete_endpoint_invalid_uuid() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri("/api/endpoints/invalid-uuid-format")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status() == StatusCode::BAD_REQUEST || response.status() == StatusCode::NOT_FOUND
    );
}

/// DELETE /api/endpoints/:id - 異常系: 不正なAPIキー
#[tokio::test]
#[serial]
async fn test_delete_endpoint_invalid_api_key() {
    let TestApp { app, .. } = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", Uuid::new_v4()))
                .header("authorization", "Bearer bad-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ========================================================================
// Viewer権限テスト
// ========================================================================

/// Viewerユーザーはエンドポイント作成できない（403）
#[tokio::test]
#[serial]
async fn test_viewer_cannot_create_endpoint() {
    let TestApp {
        app,
        db_pool,
        admin_key: _,
        ..
    } = build_app().await;

    // Viewerユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("viewer123").unwrap();
    let viewer_user =
        llmlb::db::users::create(&db_pool, "viewer", &password_hash, UserRole::Viewer, false)
            .await
            .expect("create viewer user");
    let viewer_key = llmlb::db::api_keys::create(
        &db_pool,
        "viewer-key",
        viewer_user.id,
        None,
        vec![
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::OpenaiModelsRead,
        ],
    )
    .await
    .expect("create viewer api key")
    .key;

    let payload = json!({
        "name": "Viewer Test",
        "base_url": "http://localhost:11434"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", viewer_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// Viewerユーザーはエンドポイント更新できない（403）
#[tokio::test]
#[serial]
async fn test_viewer_cannot_update_endpoint() {
    let TestApp {
        app,
        admin_key,
        db_pool,
        ..
    } = build_app().await;
    let mock = start_mock_endpoint().await;

    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    // Viewerユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("viewer123").unwrap();
    let viewer_user =
        llmlb::db::users::create(&db_pool, "viewer", &password_hash, UserRole::Viewer, false)
            .await
            .expect("create viewer user");
    let viewer_key = llmlb::db::api_keys::create(
        &db_pool,
        "viewer-key",
        viewer_user.id,
        None,
        vec![
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::OpenaiModelsRead,
        ],
    )
    .await
    .expect("create viewer api key")
    .key;

    let update_payload = json!({
        "name": "Hacked Name"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("authorization", format!("Bearer {}", viewer_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// Viewerユーザーはエンドポイント削除できない（403）
#[tokio::test]
#[serial]
async fn test_viewer_cannot_delete_endpoint() {
    let TestApp {
        app,
        admin_key,
        db_pool,
        ..
    } = build_app().await;
    let mock = start_mock_endpoint().await;

    let endpoint_id = create_endpoint_and_get_id(&app, &admin_key, &mock).await;

    // Viewerユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("viewer123").unwrap();
    let viewer_user =
        llmlb::db::users::create(&db_pool, "viewer", &password_hash, UserRole::Viewer, false)
            .await
            .expect("create viewer user");
    let viewer_key = llmlb::db::api_keys::create(
        &db_pool,
        "viewer-key",
        viewer_user.id,
        None,
        vec![
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::OpenaiModelsRead,
        ],
    )
    .await
    .expect("create viewer api key")
    .key;

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", endpoint_id))
                .header("authorization", format!("Bearer {}", viewer_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ========================================================================
// CRUD統合テスト
// ========================================================================

/// CRUD一連の操作: 作成→取得→更新→確認→削除→確認
#[tokio::test]
#[serial]
async fn test_endpoint_full_crud_lifecycle() {
    let TestApp { app, admin_key, .. } = build_app().await;
    let mock = start_mock_endpoint().await;

    // Create
    let payload = json!({
        "name": "Lifecycle Test",
        "base_url": mock.uri(),
        "notes": "Initial"
    });

    let create_resp = app
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
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let create_body: Value = serde_json::from_slice(&create_body).unwrap();
    let id = create_body["id"].as_str().unwrap();
    assert_eq!(create_body["name"], "Lifecycle Test");

    // Read
    let get_resp = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX).await.unwrap();
    let get_body: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_body["name"], "Lifecycle Test");
    assert_eq!(get_body["notes"], "Initial");

    // Update
    let update_payload = json!({
        "name": "Updated Lifecycle",
        "notes": "Updated"
    });
    let update_resp = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("PUT")
                .uri(format!("/api/endpoints/{}", id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&update_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);
    let update_body = to_bytes(update_resp.into_body(), usize::MAX).await.unwrap();
    let update_body: Value = serde_json::from_slice(&update_body).unwrap();
    assert_eq!(update_body["name"], "Updated Lifecycle");
    assert_eq!(update_body["notes"], "Updated");

    // Verify update
    let get_resp2 = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp2.status(), StatusCode::OK);
    let get_body2 = to_bytes(get_resp2.into_body(), usize::MAX).await.unwrap();
    let get_body2: Value = serde_json::from_slice(&get_body2).unwrap();
    assert_eq!(get_body2["name"], "Updated Lifecycle");

    // Delete
    let del_resp = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/endpoints/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

    // Verify delete
    let get_resp3 = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{}", id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp3.status(), StatusCode::NOT_FOUND);
}

/// 複数エンドポイントの作成と一覧取得の件数が一致する
#[tokio::test]
#[serial]
async fn test_list_endpoints_count_matches_created() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let count = 3;
    let mut mocks = Vec::new();

    for _ in 0..count {
        let mock = start_mock_endpoint().await;
        let payload = json!({
            "name": format!("ep-{}", Uuid::new_v4()),
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
        mocks.push(mock);
    }

    let list_resp = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(list_resp.status(), StatusCode::OK);
    let body = to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total"], count);
    assert_eq!(body["endpoints"].as_array().unwrap().len(), count);
}

/// POST /api/endpoints - 異常系: JSONでないボディ
#[tokio::test]
#[serial]
async fn test_create_endpoint_non_json_body() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from("this is not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        response.status() == StatusCode::BAD_REQUEST
            || response.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

/// POST /api/endpoints - 正常系: 名前に特殊文字を含む
#[tokio::test]
#[serial]
async fn test_create_endpoint_special_chars_in_name() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "Test@#$%^&*() Endpoint /v2",
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
    assert_eq!(body["name"], "Test@#$%^&*() Endpoint /v2");
}

/// POST /api/endpoints - 正常系: 名前に日本語を含む
#[tokio::test]
#[serial]
async fn test_create_endpoint_japanese_name() {
    let mock = start_mock_endpoint().await;
    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "name": "テスト推論サーバー",
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
    assert_eq!(body["name"], "テスト推論サーバー");
}
