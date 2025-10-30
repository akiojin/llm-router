//! ヘルスチェックモニター
//!
//! 定期的にエージェントのタイムアウトを検知し、オフライン判定

use crate::registry::AgentRegistry;
use chrono::Utc;
use ollama_coordinator_common::types::AgentStatus;
use tokio::time::{interval, Duration};

/// ヘルスモニター
pub struct HealthMonitor {
    registry: AgentRegistry,
    check_interval_secs: u64,
    timeout_secs: u64,
}

impl HealthMonitor {
    /// 新しいヘルスモニターを作成
    pub fn new(registry: AgentRegistry, check_interval_secs: u64, timeout_secs: u64) -> Self {
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

        println!(
            "Health monitor started: check_interval={}s, timeout={}s",
            self.check_interval_secs, self.timeout_secs
        );

        loop {
            timer.tick().await;

            if let Err(e) = self.check_agent_health().await {
                eprintln!("Health check error: {}", e);
            }
        }
    }

    /// 全エージェントのヘルスチェック
    async fn check_agent_health(&self) -> Result<(), Box<dyn std::error::Error>> {
        let agents = self.registry.list().await;
        let now = Utc::now();

        for agent in agents {
            if agent.status != AgentStatus::Online {
                continue;
            }

            // 最終確認時刻からの経過時間を計算
            let elapsed = now.signed_duration_since(agent.last_seen);
            let elapsed_secs = elapsed.num_seconds() as u64;

            if elapsed_secs > self.timeout_secs {
                println!(
                    "Agent timeout detected: {} ({}) - last seen {} seconds ago",
                    agent.machine_name, agent.id, elapsed_secs
                );

                // オフライン判定
                if let Err(e) = self.registry.mark_offline(agent.id).await {
                    eprintln!("Failed to mark agent {} offline: {}", agent.id, e);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_coordinator_common::protocol::RegisterRequest;
    use std::net::IpAddr;

    #[tokio::test]
    async fn test_health_monitor_creation() {
        let registry = AgentRegistry::new();
        let monitor = HealthMonitor::new(registry, 10, 60);

        assert_eq!(monitor.check_interval_secs, 10);
        assert_eq!(monitor.timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_check_agent_health_no_agents() {
        let registry = AgentRegistry::new();
        let monitor = HealthMonitor::new(registry, 10, 60);

        // エージェントがいない場合、エラーにならないことを確認
        let result = monitor.check_agent_health().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_agent_health_online_agent() {
        let registry = AgentRegistry::new();

        // エージェントを登録
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };
        registry.register(req).await.unwrap();

        let monitor = HealthMonitor::new(registry.clone(), 10, 60);

        // 最近登録したエージェントはタイムアウトしない
        let result = monitor.check_agent_health().await;
        assert!(result.is_ok());

        // エージェントはまだオンライン
        let agents = registry.list().await;
        assert_eq!(agents[0].status, AgentStatus::Online);
    }
}
