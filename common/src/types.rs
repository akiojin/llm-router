//! 共通型定義
//!
//! Agent, HealthMetrics, Request等のコアデータ型

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// GPUデバイス情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuDeviceInfo {
    /// GPUモデル名
    pub model: String,
    /// 当該モデルの枚数
    pub count: u32,
    /// GPUメモリ容量（バイト単位、オプション）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<u64>,
}

impl GpuDeviceInfo {
    /// GPU情報として有効か検証する
    pub fn is_valid(&self) -> bool {
        self.count > 0 && !self.model.trim().is_empty()
    }
}

/// エージェント
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    /// 一意識別子
    pub id: Uuid,
    /// マシン名
    pub machine_name: String,
    /// IPアドレス
    pub ip_address: IpAddr,
    /// Ollamaバージョン
    pub ollama_version: String,
    /// Ollamaポート番号
    pub ollama_port: u16,
    /// 状態（オンライン/オフライン）
    pub status: AgentStatus,
    /// 登録日時
    pub registered_at: DateTime<Utc>,
    /// 最終ヘルスチェック時刻
    pub last_seen: DateTime<Utc>,
    /// カスタム表示名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// タグ
    #[serde(default)]
    pub tags: Vec<String>,
    /// メモ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// ロード済みモデル一覧
    #[serde(default)]
    pub loaded_models: Vec<String>,
    /// 搭載GPUの詳細
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_devices: Vec<GpuDeviceInfo>,
    /// GPU利用可能フラグ
    pub gpu_available: bool,
    /// GPU個数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_count: Option<u32>,
    /// GPUモデル名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model: Option<String>,
    /// GPUモデル名（詳細）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model_name: Option<String>,
    /// GPU計算能力 (例: "8.9")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア (0-10000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_capability_score: Option<u32>,
}

/// エージェント状態
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    /// オンライン
    Online,
    /// オフライン
    Offline,
}

/// ヘルスメトリクス
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthMetrics {
    /// エージェントID
    pub agent_id: Uuid,
    /// CPU使用率 (0.0-100.0)
    pub cpu_usage: f32,
    /// メモリ使用率 (0.0-100.0)
    pub memory_usage: f32,
    /// GPU使用率 (0.0-100.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_usage: Option<f32>,
    /// GPUメモリ使用率 (0.0-100.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_usage: Option<f32>,
    /// GPUメモリ総容量 (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU使用メモリ (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU温度 (℃)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_temperature: Option<f32>,
    /// GPUモデル名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model_name: Option<String>,
    /// GPU計算能力
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_capability_score: Option<u32>,
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 直近の平均レスポンスタイム (ms)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_response_time_ms: Option<f32>,
    /// タイムスタンプ
    pub timestamp: DateTime<Utc>,
}

/// リクエスト
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    /// リクエストID
    pub id: Uuid,
    /// 振り分け先エージェントID
    pub agent_id: Uuid,
    /// エンドポイント ("/api/chat" など)
    pub endpoint: String,
    /// ステータス
    pub status: RequestStatus,
    /// 処理時間（ミリ秒）
    pub duration_ms: Option<u64>,
    /// 作成日時
    pub created_at: DateTime<Utc>,
    /// 完了日時
    pub completed_at: Option<DateTime<Utc>>,
}

/// リクエストステータス
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    /// 保留中
    Pending,
    /// 処理中
    Processing,
    /// 完了
    Completed,
    /// 失敗
    Failed,
}

