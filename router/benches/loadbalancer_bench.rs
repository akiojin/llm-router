//! LoadManager パフォーマンスベンチマーク
//!
//! select_agent_by_metrics() の実行時間を測定し、
//! 1000ノードで < 10ms の目標を検証する。

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use llm_router::{
    balancer::{LoadManager, MetricsUpdate},
    registry::NodeRegistry,
};
use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
use std::net::{IpAddr, Ipv4Addr};
use tokio::runtime::Runtime;

fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
    vec![GpuDeviceInfo {
        model: "Test GPU".to_string(),
        count: 1,
        memory: None,
    }]
}

async fn setup_agents(count: usize) -> LoadManager {
    let registry = NodeRegistry::new();
    let manager = LoadManager::new(registry.clone());

    for i in 0..count {
        let node_id = registry
            .register(RegisterRequest {
                machine_name: format!("agent-{}", i),
                ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8)),
                runtime_version: "0.1.0".to_string(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .node_id;

        // 各ノードに異なる負荷のメトリクスを記録
        manager
            .record_metrics(MetricsUpdate {
                node_id,
                cpu_usage: (i % 80) as f32,
                memory_usage: ((i * 2) % 90) as f32,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: (i % 5) as u32,
                average_response_time_ms: Some(100.0 + (i % 200) as f32),
                initializing: false,
                ready_models: None,
            })
            .await
            .unwrap();
    }

    manager
}

fn bench_select_agent_by_metrics(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("select_agent_by_metrics");

    for agent_count in [10, 50, 100, 500, 1000].iter() {
        let manager = rt.block_on(setup_agents(*agent_count));

        group.bench_with_input(
            BenchmarkId::from_parameter(agent_count),
            agent_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    black_box(manager.select_agent_by_metrics().await.unwrap());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_select_agent_by_metrics);
criterion_main!(benches);
