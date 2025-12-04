//! ヘルスチェックモニター
//!
//! 定期的にノードのタイムアウトを検知し、オフライン判定

use crate::registry::NodeRegistry;
use chrono::Utc;
use llm_router_common::types::NodeStatus;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

/// ヘルスモニター
pub struct HealthMonitor {
    registry: NodeRegistry,
    check_interval_secs: u64,
    timeout_secs: u64,
}

impl HealthMonitor {
    /// 新しいヘルスモニターを作成
    pub fn new(registry: NodeRegistry, check_interval_secs: u64, timeout_secs: u64) -> Self {
        Self {
            registry,
            check_interval_secs,
            timeout_secs,
        }
    }

    /// バックグラウンドで監視を開始
    pub fn start(self) {
        tokio::spawn(async move {
            self.monitor_loop().await;
        });
    }

    /// 監視ループ
    async fn monitor_loop(&self) {
        let mut timer = interval(Duration::from_secs(self.check_interval_secs));

        info!(
            "Health monitor started: check_interval={}s, timeout={}s",
            self.check_interval_secs, self.timeout_secs
        );

        loop {
            timer.tick().await;

            if let Err(e) = self.check_agent_health().await {
                error!("Health check error: {}", e);
            }
        }
    }

    /// 全ノードのヘルスチェック
    async fn check_agent_health(&self) -> Result<(), Box<dyn std::error::Error>> {
        let nodes = self.registry.list().await;
        let now = Utc::now();

        for agent in nodes {
            if agent.status != NodeStatus::Online {
                continue;
            }

            // 最終確認時刻からの経過時間を計算
            let elapsed = now.signed_duration_since(agent.last_seen);
            let elapsed_secs = elapsed.num_seconds() as u64;

            if elapsed_secs > self.timeout_secs {
                warn!(
                    "Node timeout detected: {} ({}) - last seen {} seconds ago",
                    agent.machine_name, agent.id, elapsed_secs
                );

                // オフライン判定
                if let Err(e) = self.registry.mark_offline(agent.id).await {
                    error!("Failed to mark agent {} offline: {}", agent.id, e);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::net::IpAddr;

    #[tokio::test]
    async fn test_health_monitor_creation() {
        let registry = NodeRegistry::new();
        let monitor = HealthMonitor::new(registry, 10, 60);

        assert_eq!(monitor.check_interval_secs, 10);
        assert_eq!(monitor.timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_check_agent_health_no_agents() {
        let registry = NodeRegistry::new();
        let monitor = HealthMonitor::new(registry, 10, 60);

        // ノードがいない場合、エラーにならないことを確認
        let result = monitor.check_agent_health().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_agent_health_online_agent() {
        let registry = NodeRegistry::new();

        // ノードを登録
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        registry.register(req).await.unwrap();

        let monitor = HealthMonitor::new(registry.clone(), 10, 60);

        // 最近登録したノードはタイムアウトしない
        let result = monitor.check_agent_health().await;
        assert!(result.is_ok());

        // ノードはまだオンライン
        let nodes = registry.list().await;
        assert_eq!(nodes[0].status, NodeStatus::Online);
    }
}
