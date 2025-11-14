//! Contract Test: GPU必須エージェント登録
//!
//! GPU情報を含むエージェントのみが登録され、レスポンスへGPU情報が反映されることを検証する。

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

fn build_app() -> Router {
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
async fn register_gpu_agent_success() {
    let app = build_app();

    let payload = json!({
        "machine_name": "gpu-node",
        "ip_address": "10.0.0.10",
        "ollama_version": "0.1.42",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 4090", "count": 2}
        ]
    });

    let response = app
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

    assert_eq!(response.status(), StatusCode::OK);

    let list_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(list_response.status(), StatusCode::OK);
    let body = to_bytes(list_response.into_body(), 1024).await.unwrap();
    let agents: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(agents.is_array(), "expected array response");
    let first = agents
        .as_array()
        .and_then(|list| list.first())
        .cloned()
        .expect("agent must exist");
    assert_eq!(first["machine_name"], "gpu-node");
    assert_eq!(first["gpu_available"], true);
    assert!(
        first["gpu_devices"].is_array(),
        "gpu_devices should be present"
    );
    let gpu_devices = first["gpu_devices"].as_array().unwrap();
    assert_eq!(gpu_devices.len(), 1);
    assert_eq!(gpu_devices[0]["model"], "NVIDIA RTX 4090");
    assert_eq!(gpu_devices[0]["count"], 2);
}

#[tokio::test]
async fn register_gpu_agent_missing_devices_is_rejected() {
    let app = build_app();

    let payload = json!({
        "machine_name": "cpu-only",
        "ip_address": "10.0.0.20",
        "ollama_version": "0.1.42",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": []
    });

    let response = app
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

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        error["error"],
        "検証エラー: GPU hardware is required for agent registration. No GPU devices detected in gpu_devices array."
    );
}
