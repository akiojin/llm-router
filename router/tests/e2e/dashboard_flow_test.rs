//! ダッシュボードフローE2Eテスト
//!
//! ダッシュボードAPI（/api/dashboard/*）のE2Eテスト

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llm_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
use std::net::IpAddr;
use tower::ServiceExt;

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLM_ROUTER_DATA_DIR", &temp_dir);

    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = DownloadTaskManager::new();
    let db_pool = support::router::create_test_db_pool().await;
    let jwt_secret = support::router::test_jwt_secret();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool: db_pool.clone(),
        jwt_secret,
    };

    (api::create_router(state), db_pool)
}

#[tokio::test]
async fn test_dashboard_nodes_endpoint() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/dashboard/nodes
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/dashboard/nodes should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let nodes: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        nodes.is_array(),
        "Response should be an array of dashboard nodes"
    );
}

#[tokio::test]
async fn test_dashboard_stats_endpoint() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/dashboard/stats
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/stats")
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
}

#[tokio::test]
async fn test_dashboard_overview_endpoint() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/dashboard/overview
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/overview")
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
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/dashboard/request-history
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/request-history")
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
async fn test_dashboard_nodes_with_registered_node() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // ノードを登録
    let register_request = RegisterRequest {
        machine_name: "dashboard-test-node".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 150)),
        runtime_version: "0.1.0".to_string(),
        runtime_port: 11434,
        gpu_available: true,
        gpu_devices: vec![GpuDeviceInfo {
            model: "RTX 4090".to_string(),
            count: 1,
            memory: Some(24576),
        }],
        gpu_count: Some(1),
        gpu_model: Some("RTX 4090".to_string()),
    };

    let _register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // ダッシュボードノード一覧を取得
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let nodes: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let nodes_array = nodes.as_array().unwrap();
    assert!(
        !nodes_array.is_empty(),
        "Dashboard should show registered nodes"
    );
}

#[tokio::test]
async fn test_cloud_metrics_endpoint() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /metrics/cloud
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics/cloud")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /metrics/cloud should return OK"
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
async fn test_models_loaded_endpoint() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/models/loaded
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/loaded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/models/loaded should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let models: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // get_loaded_models returns Vec<LoadedModelSummary> directly (an array)
    assert!(
        models.is_array(),
        "Response must be an array of loaded models"
    );
}
