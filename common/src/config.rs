//! 設定管理
//!
//! CoordinatorConfig, AgentConfig等の設定構造体

use serde::{Deserialize, Serialize};

/// Coordinator設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// ホストアドレス (デフォルト: "0.0.0.0")
    #[serde(default = "default_host")]
    pub host: String,

    /// ポート番号 (デフォルト: 8080)
    #[serde(default = "default_port")]
    pub port: u16,

    /// データベースURL (デフォルト: "sqlite://coordinator.db")
    #[serde(default = "default_database_url")]
    pub database_url: String,

    /// ヘルスチェック間隔（秒）(デフォルト: 30)
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,

    /// エージェントタイムアウト（秒）(デフォルト: 60)
    #[serde(default = "default_agent_timeout")]
    pub agent_timeout_secs: u64,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_database_url() -> String {
    "sqlite://coordinator.db".to_string()
}

fn default_health_check_interval() -> u64 {
    30
}

fn default_agent_timeout() -> u64 {
    60
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            database_url: default_database_url(),
            health_check_interval_secs: default_health_check_interval(),
            agent_timeout_secs: default_agent_timeout(),
        }
    }
}

/// Agent設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// CoordinatorのURL (デフォルト: "http://localhost:8080")
    #[serde(default = "default_coordinator_url")]
    pub coordinator_url: String,

    /// OllamaのURL (デフォルト: "http://localhost:11434")
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,

    /// ハートビート送信間隔（秒）(デフォルト: 10)
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,

    /// Windows起動時の自動起動 (デフォルト: false)
    #[serde(default)]
    pub auto_start: bool,
}

fn default_coordinator_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_heartbeat_interval() -> u64 {
    10
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            coordinator_url: default_coordinator_url(),
            ollama_url: default_ollama_url(),
            heartbeat_interval_secs: default_heartbeat_interval(),
            auto_start: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_config_defaults() {
        let config = CoordinatorConfig::default();

        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
        assert_eq!(config.database_url, "sqlite://coordinator.db");
        assert_eq!(config.health_check_interval_secs, 30);
        assert_eq!(config.agent_timeout_secs, 60);
    }

    #[test]
    fn test_agent_config_defaults() {
        let config = AgentConfig::default();

        assert_eq!(config.coordinator_url, "http://localhost:8080");
        assert_eq!(config.ollama_url, "http://localhost:11434");
        assert_eq!(config.heartbeat_interval_secs, 10);
        assert!(!config.auto_start);
    }

    #[test]
    fn test_coordinator_config_deserialization() {
        let json = r#"{"host":"127.0.0.1","port":9000}"#;
        let config: CoordinatorConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 9000);
        // デフォルト値が適用される
        assert_eq!(config.database_url, "sqlite://coordinator.db");
    }

    #[test]
    fn test_agent_config_deserialization() {
        let json = r#"{"coordinator_url":"http://192.168.1.10:8080","auto_start":true}"#;
        let config: AgentConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.coordinator_url, "http://192.168.1.10:8080");
        assert!(config.auto_start);
        // デフォルト値が適用される
        assert_eq!(config.ollama_url, "http://localhost:11434");
    }
}
