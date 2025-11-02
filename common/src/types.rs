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
        };
        assert!(valid.is_valid());

        let zero_count = GpuDeviceInfo {
            model: "AMD".to_string(),
            count: 0,
        };
        assert!(!zero_count.is_valid());

        let empty_model = GpuDeviceInfo {
            model: " ".to_string(),
            count: 1,
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
}
