//! 通信プロトコル定義
//!
//! Agent↔Coordinator間の通信メッセージ

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

/// エージェント登録リクエスト
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterRequest {
    /// マシン名
    pub machine_name: String,
    /// IPアドレス
    pub ip_address: IpAddr,
    /// Ollamaバージョン
    pub ollama_version: String,
    /// Ollamaポート番号
    pub ollama_port: u16,
}

/// エージェント登録レスポンス
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterResponse {
    /// エージェントID
    pub agent_id: Uuid,
    /// ステータス ("registered" または "updated")
    pub status: RegisterStatus,
}

/// 登録ステータス
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegisterStatus {
    /// 新規登録
    Registered,
    /// 既存エージェント更新
    Updated,
}

/// ヘルスチェックリクエスト
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheckRequest {
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
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 過去N件の平均レスポンスタイム (ms)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_response_time_ms: Option<f32>,
    /// エージェントがロード済みのモデル一覧
    #[serde(default)]
    pub loaded_models: Vec<String>,
}

/// Ollamaチャットリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// モデル名
    pub model: String,
    /// メッセージ配列
    pub messages: Vec<ChatMessage>,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}

/// チャットメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// ロール ("user", "assistant", "system")
    pub role: String,
    /// メッセージ内容
    pub content: String,
}

/// Ollamaチャットレスポンス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// レスポンスメッセージ
    pub message: ChatMessage,
    /// 完了フラグ
    pub done: bool,
}

/// Ollama Generateリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// モデル名
    pub model: String,
    /// プロンプト
    pub prompt: String,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_request_serialization() {
        let request = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            ollama_version: "0.1.0".to_string(),
            ollama_port: 11434,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: RegisterRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_register_status_serialization() {
        assert_eq!(
            serde_json::to_string(&RegisterStatus::Registered).unwrap(),
            "\"registered\""
        );
        assert_eq!(
            serde_json::to_string(&RegisterStatus::Updated).unwrap(),
            "\"updated\""
        );
    }

    #[test]
    fn test_health_check_request_serialization() {
        let request = HealthCheckRequest {
            agent_id: Uuid::new_v4(),
            cpu_usage: 45.5,
            memory_usage: 60.2,
            gpu_usage: Some(33.0),
            gpu_memory_usage: Some(71.0),
            gpu_memory_total_mb: Some(8192),
            gpu_memory_used_mb: Some(5800),
            gpu_temperature: Some(72.5),
            active_requests: 3,
            average_response_time_ms: Some(123.4),
            loaded_models: vec!["gpt-oss:20b".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: HealthCheckRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.cpu_usage, deserialized.cpu_usage);
        assert_eq!(request.memory_usage, deserialized.memory_usage);
        assert_eq!(request.gpu_usage, deserialized.gpu_usage);
        assert_eq!(request.gpu_memory_usage, deserialized.gpu_memory_usage);
        assert_eq!(request.active_requests, deserialized.active_requests);
        assert_eq!(
            request.average_response_time_ms,
            deserialized.average_response_time_ms
        );
        assert_eq!(request.loaded_models, deserialized.loaded_models);
    }

    #[test]
    fn test_chat_request_default_stream_false() {
        let json = r#"{"model":"llama2","messages":[{"role":"user","content":"Hello"}]}"#;
        let request: ChatRequest = serde_json::from_str(json).unwrap();

        assert!(!request.stream);
    }
}
