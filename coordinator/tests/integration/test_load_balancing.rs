//! Integration Test: ロードバランシング
//!
//! 複数エージェントへのリクエスト分散と負荷ベース選択の検証

use ollama_coordinator_coordinator::{
    balancer::{LoadManager, MetricsUpdate, RequestOutcome},
    registry::AgentRegistry,
};
use ollama_coordinator_common::protocol::RegisterRequest;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

#[tokio::test]
async fn test_round_robin_load_balancing() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());

    let mut agent_ids = Vec::new();
    for idx in 0..3 {
        let req = RegisterRequest {
            machine_name: format!("round-robin-agent-{}", idx),
            ip_address: format!("192.168.1.{}", 200 + idx)
                .parse::<IpAddr>()
                .unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        let response = registry.register(req).await.unwrap();
        agent_ids.push(response.agent_id);
    }

    let mut distribution: HashMap<_, usize> = HashMap::new();

    for _ in 0..9 {
        let agent = load_manager.select_agent().await.unwrap();
        let entry = distribution.entry(agent.id).or_default();
        *entry += 1;

        load_manager.begin_request(agent.id).await.unwrap();
        load_manager
            .finish_request(
                agent.id,
                RequestOutcome::Success,
                Duration::from_millis(50),
            )
            .await
            .unwrap();
    }

    for agent_id in agent_ids {
        assert_eq!(distribution.get(&agent_id).copied().unwrap_or_default(), 3);
    }
}

#[tokio::test]
async fn test_load_based_balancing_favors_low_cpu_agents() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());

    let high_cpu_agent = registry
        .register(RegisterRequest {
            machine_name: "high-cpu-agent".to_string(),
            ip_address: "192.168.2.10".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    let low_cpu_agent = registry
        .register(RegisterRequest {
            machine_name: "low-cpu-agent".to_string(),
            ip_address: "192.168.2.11".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    // 高負荷エージェントはCPU 95%、低負荷エージェントはCPU 10%
    load_manager
        .record_metrics(MetricsUpdate {
            agent_id: high_cpu_agent,
            cpu_usage: 95.0,
            memory_usage: 40.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            active_requests: 2,
            average_response_time_ms: None,
        })
        .await
        .unwrap();
    load_manager
        .record_metrics(MetricsUpdate {
            agent_id: low_cpu_agent,
            cpu_usage: 10.0,
            memory_usage: 30.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            active_requests: 0,
            average_response_time_ms: None,
        })
        .await
        .unwrap();

    for _ in 0..10 {
        let selected = load_manager.select_agent().await.unwrap();
        assert_eq!(
            selected.id, low_cpu_agent,
            "Load-based balancer should prefer low CPU agent"
        );

        load_manager.begin_request(selected.id).await.unwrap();
        load_manager
            .finish_request(
                selected.id,
                RequestOutcome::Success,
                Duration::from_millis(25),
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_load_based_balancing_prefers_lower_latency() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());

    let slow_agent = registry
        .register(RegisterRequest {
            machine_name: "slow-agent".to_string(),
            ip_address: "192.168.3.10".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    let fast_agent = registry
        .register(RegisterRequest {
            machine_name: "fast-agent".to_string(),
            ip_address: "192.168.3.11".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    load_manager
        .record_metrics(MetricsUpdate {
            agent_id: slow_agent,
            cpu_usage: 50.0,
            memory_usage: 40.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            active_requests: 1,
            average_response_time_ms: Some(250.0),
        })
        .await
        .unwrap();
    load_manager
        .record_metrics(MetricsUpdate {
            agent_id: fast_agent,
            cpu_usage: 50.0,
            memory_usage: 40.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            active_requests: 1,
            average_response_time_ms: Some(120.0),
        })
        .await
        .unwrap();

    let selected = load_manager.select_agent().await.unwrap();
    assert_eq!(selected.id, fast_agent);
}
