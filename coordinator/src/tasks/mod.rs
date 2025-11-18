//! ダウンロードタスク管理
//!
//! モデルダウンロードタスクの作成、進捗追跡、状態管理

use crate::registry::models::DownloadTask;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// ダウンロードタスクマネージャー
#[derive(Clone)]
pub struct DownloadTaskManager {
    /// タスクの状態（タスクID -> DownloadTask）
    tasks: Arc<Mutex<HashMap<Uuid, DownloadTask>>>,
}

impl DownloadTaskManager {
    /// 新しいDownloadTaskManagerを作成
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// タスクを作成して登録
    pub async fn create_task(&self, agent_id: Uuid, model_name: String) -> DownloadTask {
        let task = DownloadTask::new(agent_id, model_name);
        let task_id = task.id;

        let mut tasks = self.tasks.lock().await;
        tasks.insert(task_id, task.clone());

        task
    }

    /// タスクの進捗を更新
    pub async fn update_progress(
        &self,
        task_id: Uuid,
        progress: f32,
        speed: Option<u64>,
    ) -> Option<DownloadTask> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            task.update_progress(progress, speed);
            if progress >= 1.0 {
                task.mark_completed();
            }
            Some(task.clone())
        } else {
            None
        }
    }

    /// タスクを完了としてマーク
    pub async fn mark_completed(&self, task_id: Uuid) -> Option<DownloadTask> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            task.mark_completed();
            Some(task.clone())
        } else {
            None
        }
    }

    /// タスクを失敗としてマーク
    pub async fn mark_failed(&self, task_id: Uuid, error: String) -> Option<DownloadTask> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            task.mark_failed(error);
            Some(task.clone())
        } else {
            None
        }
    }

    /// タスクを取得
    pub async fn get_task(&self, task_id: Uuid) -> Option<DownloadTask> {
        let tasks = self.tasks.lock().await;
        tasks.get(&task_id).cloned()
    }

    /// すべてのタスクを取得
    pub async fn list_tasks(&self) -> Vec<DownloadTask> {
        let tasks = self.tasks.lock().await;
        tasks.values().cloned().collect()
    }

    /// 特定のエージェントのタスクを取得
    pub async fn list_tasks_by_agent(&self, agent_id: Uuid) -> Vec<DownloadTask> {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|task| task.agent_id == agent_id)
            .cloned()
            .collect()
    }

    /// 進行中のタスクを取得
    pub async fn list_active_tasks(&self) -> Vec<DownloadTask> {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|task| !task.is_finished())
            .cloned()
            .collect()
    }

    /// 完了したタスクを削除（クリーンアップ）
    pub async fn cleanup_finished_tasks(&self) -> usize {
        let mut tasks = self.tasks.lock().await;
        let initial_count = tasks.len();
        tasks.retain(|_, task| !task.is_finished());
        initial_count - tasks.len()
    }
}

impl Default for DownloadTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::models::DownloadStatus;

    #[tokio::test]
    async fn test_create_task() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task = manager
            .create_task(agent_id, "gpt-oss:7b".to_string())
            .await;

        assert_eq!(task.agent_id, agent_id);
        assert_eq!(task.model_name, "gpt-oss:7b");
        assert_eq!(task.status, DownloadStatus::Pending);

        // タスクが取得できることを確認
        let retrieved = manager.get_task(task.id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, task.id);
    }

    #[tokio::test]
    async fn test_update_progress() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task = manager
            .create_task(agent_id, "test-model".to_string())
            .await;

        // 進捗更新
        let updated = manager.update_progress(task.id, 0.5, Some(1_000_000)).await;
        assert!(updated.is_some());

        let updated_task = updated.unwrap();
        assert_eq!(updated_task.progress, 0.5);
        assert_eq!(updated_task.speed, Some(1_000_000));
        assert_eq!(updated_task.status, DownloadStatus::InProgress);
    }

    #[tokio::test]
    async fn test_mark_completed() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task = manager
            .create_task(agent_id, "test-model".to_string())
            .await;

        let completed = manager.mark_completed(task.id).await;
        assert!(completed.is_some());

        let completed_task = completed.unwrap();
        assert_eq!(completed_task.status, DownloadStatus::Completed);
        assert_eq!(completed_task.progress, 1.0);
        assert!(completed_task.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_mark_failed() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task = manager
            .create_task(agent_id, "test-model".to_string())
            .await;

        let failed = manager
            .mark_failed(task.id, "Network error".to_string())
            .await;
        assert!(failed.is_some());

        let failed_task = failed.unwrap();
        assert_eq!(failed_task.status, DownloadStatus::Failed);
        assert_eq!(failed_task.error, Some("Network error".to_string()));
    }

    #[tokio::test]
    async fn test_list_tasks_by_agent() {
        let manager = DownloadTaskManager::new();
        let agent1 = Uuid::new_v4();
        let agent2 = Uuid::new_v4();

        manager.create_task(agent1, "model1".to_string()).await;
        manager.create_task(agent1, "model2".to_string()).await;
        manager.create_task(agent2, "model3".to_string()).await;

        let agent1_tasks = manager.list_tasks_by_agent(agent1).await;
        assert_eq!(agent1_tasks.len(), 2);

        let agent2_tasks = manager.list_tasks_by_agent(agent2).await;
        assert_eq!(agent2_tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_finished_tasks() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task1 = manager.create_task(agent_id, "model1".to_string()).await;
        let task2 = manager.create_task(agent_id, "model2".to_string()).await;
        let _task3 = manager.create_task(agent_id, "model3".to_string()).await;

        // 2つのタスクを完了
        manager.mark_completed(task1.id).await;
        manager.mark_failed(task2.id, "Error".to_string()).await;

        // クリーンアップ
        let removed = manager.cleanup_finished_tasks().await;
        assert_eq!(removed, 2);

        let remaining = manager.list_tasks().await;
        assert_eq!(remaining.len(), 1);
    }
}
