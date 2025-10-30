//! Integration Test: Heartbeat Sending
//!
//! エージェントのハートビート送信機能をテスト

use ollama_coordinator_agent::{client::CoordinatorClient, metrics::MetricsCollector};
use ollama_coordinator_common::protocol::{HealthCheckRequest, RegisterRequest};
use std::net::IpAddr;

#[tokio::test]
#[ignore] // Coordinatorサーバーが必要
async fn test_heartbeat_sending_after_registration() {
    // Arrange: エージェント登録
    let coordinator_url = std::env::var("TEST_COORDINATOR_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let mut client = CoordinatorClient::new(coordinator_url);

    let register_req = RegisterRequest {
        machine_name: "heartbeat-test-machine".to_string(),
        ip_address: "192.168.1.102".parse::<IpAddr>().unwrap(),
        ollama_version: "0.1.0".to_string(),
        ollama_port: 11434,
    };

    let register_response = client.register(register_req).await.unwrap();
    let agent_id = register_response.agent_id;

    // Act: ハートビート送信
    let heartbeat_req = HealthCheckRequest {
        agent_id,
        cpu_usage: 45.5,
        memory_usage: 60.2,
        active_requests: 0,
    };

    let heartbeat_result = client.send_heartbeat(heartbeat_req).await;

    // Assert: ハートビート送信成功
    assert!(
        heartbeat_result.is_ok(),
        "Heartbeat should be sent successfully: {:?}",
        heartbeat_result
    );
}

#[tokio::test]
#[ignore] // Coordinatorサーバーが必要
async fn test_heartbeat_with_real_metrics() {
    // Arrange: エージェント登録
    let coordinator_url = std::env::var("TEST_COORDINATOR_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let mut client = CoordinatorClient::new(coordinator_url);

    let register_req = RegisterRequest {
        machine_name: "metrics-test-machine".to_string(),
        ip_address: "192.168.1.103".parse().unwrap(),
        ollama_version: "0.1.0".to_string(),
        ollama_port: 11434,
    };

    let register_response = client.register(register_req).await.unwrap();
    let agent_id = register_response.agent_id;

    // Act: 実際のメトリクスを収集
    let mut metrics_collector = MetricsCollector::new();
    let (cpu_usage, memory_usage) = metrics_collector.collect_metrics().unwrap();

    println!(
        "Collected metrics - CPU: {}%, Memory: {}%",
        cpu_usage, memory_usage
    );

    // Act: ハートビート送信
    let heartbeat_req = HealthCheckRequest {
        agent_id,
        cpu_usage,
        memory_usage,
        active_requests: 0,
    };

    let heartbeat_result = client.send_heartbeat(heartbeat_req).await;

    // Assert: ハートビート送信成功
    assert!(
        heartbeat_result.is_ok(),
        "Heartbeat with real metrics should succeed"
    );
}

#[tokio::test]
#[ignore] // Coordinatorサーバーが必要
async fn test_heartbeat_unregistered_agent() {
    // Arrange: 未登録のエージェントID
    let coordinator_url = std::env::var("TEST_COORDINATOR_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let client = CoordinatorClient::new(coordinator_url);

    let fake_agent_id = uuid::Uuid::new_v4();

    let heartbeat_req = HealthCheckRequest {
        agent_id: fake_agent_id,
        cpu_usage: 50.0,
        memory_usage: 50.0,
        active_requests: 0,
    };

    // Act: 未登録エージェントでハートビート送信
    let result = client.send_heartbeat(heartbeat_req).await;

    // Assert: エラーが返されること（404 Not Found）
    assert!(
        result.is_err(),
        "Heartbeat from unregistered agent should fail"
    );
    println!("Expected error: {:?}", result.unwrap_err());
}

#[tokio::test]
#[ignore] // Coordinatorサーバーが必要
async fn test_multiple_heartbeats() {
    // Arrange: エージェント登録
    let coordinator_url = std::env::var("TEST_COORDINATOR_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let mut client = CoordinatorClient::new(coordinator_url);

    let register_req = RegisterRequest {
        machine_name: "multi-heartbeat-machine".to_string(),
        ip_address: "192.168.1.104".parse().unwrap(),
        ollama_version: "0.1.0".to_string(),
        ollama_port: 11434,
    };

    let register_response = client.register(register_req).await.unwrap();
    let agent_id = register_response.agent_id;

    // Act: 複数回ハートビート送信
    for i in 0..5 {
        let heartbeat_req = HealthCheckRequest {
            agent_id,
            cpu_usage: 40.0 + i as f32,
            memory_usage: 55.0 + i as f32,
            active_requests: i,
        };

        let result = client.send_heartbeat(heartbeat_req).await;
        assert!(result.is_ok(), "Heartbeat #{} should succeed", i + 1);

        // 少し待つ
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!("Successfully sent 5 heartbeats");
}
