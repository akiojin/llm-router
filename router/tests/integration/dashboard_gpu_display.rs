//! Integration Test: ダッシュボードAPIでのGPU情報表示
//!
//! ダッシュボードエンドポイントがノードのGPU情報（モデル名・枚数）を返すことを検証する。

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llm_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use tower::ServiceExt;

async fn build_router() -> Router {
    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = DownloadTaskManager::new();
    let db_pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run migrations");
    let jwt_secret = "test-secret".to_string();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };
    api::create_router(state)
}

#[tokio::test]
async fn dashboard_agents_include_gpu_devices() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let router = build_router().await;

    let payload = json!({
        "machine_name": "dashboard-gpu",
        "ip_address": "10.1.0.50",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "Apple M3 Max", "count": 1}
        ]
    });

    let register_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::CREATED);

    let response = router
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
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        payload.is_array(),
        "expected array response but got {payload:?}"
    );
    let agent = payload
        .as_array()
        .and_then(|list| list.first())
        .cloned()
        .expect("agent entry must exist");
    assert_eq!(agent["machine_name"], "dashboard-gpu");
    assert!(
        agent["gpu_devices"].is_array(),
        "gpu_devices should be present in dashboard payload"
    );
    let devices = agent["gpu_devices"].as_array().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["model"], "Apple M3 Max");
    assert_eq!(devices[0]["count"], 1);
}