/// エージェントメトリクス
///
/// エージェントから定期的に送信される負荷情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// エージェントID
    pub agent_id: Uuid,
    /// CPU使用率（0.0〜100.0）
    pub cpu_usage: f64,
    /// メモリ使用率（0.0〜100.0）
    pub memory_usage: f64,
    /// アクティブリクエスト数
    pub active_requests: u32,
    /// 平均レスポンス時間（ミリ秒）
    pub avg_response_time_ms: Option<f64>,
    /// タイムスタンプ
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_serialization() {
        let agent = Agent {
            id: Uuid::new_v4(),
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
            status: AgentStatus::Online,
            registered_at: Utc::now(),
            last_seen: Utc::now(),
            custom_name: Some("Custom".to_string()),
            tags: vec!["primary".to_string()],
            notes: Some("memo".to_string()),
            loaded_models: vec!["gpt-oss:20b".to_string()],
            gpu_devices: vec![GpuDeviceInfo {
                model: "NVIDIA RTX 4090".to_string(),
                count: 2,
                memory: None,
            }],
            gpu_available: true,
            gpu_count: Some(2),
            gpu_model: Some("NVIDIA RTX 4090".to_string()),
            gpu_model_name: Some("NVIDIA GeForce RTX 4090".to_string()),
            gpu_compute_capability: Some("8.9".to_string()),
            gpu_capability_score: Some(9850),
        };

        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();

        assert_eq!(agent, deserialized);
    }

    #[test]
    fn test_agent_defaults() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "machine_name": "machine",
            "ip_address": "127.0.0.1",
            "ollama_version": "0.1.0",
            "ollama_port": 11434,
            "status": "online",
            "registered_at": "2025-10-31T00:00:00Z",
            "last_seen": "2025-10-31T00:00:00Z",
            "gpu_available": false
        }"#;

        let agent: Agent = serde_json::from_str(json).unwrap();
        assert!(agent.custom_name.is_none());
        assert!(agent.tags.is_empty());
        assert!(agent.notes.is_none());
        assert!(agent.loaded_models.is_empty());
        assert!(agent.gpu_devices.is_empty());
        assert!(!agent.gpu_available);
        assert!(agent.gpu_count.is_none());
        assert!(agent.gpu_model.is_none());
        assert!(agent.gpu_model_name.is_none());
        assert!(agent.gpu_compute_capability.is_none());
        assert!(agent.gpu_capability_score.is_none());
    }

    #[test]
    fn test_agent_status_serialization() {
        assert_eq!(
            serde_json::to_string(&AgentStatus::Online).unwrap(),
            "\"online\""
        );
        assert_eq!(
            serde_json::to_string(&AgentStatus::Offline).unwrap(),
            "\"offline\""
        );
    }

    #[test]
    fn test_gpu_device_info_validation() {
        let valid = GpuDeviceInfo {
            model: "NVIDIA RTX 4090".to_string(),
            count: 2,
            memory: None,
        };
        assert!(valid.is_valid());

        let zero_count = GpuDeviceInfo {
            model: "AMD".to_string(),
            count: 0,
            memory: None,
        };
        assert!(!zero_count.is_valid());

        let empty_model = GpuDeviceInfo {
            model: " ".to_string(),
            count: 1,
            memory: None,
        };
        assert!(!empty_model.is_valid());
    }

    #[test]
    fn test_request_status_serialization() {
        assert_eq!(
            serde_json::to_string(&RequestStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&RequestStatus::Processing).unwrap(),
            "\"processing\""
        );
        assert_eq!(
            serde_json::to_string(&RequestStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&RequestStatus::Failed).unwrap(),
            "\"failed\""
        );
    }

    #[test]
    fn test_agent_metrics_serialization() {
        let agent_id = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let timestamp = DateTime::parse_from_rfc3339("2025-11-02T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let metrics = AgentMetrics {
            agent_id,
            cpu_usage: 45.5,
            memory_usage: 60.2,
            active_requests: 3,
            avg_response_time_ms: Some(250.5),
            timestamp,
        };

        // JSON serialization
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("\"agent_id\":\"12345678-1234-1234-1234-123456789012\""));
        assert!(json.contains("\"cpu_usage\":45.5"));
        assert!(json.contains("\"memory_usage\":60.2"));
        assert!(json.contains("\"active_requests\":3"));
        assert!(json.contains("\"avg_response_time_ms\":250.5"));

        // JSON deserialization
        let deserialized: AgentMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.agent_id, agent_id);
        assert_eq!(deserialized.cpu_usage, 45.5);
        assert_eq!(deserialized.memory_usage, 60.2);
        assert_eq!(deserialized.active_requests, 3);
        assert_eq!(deserialized.avg_response_time_ms, Some(250.5));
        assert_eq!(deserialized.timestamp, timestamp);
    }

    #[test]
    fn test_agent_metrics_deserialization_without_avg_response_time() {
        let json = r#"{
            "agent_id": "12345678-1234-1234-1234-123456789012",
            "cpu_usage": 30.0,
            "memory_usage": 40.0,
            "active_requests": 2,
            "avg_response_time_ms": null,
            "timestamp": "2025-11-02T10:00:00Z"
        }"#;

        let metrics: AgentMetrics = serde_json::from_str(json).unwrap();
        assert_eq!(metrics.cpu_usage, 30.0);
        assert_eq!(metrics.memory_usage, 40.0);
        assert_eq!(metrics.active_requests, 2);
        assert_eq!(metrics.avg_response_time_ms, None);
    }
}
