//! 手動モデル配布統合テスト
//!
//! TDD RED: ダッシュボードからの手動モデル配布機能

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use serial_test::serial;
use tower::ServiceExt;

use crate::support;
use uuid::Uuid;

async fn build_app() -> Router {
    // AUTH_DISABLED=trueで認証を無効化
    std::env::set_var("AUTH_DISABLED", "true");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = ollama_coordinator_coordinator::tasks::DownloadTaskManager::new();
    let db_pool = support::coordinator::create_test_db_pool().await;
    let jwt_secret = support::coordinator::test_jwt_secret();

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

/// T014: 特定のエージェントへ手動配布
#[tokio::test]
#[serial]
async fn test_manual_distribution_to_specific_agent() {
    let app = build_app().await;
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // テスト用エージェントを登録
    let register_payload = json!({
        "machine_name": "target-agent",
        "ip_address": "192.168.1.200",
        "ollama_version": "0.1.42",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 3090", "count": 1}
        ]
    });

    let register_response = app
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

    assert_eq!(register_response.status(), StatusCode::CREATED);

    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("Agent must have 'agent_id' field");

    // 特定のエージェントにモデルを配布
    let distribute_payload = json!({
        "model_name": "llama3.2",
        "target": "specific",
        "agent_ids": [agent_id]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&distribute_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // 202 ACCEPTED を期待
    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "Manual distribution should return 202 ACCEPTED"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // タスクIDが1つ返されることを検証
    assert!(
        result.get("task_ids").is_some(),
        "Response must have task_ids"
    );
    let task_ids = result["task_ids"]
        .as_array()
        .expect("task_ids must be an array");
    assert_eq!(task_ids.len(), 1, "Should have exactly 1 task for 1 agent");
}

/// T015: 全エージェントへ一括配布
#[tokio::test]
#[ignore = "RED phase: waiting for models/distribute endpoint implementation"]
#[serial]
async fn test_bulk_distribution_to_all_agents() {
    let app = build_app().await;
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // 複数のエージェントを登録
    for i in 0..3 {
        let register_payload = json!({
            "machine_name": format!("bulk-agent-{}", i),
            "ip_address": format!("192.168.1.{}", 210 + i),
            "ollama_version": "0.1.42",
            "ollama_port": 11434,
            "gpu_available": true,
            "gpu_devices": [
                {"model": "NVIDIA RTX 3090", "count": 1}
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
    }

    // 全エージェントに一括配布
    let distribute_payload = json!({
        "model_name": "deepseek-r1",
        "target": "all",
        "agent_ids": []
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&distribute_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // 3つのタスクIDが返されることを検証
    let task_ids = result["task_ids"]
        .as_array()
        .expect("task_ids must be an array");
    assert_eq!(task_ids.len(), 3, "Should have 3 tasks for 3 agents");

    // すべてのタスクIDがUUIDであることを検証
    for task_id in task_ids {
        let task_id_str = task_id.as_str().expect("task_id must be a string");
        Uuid::parse_str(task_id_str).expect("task_id must be a valid UUID");
    }
}

/// T016: 複数エージェントの進捗追跡
#[tokio::test]
#[serial]
async fn test_progress_tracking_multiple_agents() {
    let app = build_app().await;
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // 2つのエージェントを登録
    let mut agent_ids = Vec::new();
    for i in 0..2 {
        let register_payload = json!({
            "machine_name": format!("progress-agent-{}", i),
            "ip_address": format!("192.168.1.{}", 220 + i),
            "ollama_version": "0.1.42",
            "ollama_port": 11434,
            "gpu_available": true,
            "gpu_devices": [
                {"model": "NVIDIA RTX 3090", "count": 1}
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        agent_ids.push(
            agent["agent_id"]
                .as_str()
                .expect("Agent must have 'agent_id'")
                .to_string(),
        );
    }

    // 複数エージェントに配布
    let distribute_payload = json!({
        "model_name": "gpt-oss:7b",
        "target": "specific",
        "agent_ids": agent_ids
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&distribute_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let task_ids = result["task_ids"]
        .as_array()
        .expect("task_ids must be an array");

    // 各タスクの進捗を取得できることを検証
    for task_id in task_ids {
        let task_id_str = task_id.as_str().expect("task_id must be a string");

        let progress_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/tasks/{}", task_id_str))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            progress_response.status(),
            StatusCode::OK,
            "Each task should have accessible progress"
        );

        let body = to_bytes(progress_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let task: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(task.get("status").is_some());
        assert!(task.get("progress").is_some());
    }
}

/// T017: オフラインエージェントへの配布エラーハンドリング
#[tokio::test]
#[serial]
async fn test_offline_agent_error_handling() {
    let app = build_app().await;
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::env::remove_var("AUTH_DISABLED");
        }
    }
    let _cleanup = Cleanup;

    // 存在しないエージェントIDを使って配布を試みる
    let fake_agent_id = Uuid::new_v4().to_string();

    let distribute_payload = json!({
        "model_name": "gpt-oss:3b",
        "target": "specific",
        "agent_ids": [fake_agent_id]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&distribute_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // エラーレスポンスを期待（400 Bad Request or 404 Not Found）
    assert!(
        response.status() == StatusCode::BAD_REQUEST || response.status() == StatusCode::NOT_FOUND,
        "Distributing to non-existent agent should return error"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let error: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // エラーメッセージが含まれることを検証
    assert!(
        error.get("error").is_some() || error.get("message").is_some(),
        "Error response must include error or message field"
    );
}
