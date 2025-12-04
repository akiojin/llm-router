//! 自動モデル配布統合テスト
//!
//! TDD RED: エージェント登録時のGPUメモリに応じた自動モデル配布

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llm_router::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use serial_test::serial;
use tower::ServiceExt;

use crate::support;

async fn build_app() -> Router {
    // AUTH_DISABLED=trueで認証を無効化
    std::env::set_var("AUTH_DISABLED", "true");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        llm_router::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = llm_router::tasks::DownloadTaskManager::new();
    let db_pool = support::router::create_test_db_pool().await;
    let jwt_secret = support::router::test_jwt_secret();

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

/// T009: 16GB GPU搭載エージェント登録時に gpt-oss:20b が自動配布される
#[tokio::test]
#[ignore = "RED phase: waiting for auto_distributed_model implementation"]
#[serial]
async fn test_auto_download_on_registration_16gb_gpu() {
    let app = build_app().await;

    // 16GB GPUを持つエージェントを登録
    let register_payload = json!({
        "machine_name": "high-end-gpu-server",
        "ip_address": "192.168.1.100",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 4090", "count": 1, "memory": 16_000_000_000u64}
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // レスポンスに自動配布されたモデル情報が含まれることを検証
    assert!(
        agent.get("auto_distributed_model").is_some(),
        "Response must include auto_distributed_model field"
    );
    let auto_model = agent["auto_distributed_model"]
        .as_str()
        .expect("auto_distributed_model must be a string");
    assert_eq!(
        auto_model, "gpt-oss:20b",
        "16GB GPU should auto-distribute gpt-oss:20b"
    );
}

/// T010: 8GB GPU搭載エージェント登録時に gpt-oss:7b が自動配布される
#[tokio::test]
#[ignore = "RED phase: waiting for auto_distributed_model implementation"]
#[serial]
async fn test_auto_download_on_registration_8gb_gpu() {
    let app = build_app().await;

    // 8GB GPUを持つエージェントを登録
    let register_payload = json!({
        "machine_name": "mid-range-gpu-server",
        "ip_address": "192.168.1.101",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 3060", "count": 1, "memory": 8_000_000_000u64}
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let auto_model = agent["auto_distributed_model"]
        .as_str()
        .expect("auto_distributed_model must be present");
    assert_eq!(
        auto_model, "gpt-oss:7b",
        "8GB GPU should auto-distribute gpt-oss:7b"
    );
}

/// T011: 4.5GB GPU搭載エージェント登録時に gpt-oss:3b が自動配布される
#[tokio::test]
#[ignore = "RED phase: waiting for auto_distributed_model implementation"]
#[serial]
async fn test_auto_download_on_registration_4_5gb_gpu() {
    let app = build_app().await;

    // 4.5GB GPUを持つエージェントを登録
    let register_payload = json!({
        "machine_name": "entry-gpu-server",
        "ip_address": "192.168.1.102",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA GTX 1650", "count": 1, "memory": 4_500_000_000u64}
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let auto_model = agent["auto_distributed_model"]
        .as_str()
        .expect("auto_distributed_model must be present");
    assert_eq!(
        auto_model, "gpt-oss:3b",
        "4.5GB GPU should auto-distribute gpt-oss:3b"
    );
}

/// T012: 小容量GPU搭載エージェント登録時に gpt-oss:1b が自動配布される
#[tokio::test]
#[ignore = "RED phase: waiting for auto_distributed_model implementation"]
#[serial]
async fn test_auto_download_on_registration_small_gpu() {
    let app = build_app().await;

    // 2GB GPUを持つエージェントを登録
    let register_payload = json!({
        "machine_name": "small-gpu-server",
        "ip_address": "192.168.1.103",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA GT 1030", "count": 1, "memory": 2_000_000_000u64}
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let auto_model = agent["auto_distributed_model"]
        .as_str()
        .expect("auto_distributed_model must be present");
    assert_eq!(
        auto_model, "gpt-oss:1b",
        "Small GPU (<4.5GB) should auto-distribute gpt-oss:1b"
    );
}

/// T013: ダウンロード進捗が表示される（タスクIDを返す）
#[tokio::test]
#[ignore = "RED phase: waiting for download_task_id implementation"]
#[serial]
async fn test_progress_display_during_download() {
    let app = build_app().await;

    // エージェントを登録
    let register_payload = json!({
        "machine_name": "progress-test-server",
        "ip_address": "192.168.1.104",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 4090", "count": 1, "memory": 16_000_000_000u64}
        ]
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // タスクIDが含まれることを検証
    assert!(
        agent.get("download_task_id").is_some(),
        "Response must include download_task_id for progress tracking"
    );

    let task_id = agent["download_task_id"]
        .as_str()
        .expect("download_task_id must be a string");

    // タスク進捗を取得できることを検証
    let progress_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/tasks/{}", task_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        progress_response.status(),
        StatusCode::OK,
        "Progress endpoint should return 200 OK"
    );

    let body = to_bytes(progress_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let task: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // タスク情報の検証
    assert!(task.get("status").is_some(), "Task must have status");
    assert!(task.get("progress").is_some(), "Task must have progress");
    assert!(
        task.get("model_name").is_some(),
        "Task must have model_name"
    );
}
