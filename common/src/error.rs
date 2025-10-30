//! エラー型定義
//!
//! 統一エラー型（thiserror使用）

use thiserror::Error;
use uuid::Uuid;

/// Common層のエラー型
#[derive(Debug, Error)]
pub enum CommonError {
    /// 設定エラー
    #[error("設定エラー: {0}")]
    Config(String),

    /// シリアライゼーションエラー
    #[error("シリアライゼーションエラー: {0}")]
    Serialization(#[from] serde_json::Error),

    /// UUID解析エラー
    #[error("UUID解析エラー: {0}")]
    UuidParse(#[from] uuid::Error),

    /// IPアドレス解析エラー
    #[error("IPアドレス解析エラー: {0}")]
    IpAddrParse(#[from] std::net::AddrParseError),

    /// 検証エラー
    #[error("検証エラー: {0}")]
    Validation(String),
}

/// Coordinatorエラー型
#[derive(Debug, Error)]
pub enum CoordinatorError {
    /// Common層エラー
    #[error(transparent)]
    Common(#[from] CommonError),

    /// エージェント未登録
    #[error("エージェントが見つかりません: {0}")]
    AgentNotFound(Uuid),

    /// 利用可能なエージェントがない
    #[error("利用可能なエージェントがありません")]
    NoAgentsAvailable,

    /// データベースエラー
    #[error("データベースエラー: {0}")]
    Database(String),

    /// HTTPクライアントエラー
    #[error("HTTPクライアントエラー: {0}")]
    Http(String),

    /// タイムアウトエラー
    #[error("タイムアウトエラー: {0}")]
    Timeout(String),

    /// 内部エラー
    #[error("内部エラー: {0}")]
    Internal(String),
}

/// Agentエラー型
#[derive(Debug, Error)]
pub enum AgentError {
    /// Common層エラー
    #[error(transparent)]
    Common(#[from] CommonError),

    /// Coordinatorへの接続エラー
    #[error("Coordinatorへの接続に失敗しました: {0}")]
    CoordinatorConnection(String),

    /// Ollamaへの接続エラー
    #[error("Ollamaへの接続に失敗しました: {0}")]
    OllamaConnection(String),

    /// 登録エラー
    #[error("エージェント登録に失敗しました: {0}")]
    Registration(String),

    /// ヘルスチェック送信エラー
    #[error("ヘルスチェック送信に失敗しました: {0}")]
    Heartbeat(String),

    /// メトリクス収集エラー
    #[error("メトリクス収集に失敗しました: {0}")]
    Metrics(String),

    /// GUI エラー
    #[error("GUIエラー: {0}")]
    Gui(String),

    /// 内部エラー
    #[error("内部エラー: {0}")]
    Internal(String),
}

/// Result型エイリアス（Common）
pub type CommonResult<T> = Result<T, CommonError>;

/// Result型エイリアス（Coordinator）
pub type CoordinatorResult<T> = Result<T, CoordinatorError>;

/// Result型エイリアス（Agent）
pub type AgentResult<T> = Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_error_display() {
        let error = CommonError::Config("テスト設定エラー".to_string());
        assert_eq!(error.to_string(), "設定エラー: テスト設定エラー");
    }

    #[test]
    fn test_coordinator_error_agent_not_found() {
        let agent_id = Uuid::new_v4();
        let error = CoordinatorError::AgentNotFound(agent_id);
        assert!(error.to_string().contains(&agent_id.to_string()));
    }

    #[test]
    fn test_coordinator_error_no_agents() {
        let error = CoordinatorError::NoAgentsAvailable;
        assert_eq!(error.to_string(), "利用可能なエージェントがありません");
    }

    #[test]
    fn test_agent_error_coordinator_connection() {
        let error = AgentError::CoordinatorConnection("タイムアウト".to_string());
        assert_eq!(
            error.to_string(),
            "Coordinatorへの接続に失敗しました: タイムアウト"
        );
    }

    #[test]
    fn test_error_from_conversion() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let common_error: CommonError = json_error.into();
        assert!(matches!(common_error, CommonError::Serialization(_)));
    }
}
