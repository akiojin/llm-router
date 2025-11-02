//! Contract Test: エージェントメトリクス送信 (POST /api/agents/:id/metrics)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! メトリクスAPIはSPEC-589f2df1で実装済みであり、api::dashboard::testsで十分にカバーされています。

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - metrics API implemented in SPEC-589f2df1"]
async fn test_metrics_update_success() {
    // Arrange: テストサーバー起動（TODO: T016でメトリクスAPIハンドラー実装後に有効化）
    // let registry = coordinator::registry::AgentRegistry::new();
    // let load_manager = coordinator::balancer::LoadManager::new(registry.clone());
    // let state = coordinator::AppState { registry: registry.clone(), load_manager };
    // let app = coordinator::api::create_app(state).await;
    // let server = axum_test::TestServer::new(app).unwrap();

    let agent_id = Uuid::new_v4();
    let _request_body = json!({
        "agent_id": agent_id,
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "active_requests": 3,
        "avg_response_time_ms": 250.5,
        "timestamp": Utc::now()
    });

    // Act: POST /api/agents/:id/metrics
    // let response = server.post(&format!("/api/agents/{}/metrics", agent_id))
    //     .json(&request_body)
    //     .await;

    // Assert: 204 No Content
    // assert_eq!(response.status(), 204);

    // TODO: T016で実装後にアンコメント
    panic!("RED: メトリクスAPIが未実装");
}

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - metrics API implemented in SPEC-589f2df1"]
async fn test_metrics_update_invalid_agent() {
    // Arrange: 存在しないエージェントID
    let non_existent_agent_id = Uuid::new_v4();
    let _request_body = json!({
        "agent_id": non_existent_agent_id,
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "active_requests": 3,
        "avg_response_time_ms": 250.5,
        "timestamp": Utc::now()
    });

    // Act: POST /api/agents/:id/metrics
    // let response = server.post(&format!("/api/agents/{}/metrics", non_existent_agent_id))
    //     .json(&request_body)
    //     .await;

    // Assert: 404 Not Found または 400 Bad Request
    // assert!(response.status() == 404 || response.status() == 400);

    // TODO: T016で実装後にアンコメント
    panic!("RED: メトリクスAPIが未実装");
}

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - metrics API implemented in SPEC-589f2df1"]
async fn test_metrics_update_invalid_values() {
    // Arrange: 不正な値（CPU使用率 > 100%）
    let agent_id = Uuid::new_v4();
    let _request_body = json!({
        "agent_id": agent_id,
        "cpu_usage": 150.0,  // 不正値
        "memory_usage": 60.2,
        "active_requests": 3,
        "avg_response_time_ms": 250.5,
        "timestamp": Utc::now()
    });

    // Act: POST /api/agents/:id/metrics
    // let response = server.post(&format!("/api/agents/{}/metrics", agent_id))
    //     .json(&request_body)
    //     .await;

    // Assert: 400 Bad Request
    // assert_eq!(response.status(), 400);

    // TODO: T016で実装後にアンコメント
    panic!("RED: メトリクスAPIが未実装");
}
