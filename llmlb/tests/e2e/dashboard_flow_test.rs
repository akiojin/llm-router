//! ダッシュボードフローE2Eテスト
//!
//! ダッシュボードAPI（/api/dashboard/*）のE2Eテスト
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::UserRole;
use llmlb::{
    api, balancer::LoadManager, db::endpoints as db_endpoints,
    registry::endpoints::EndpointRegistry, types::endpoint::Endpoint, AppState,
};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool, String) {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

    let db_pool = support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = support::lb::test_jwt_secret();

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
    let admin_user =
        llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
            .await
            .expect("create admin user");
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin_user.id.to_string(),
        UserRole::Admin,
        &support::lb::test_jwt_secret(),
        false,
    )
    .expect("create admin jwt");

    (api::create_app(state), db_pool, jwt)
}

#[tokio::test]
async fn test_dashboard_stats_endpoint() {
    let (app, db_pool, jwt) = build_app().await;

    // Seed audit_log_entries with token usage to ensure `/api/dashboard/stats` token totals
    // match the persisted statistics (used by the Statistics tab).
    // SPEC-8301d106: stats are now served from audit_log_entries, not request_history.
    let audit_storage = llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone());
    let now = chrono::Utc::now();
    let entry = llmlb::audit::types::AuditLogEntry {
        id: None,
        timestamp: now,
        http_method: "POST".to_string(),
        request_path: "/v1/chat/completions".to_string(),
        status_code: 200,
        actor_type: llmlb::audit::types::ActorType::User,
        actor_id: Some("1".to_string()),
        actor_username: Some("admin".to_string()),
        api_key_owner_id: None,
        client_ip: Some("127.0.0.1".to_string()),
        duration_ms: Some(123),
        input_tokens: Some(150),
        output_tokens: Some(50),
        total_tokens: Some(200),
        model_name: Some("test-model".to_string()),
        endpoint_id: None,
        detail: None,
        is_migrated: false,
        batch_id: None,
    };
    audit_storage.insert_batch(&[entry]).await.unwrap();

    // GET /api/dashboard/stats
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/stats")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/stats should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(stats.is_object(), "Response should be a stats object");

    assert_eq!(
        stats["total_input_tokens"].as_u64(),
        Some(150),
        "total_input_tokens should come from audit_log_entries"
    );
    assert_eq!(
        stats["total_output_tokens"].as_u64(),
        Some(50),
        "total_output_tokens should come from audit_log_entries"
    );
    assert_eq!(
        stats["total_tokens"].as_u64(),
        Some(200),
        "total_tokens should come from audit_log_entries"
    );
}

#[tokio::test]
async fn test_dashboard_overview_stats_reflects_persisted_request_totals() {
    let (app, db_pool, jwt) = build_app().await;

    let endpoint = Endpoint::new(
        "Overview Persistence Test".to_string(),
        "http://127.0.0.1:65500".to_string(),
        llmlb::types::endpoint::EndpointType::OpenaiCompatible,
    );
    db_endpoints::create_endpoint(&db_pool, &endpoint)
        .await
        .expect("create endpoint");

    db_endpoints::increment_request_counters(&db_pool, endpoint.id, true)
        .await
        .expect("increment success request counter");
    db_endpoints::increment_request_counters(&db_pool, endpoint.id, true)
        .await
        .expect("increment success request counter");
    db_endpoints::increment_request_counters(&db_pool, endpoint.id, false)
        .await
        .expect("increment failed request counter");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/overview")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/overview should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let overview: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let stats = overview["stats"]
        .as_object()
        .expect("overview should include stats");

    assert_eq!(
        stats["total_requests"].as_u64(),
        Some(3),
        "Total requests in overview should reflect persisted endpoint counters"
    );
    assert_eq!(
        stats["successful_requests"].as_u64(),
        Some(2),
        "Successful requests in overview should reflect persisted endpoint counters"
    );
    assert_eq!(
        stats["failed_requests"].as_u64(),
        Some(1),
        "Failed requests in overview should reflect persisted endpoint counters"
    );
}

#[tokio::test]
async fn test_dashboard_stats_uses_persisted_endpoint_counters() {
    let (app, db_pool, jwt) = build_app().await;

    let endpoint = Endpoint::new(
        "Stats Persistence Test".to_string(),
        "http://127.0.0.1:65535".to_string(),
        llmlb::types::endpoint::EndpointType::OpenaiCompatible,
    );
    db_endpoints::create_endpoint(&db_pool, &endpoint)
        .await
        .expect("create endpoint");

    db_endpoints::increment_request_counters(&db_pool, endpoint.id, true)
        .await
        .expect("increment success request counter");
    db_endpoints::increment_request_counters(&db_pool, endpoint.id, true)
        .await
        .expect("increment success request counter");
    db_endpoints::increment_request_counters(&db_pool, endpoint.id, false)
        .await
        .expect("increment failed request counter");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/stats")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/stats should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        stats["total_requests"].as_u64(),
        Some(3),
        "Total requests should reflect persisted endpoint counters"
    );
    assert_eq!(
        stats["successful_requests"].as_u64(),
        Some(2),
        "Successful requests should reflect persisted endpoint counters"
    );
    assert_eq!(
        stats["failed_requests"].as_u64(),
        Some(1),
        "Failed requests should reflect persisted endpoint counters"
    );
}

