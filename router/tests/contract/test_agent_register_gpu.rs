//! Contract Test: GPU必須ノード登録
//!
//! GPU情報を含むノードのみが登録され、レスポンスへGPU情報が反映されることを検証する。

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use or_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use tower::ServiceExt;

async fn build_app() -> Router {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("OLLAMA_ROUTER_DATA_DIR", &temp_dir);

    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(or_router::db::request_history::RequestHistoryStorage::new().unwrap());
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
async fn register_gpu_agent_success() {
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let app = build_app().await;

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
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let list_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(list_response.status(), StatusCode::OK);
    let body = to_bytes(list_response.into_body(), 1024).await.unwrap();
    let nodes: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(nodes.is_array(), "expected array response");
    let first = nodes
        .as_array()
        .and_then(|list| list.first())
        .cloned()
        .expect("node must exist");
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
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let app = build_app().await;

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
                .uri("/api/nodes")
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
