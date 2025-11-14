//! Integration Test: ダッシュボードAPIでのGPU情報表示
//!
//! ダッシュボードエンドポイントがエージェントのGPU情報（モデル名・枚数）を返すことを検証する。

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use tower::ServiceExt;

fn build_router() -> Router {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = DownloadTaskManager::new();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    };
    api::create_router(state)
}

#[tokio::test]
async fn dashboard_agents_include_gpu_devices() {
    let router = build_router();

    let payload = json!({
        "machine_name": "dashboard-gpu",
        "ip_address": "10.1.0.50",
        "ollama_version": "0.1.42",
        "ollama_port": 11434,
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
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::OK);

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/agents")
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