#[tokio::test]
async fn test_dashboard_stats_keeps_last_known_persisted_totals_when_db_unavailable() {
    let (app, db_pool, jwt) = build_app().await;

    let endpoint = Endpoint::new(
        "Stats Cache Fallback Test".to_string(),
        "http://127.0.0.1:65534".to_string(),
        llmlb::types::endpoint::EndpointType::OpenaiCompatible,
    );
    db_endpoints::create_endpoint(&db_pool, &endpoint)
        .await
        .expect("create endpoint");

    db_endpoints::increment_request_counters(&db_pool, endpoint.id, true)
        .await
        .expect("increment success request counter");
    db_endpoints::increment_request_counters(&db_pool, endpoint.id, false)
        .await
        .expect("increment failed request counter");

    // SPEC-8301d106: stats are now served from audit_log_entries
    let audit_storage = llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone());
    let now = chrono::Utc::now();
    let entry = llmlb::audit::types::AuditLogEntry {
        id: None,
        timestamp: now,
        http_method: "POST".to_string(),
        request_path: "/v1/chat/completions".to_string(),
        status_code: 200,
        actor_type: llmlb::audit::types::ActorType::User,
        actor_id: Some("1".to_string()),
        actor_username: Some("admin".to_string()),
        api_key_owner_id: None,
        client_ip: Some("127.0.0.1".to_string()),
        duration_ms: Some(80),
        input_tokens: Some(40),
        output_tokens: Some(60),
        total_tokens: Some(100),
        model_name: Some("fallback-model".to_string()),
        endpoint_id: Some(endpoint.id.to_string()),
        detail: None,
        is_migrated: false,
        batch_id: None,
    };
    audit_storage
        .insert_batch(&[entry])
        .await
        .expect("insert audit log entry");

    // Warm cache with persisted totals.
    let first_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/stats")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(first_response.status(), StatusCode::OK);

    let first_body = axum::body::to_bytes(first_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let first_stats: serde_json::Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(first_stats["total_requests"].as_u64(), Some(2));
    assert_eq!(first_stats["successful_requests"].as_u64(), Some(1));
    assert_eq!(first_stats["failed_requests"].as_u64(), Some(1));
    assert_eq!(first_stats["total_input_tokens"].as_u64(), Some(40));
    assert_eq!(first_stats["total_output_tokens"].as_u64(), Some(60));
    assert_eq!(first_stats["total_tokens"].as_u64(), Some(100));

    // Simulate DB outage and verify stats still return the last persisted values.
    db_pool.close().await;

    let second_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/stats")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second_response.status(), StatusCode::OK);

    let second_body = axum::body::to_bytes(second_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_stats: serde_json::Value = serde_json::from_slice(&second_body).unwrap();
    assert_eq!(second_stats["total_requests"].as_u64(), Some(2));
    assert_eq!(second_stats["successful_requests"].as_u64(), Some(1));
    assert_eq!(second_stats["failed_requests"].as_u64(), Some(1));
    assert_eq!(second_stats["total_input_tokens"].as_u64(), Some(40));
    assert_eq!(second_stats["total_output_tokens"].as_u64(), Some(60));
    assert_eq!(second_stats["total_tokens"].as_u64(), Some(100));
}

#[tokio::test]
async fn test_dashboard_overview_endpoint() {
    let (app, _db_pool, jwt) = build_app().await;

    // GET /api/dashboard/overview
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/overview")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/overview should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let overview: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        overview.is_object(),
        "Response should be an overview object"
    );
}

#[tokio::test]
async fn test_dashboard_request_history_endpoint() {
    let (app, _db_pool, jwt) = build_app().await;

    // GET /api/dashboard/request-history
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/request-history")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/request-history should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let history: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        history.is_array(),
        "Response should be an array of request history"
    );
}

#[tokio::test]
async fn test_dashboard_endpoints_include_endpoint_type() {
    let (app, _db_pool, jwt) = build_app().await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": []
        })))
        .mount(&mock)
        .await;

    let create_body = json!({
        "name": "Test Endpoint",
        "base_url": mock.uri()
    });

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", jwt))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        create_response.status(),
        StatusCode::CREATED,
        "POST /api/endpoints should return CREATED"
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/endpoints")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/endpoints should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let endpoints: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let endpoints = endpoints.as_array().expect("response should be an array");
    assert!(
        endpoints.iter().any(|endpoint| {
            let et = endpoint["endpoint_type"].as_str().unwrap_or("");
            [
                "xllm",
                "ollama",
                "vllm",
                "lm_studio",
                "llamacpp",
                "openai_compatible",
            ]
            .contains(&et)
        }),
        "endpoint_type should be included in dashboard endpoints"
    );
}

#[tokio::test]
async fn test_cloud_metrics_endpoint() {
    let (app, _db_pool, admin_key) = build_app().await;

    // GET /api/metrics/cloud
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/metrics/cloud")
                .header("authorization", format!("Bearer {}", admin_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/metrics/cloud should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics_text = String::from_utf8(body.to_vec()).unwrap();

    // Prometheus形式のメトリクスが含まれることを確認
    // メトリクスが空の場合もあるので、形式チェックのみ
    assert!(
        metrics_text.is_empty() || metrics_text.contains("# ") || metrics_text.contains("_"),
        "Response should be in Prometheus text format"
    );
}

#[tokio::test]
async fn test_models_loaded_endpoint_is_removed() {
    let (app, _db_pool, admin_key) = build_app().await;

    // GET /api/models/loaded
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/loaded")
                .header("authorization", format!("Bearer {}", admin_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        matches!(
            response.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "/api/models/loaded should be removed (got {})",
        response.status()
    );
}
