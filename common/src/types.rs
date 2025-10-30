//! 共通型定義
//!
//! Agent, HealthMetrics, Request等のコアデータ型

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

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
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 累積リクエスト数
    pub total_requests: u64,
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
        };

        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();

        assert_eq!(agent, deserialized);
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
