//! Integration Test: メトリクス収集
//!
//! ⚠️ このテストはTDD RED状態の統合テストです。
//! メトリクス機能はSPEC-589f2df1で実装済みであり、
//! balancer::testsとapi::dashboard::testsで十分にカバーされています。

// use chrono::Utc;
// use serde_json::json;
use uuid::Uuid;

#[tokio::test]
#[ignore = "TDD RED phase - metrics implemented in SPEC-589f2df1, covered by unit tests"]
async fn test_metrics_collection_and_storage() {
    // Arrange: Coordinatorサーバー起動、エージェント登録
    // let registry = coordinator::registry::AgentRegistry::new();
    // let load_manager = coordinator::balancer::LoadManager::new(registry.clone());
    // let state = coordinator::AppState { registry: registry.clone(), load_manager: load_manager.clone() };
    // let app = coordinator::api::create_app(state).await;
    // let server = axum_test::TestServer::new(app).unwrap();

    // エージェント登録
    // let agent_req = json!({
    //     "machine_name": "metrics-test-agent",
    //     "ip_address": "192.168.100.50",
    //     "ollama_version": "0.1.0",
    //     "ollama_port": 11434,
    //     "gpu_available": true,
    //     "gpu_devices": [{
    //         "model": "Test GPU",
    //         "count": 1
    //     }],
    //     "gpu_count": 1,
    //     "gpu_model": "Test GPU"
    // });
    // let agent_response = server.post("/api/agents/register")
    //     .json(&agent_req)
    //     .await;
    // let agent_id: Uuid = agent_response.json().get("agent_id").as_str().unwrap().parse().unwrap();

    let _agent_id = Uuid::new_v4(); // テスト用プレースホルダー

    // Act: メトリクス送信
    // let metrics_data = json!({
    //     "agent_id": agent_id,
    //     "cpu_usage": 45.5,
    //     "memory_usage": 60.2,
    //     "active_requests": 3,
    //     "avg_response_time_ms": 250.5,
    //     "timestamp": Utc::now()
    // });
    // let metrics_response = server.post(&format!("/api/agents/{}/metrics", agent_id))
    //     .json(&metrics_data)
    //     .await;

    // Assert: 204 No Content
    // assert_eq!(metrics_response.status(), 204);

    // Assert: メトリクスがメモリに保存されている
    // let stored_metrics = load_manager.get_metrics(agent_id).await.unwrap();
    // assert_eq!(stored_metrics.cpu_usage, 45.5);
    // assert_eq!(stored_metrics.memory_usage, 60.2);
    // assert_eq!(stored_metrics.active_requests, 3);
    // assert_eq!(stored_metrics.avg_response_time_ms, Some(250.5));

    // TODO: T016でメトリクスAPIハンドラー実装後にアンコメント
    panic!("RED: メトリクス収集APIが未実装");
}

#[tokio::test]
#[ignore = "TDD RED phase - metrics implemented in SPEC-589f2df1, covered by unit tests"]
async fn test_metrics_update_existing_data() {
    // Arrange: Coordinatorサーバー起動、エージェント登録、初回メトリクス送信
    // let registry = coordinator::registry::AgentRegistry::new();
    // let load_manager = coordinator::balancer::LoadManager::new(registry.clone());
    // let state = coordinator::AppState { registry: registry.clone(), load_manager: load_manager.clone() };
    // let app = coordinator::api::create_app(state).await;
    // let server = axum_test::TestServer::new(app).unwrap();

    let _agent_id = Uuid::new_v4();

    // 初回メトリクス送信
    // let initial_metrics = json!({
    //     "agent_id": agent_id,
    //     "cpu_usage": 30.0,
    //     "memory_usage": 40.0,
    //     "active_requests": 1,
    //     "avg_response_time_ms": 100.0,
    //     "timestamp": Utc::now()
    // });
    // server.post(&format!("/api/agents/{}/metrics", agent_id))
    //     .json(&initial_metrics)
    //     .await;

    // Act: 更新メトリクス送信
    // let updated_metrics = json!({
    //     "agent_id": agent_id,
    //     "cpu_usage": 75.0,
    //     "memory_usage": 80.0,
    //     "active_requests": 5,
    //     "avg_response_time_ms": 300.0,
    //     "timestamp": Utc::now()
    // });
    // let response = server.post(&format!("/api/agents/{}/metrics", agent_id))
    //     .json(&updated_metrics)
    //     .await;

    // Assert: 204 No Content
    // assert_eq!(response.status(), 204);

    // Assert: メトリクスが更新されている（初回データは上書きされる）
    // let stored_metrics = load_manager.get_metrics(agent_id).await.unwrap();
    // assert_eq!(stored_metrics.cpu_usage, 75.0);
    // assert_eq!(stored_metrics.memory_usage, 80.0);
    // assert_eq!(stored_metrics.active_requests, 5);
    // assert_eq!(stored_metrics.avg_response_time_ms, Some(300.0));

    // TODO: T016でメトリクスAPIハンドラー実装後にアンコメント
    panic!("RED: メトリクス更新APIが未実装");
}

#[tokio::test]
#[ignore = "TDD RED phase - metrics implemented in SPEC-589f2df1, covered by unit tests"]
async fn test_metrics_for_nonexistent_agent_returns_error() {
    // Arrange: Coordinatorサーバー起動（エージェント未登録）
    // let registry = coordinator::registry::AgentRegistry::new();
    // let load_manager = coordinator::balancer::LoadManager::new(registry.clone());
    // let state = coordinator::AppState { registry, load_manager };
    // let app = coordinator::api::create_app(state).await;
    // let server = axum_test::TestServer::new(app).unwrap();

    let _nonexistent_agent_id = Uuid::new_v4();

    // Act: 存在しないエージェントIDでメトリクス送信
    // let metrics_data = json!({
    //     "agent_id": nonexistent_agent_id,
    //     "cpu_usage": 45.5,
    //     "memory_usage": 60.2,
    //     "active_requests": 3,
    //     "avg_response_time_ms": 250.5,
    //     "timestamp": Utc::now()
    // });
    // let response = server.post(&format!("/api/agents/{}/metrics", nonexistent_agent_id))
    //     .json(&metrics_data)
    //     .await;

    // Assert: 404 Not Found または 400 Bad Request
    // assert!(response.status() == 404 || response.status() == 400);

    // TODO: T016でメトリクスAPIハンドラー実装後にアンコメント
    panic!("RED: メトリクスAPI未実装（存在しないエージェントケース）");
}
