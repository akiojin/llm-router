//! モデルダウンロード進捗管理（ダウンロードタスクとその状態列挙）
//!
//! arch-review [H6]: types/endpoint.rs から凝集した DTO/状態型を分離。親は
//! `pub use download::{DownloadStatus, ModelDownloadTask}` で再エクスポートする。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

/// ダウンロードタスクの状態（SPEC-e8e9326e追加要件 2026-01-26）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    /// 待機中
    #[default]
    Pending,
    /// ダウンロード中
    Downloading,
    /// 完了
    Completed,
    /// 失敗
    Failed,
    /// キャンセル
    Cancelled,
}

impl DownloadStatus {
    /// DownloadStatusを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FromStr for DownloadStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "pending" => Self::Pending,
            "downloading" => Self::Downloading,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        })
    }
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

///
/// xLLMエンドポイント専用のモデルダウンロード進捗管理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDownloadTask {
    /// タスク識別子
    pub id: String,
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// モデル名（例: "llama-3.2-1b"）
    pub model: String,
    /// ダウンロード中のファイル名
    pub filename: Option<String>,
    /// ダウンロード状態
    pub status: DownloadStatus,
    /// 進捗率（0.0 〜 1.0）
    pub progress: f64,
    /// ダウンロード速度（Mbps）
    pub speed_mbps: Option<f64>,
    /// 残り時間（秒）
    pub eta_seconds: Option<u32>,
    /// エラーメッセージ（失敗時のみ）
    pub error_message: Option<String>,
    /// 開始時刻
    pub started_at: DateTime<Utc>,
    /// 完了時刻
    pub completed_at: Option<DateTime<Utc>>,
}

impl ModelDownloadTask {
    /// 新しいダウンロードタスクを作成
    pub fn new(endpoint_id: Uuid, model: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            endpoint_id,
            model,
            filename: None,
            status: DownloadStatus::Pending,
            progress: 0.0,
            speed_mbps: None,
            eta_seconds: None,
            error_message: None,
            started_at: Utc::now(),
            completed_at: None,
        }
    }

    /// ダウンロード完了かどうか
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled
        )
    }
}
