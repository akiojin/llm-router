//! モデル情報管理
//!
//! Ollamaモデルのメタデータとダウンロードタスク管理

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Ollamaモデル情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// モデル名（例: "gpt-oss:20b", "llama3.2"）
    pub name: String,
    /// モデルサイズ（バイト）
    pub size: u64,
    /// モデルの説明
    pub description: String,
    /// 必要なGPUメモリ（バイト）
    pub required_memory: u64,
    /// タグ（例: ["vision", "tools", "thinking"]）
    pub tags: Vec<String>,
}

impl ModelInfo {
    /// 新しいModelInfoを作成
    pub fn new(
        name: String,
        size: u64,
        description: String,
        required_memory: u64,
        tags: Vec<String>,
    ) -> Self {
        Self {
            name,
            size,
            description,
            required_memory,
            tags,
        }
    }

    /// 必要メモリをMB単位で取得
    pub fn required_memory_mb(&self) -> u64 {
        self.required_memory / (1024 * 1024)
    }

    /// 必要メモリをGB単位で取得
    pub fn required_memory_gb(&self) -> f64 {
        self.required_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

/// エージェントにインストール済みのモデル
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledModel {
    /// モデル名
    pub name: String,
    /// モデルサイズ（バイト）
    pub size: u64,
    /// インストール日時
    pub installed_at: DateTime<Utc>,
    /// digest（Ollamaのモデル識別子）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl InstalledModel {
    /// 新しいInstalledModelを作成
    pub fn new(name: String, size: u64) -> Self {
        Self {
            name,
            size,
            installed_at: Utc::now(),
            digest: None,
        }
    }

    /// digestを指定してInstalledModelを作成
    pub fn with_digest(name: String, size: u64, digest: String) -> Self {
        Self {
            name,
            size,
            installed_at: Utc::now(),
            digest: Some(digest),
        }
    }
}

/// ダウンロードタスク
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadTask {
    /// タスクID
    pub id: Uuid,
    /// エージェントID
    pub agent_id: Uuid,
    /// モデル名
    pub model_name: String,
    /// ステータス
    pub status: DownloadStatus,
    /// 進捗（0.0-1.0）
    pub progress: f32,
    /// ダウンロード速度（バイト/秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<u64>,
    /// 開始日時
    pub started_at: DateTime<Utc>,
    /// 完了日時
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// エラーメッセージ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DownloadTask {
    /// 新しいダウンロードタスクを作成
    pub fn new(agent_id: Uuid, model_name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            agent_id,
            model_name,
            status: DownloadStatus::Pending,
            progress: 0.0,
            speed: None,
            started_at: Utc::now(),
            completed_at: None,
            error: None,
        }
    }

    /// 進捗を更新
    pub fn update_progress(&mut self, progress: f32, speed: Option<u64>) {
        self.progress = progress.clamp(0.0, 1.0);
        self.speed = speed;

        if self.status == DownloadStatus::Pending && progress > 0.0 {
            self.status = DownloadStatus::InProgress;
        }
    }

    /// 完了として  マーク
    pub fn mark_completed(&mut self) {
        self.status = DownloadStatus::Completed;
        self.progress = 1.0;
        self.completed_at = Some(Utc::now());
    }

    /// 失敗としてマーク
    pub fn mark_failed(&mut self, error: String) {
        self.status = DownloadStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error);
    }

    /// タスクが完了しているか（成功または失敗）
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            DownloadStatus::Completed | DownloadStatus::Failed
        )
    }
}

/// ダウンロードステータス
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    /// 待機中
    Pending,
    /// ダウンロード中
    InProgress,
    /// 完了
    Completed,
    /// 失敗
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_info_new() {
        let model = ModelInfo::new(
            "gpt-oss:20b".to_string(),
            10_000_000_000,
            "GPT-OSS 20B model".to_string(),
            16_000_000_000,
            vec!["llm".to_string(), "text".to_string()],
        );

        assert_eq!(model.name, "gpt-oss:20b");
        assert_eq!(model.size, 10_000_000_000);
        assert_eq!(model.required_memory_gb(), 14.901161193847656);
    }

    #[test]
    fn test_installed_model_new() {
        let model = InstalledModel::new("llama3.2".to_string(), 5_000_000_000);

        assert_eq!(model.name, "llama3.2");
        assert_eq!(model.size, 5_000_000_000);
        assert!(model.digest.is_none());
    }

    #[test]
    fn test_download_task_lifecycle() {
        let mut task = DownloadTask::new(Uuid::new_v4(), "gpt-oss:7b".to_string());

        assert_eq!(task.status, DownloadStatus::Pending);
        assert_eq!(task.progress, 0.0);
        assert!(!task.is_finished());

        // 進捗更新
        task.update_progress(0.5, Some(1_000_000));
        assert_eq!(task.status, DownloadStatus::InProgress);
        assert_eq!(task.progress, 0.5);
        assert_eq!(task.speed, Some(1_000_000));

        // 完了
        task.mark_completed();
        assert_eq!(task.status, DownloadStatus::Completed);
        assert_eq!(task.progress, 1.0);
        assert!(task.is_finished());
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_download_task_failure() {
        let mut task = DownloadTask::new(Uuid::new_v4(), "invalid-model".to_string());

        task.mark_failed("Model not found".to_string());
        assert_eq!(task.status, DownloadStatus::Failed);
        assert!(task.is_finished());
        assert_eq!(task.error, Some("Model not found".to_string()));
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_progress_clamping() {
        let mut task = DownloadTask::new(Uuid::new_v4(), "test-model".to_string());

        // 範囲外の値はクランプされる
        task.update_progress(1.5, None);
        assert_eq!(task.progress, 1.0);

        task.update_progress(-0.5, None);
        assert_eq!(task.progress, 0.0);
    }
}
