//! モデルリポジトリ（タスク管理）ユニットテスト
//!
//! TDD RED: DownloadTaskのライフサイクル管理

#[cfg(test)]
mod tests {
    use ollama_coordinator_coordinator::{
        registry::models::DownloadStatus, tasks::DownloadTaskManager,
    };
    use uuid::Uuid;

    /// T022: タスクライフサイクル全体のテスト
    #[tokio::test]
    async fn test_task_lifecycle() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();
        let model_name = "gpt-oss:7b".to_string();

        // 1. タスク作成
        let task = manager.create_task(agent_id, model_name.clone()).await;
        assert_eq!(task.agent_id, agent_id);
        assert_eq!(task.model_name, model_name);
        assert_eq!(task.status, DownloadStatus::Pending);
        assert_eq!(task.progress, 0.0);

        let task_id = task.id;

        // 2. タスク取得
        let retrieved_task = manager.get_task(task_id).await;
        assert!(
            retrieved_task.is_some(),
            "Created task should be retrievable"
        );
        let retrieved_task = retrieved_task.unwrap();
        assert_eq!(retrieved_task.id, task_id);

        // 3. 進捗更新
        let updated_task = manager.update_progress(task_id, 0.5, Some(1_000_000)).await;
        assert!(updated_task.is_some(), "Task should be updateable");
        let updated_task = updated_task.unwrap();
        assert_eq!(updated_task.progress, 0.5);
        assert_eq!(updated_task.speed, Some(1_000_000));
        assert_eq!(updated_task.status, DownloadStatus::InProgress);

        // 4. タスク完了
        let completed_task = manager.mark_completed(task_id).await;
        assert!(completed_task.is_some(), "Task should be completable");
        let completed_task = completed_task.unwrap();
        assert_eq!(completed_task.status, DownloadStatus::Completed);
        assert_eq!(completed_task.progress, 1.0);
        assert!(completed_task.completed_at.is_some());

        // 5. 完了したタスクの取得
        let final_task = manager.get_task(task_id).await;
        assert!(final_task.is_some());
        let final_task = final_task.unwrap();
        assert_eq!(final_task.status, DownloadStatus::Completed);
    }

    /// T022: タスク失敗のテスト
    #[tokio::test]
    async fn test_task_failure() {
        let manager = DownloadTaskManager::new();
        let agent_id = Uuid::new_v4();

        let task = manager
            .create_task(agent_id, "invalid-model".to_string())
            .await;
        let task_id = task.id;

        // タスクを失敗としてマーク
        let error_message = "Model not found".to_string();
        let failed_task = manager.mark_failed(task_id, error_message.clone()).await;

        assert!(failed_task.is_some(), "Task should be markable as failed");
        let failed_task = failed_task.unwrap();
        assert_eq!(failed_task.status, DownloadStatus::Failed);
        assert_eq!(failed_task.error, Some(error_message));
        assert!(failed_task.completed_at.is_some());
    }

    /// T022: 複数タスクの並行管理
    #[tokio::test]
    async fn test_multiple_tasks() {
        let manager = DownloadTaskManager::new();
        let agent1 = Uuid::new_v4();
        let agent2 = Uuid::new_v4();

        // 2つのタスクを作成
        let task1 = manager.create_task(agent1, "gpt-oss:20b".to_string()).await;
        let task2 = manager.create_task(agent2, "gpt-oss:7b".to_string()).await;

        // 両方のタスクが取得できることを確認
        let retrieved1 = manager.get_task(task1.id).await;
        let retrieved2 = manager.get_task(task2.id).await;

        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());
        assert_ne!(task1.id, task2.id, "Tasks should have unique IDs");
    }

    /// T022: エージェント別タスク一覧
    #[tokio::test]
    async fn test_list_tasks_by_agent() {
        let manager = DownloadTaskManager::new();
        let agent1 = Uuid::new_v4();
        let agent2 = Uuid::new_v4();

        // agent1に2つのタスク
        manager.create_task(agent1, "gpt-oss:20b".to_string()).await;
        manager.create_task(agent1, "gpt-oss:7b".to_string()).await;

        // agent2に1つのタスク
        manager.create_task(agent2, "gpt-oss:3b".to_string()).await;

        // agent1のタスク一覧
        let agent1_tasks = manager.list_tasks_by_agent(agent1).await;
        assert_eq!(agent1_tasks.len(), 2, "Agent1 should have 2 tasks");

        // agent2のタスク一覧
        let agent2_tasks = manager.list_tasks_by_agent(agent2).await;
        assert_eq!(agent2_tasks.len(), 1, "Agent2 should have 1 task");
    }

    /// T022: 全タスク一覧
    #[tokio::test]
    async fn test_list_all_tasks() {
        let manager = DownloadTaskManager::new();

        // 初期状態では空
        let tasks = manager.list_tasks().await;
        assert_eq!(tasks.len(), 0, "Initial task list should be empty");

        // 3つのタスクを作成
        for i in 0..3 {
            manager
                .create_task(Uuid::new_v4(), format!("model-{}", i))
                .await;
        }

        // 全タスクを取得
        let all_tasks = manager.list_tasks().await;
        assert_eq!(all_tasks.len(), 3, "Should have 3 tasks total");
    }

    /// T022: 完了タスクのクリーンアップ
    #[tokio::test]
    async fn test_cleanup_finished_tasks() {
        let manager = DownloadTaskManager::new();

        // 3つのタスクを作成
        let task1 = manager
            .create_task(Uuid::new_v4(), "model-1".to_string())
            .await;
        let task2 = manager
            .create_task(Uuid::new_v4(), "model-2".to_string())
            .await;
        let task3 = manager
            .create_task(Uuid::new_v4(), "model-3".to_string())
            .await;

        // task1を完了
        manager.mark_completed(task1.id).await;

        // task2を失敗
        manager
            .mark_failed(task2.id, "Test error".to_string())
            .await;

        // task3は進行中のまま
        manager.update_progress(task3.id, 0.5, None).await;

        // クリーンアップ実行
        let cleaned_count = manager.cleanup_finished_tasks().await;

        // 完了と失敗のタスク（2つ）がクリーンアップされる
        assert_eq!(cleaned_count, 2, "Should cleanup 2 finished tasks");

        // 進行中のタスクは残る
        let remaining_tasks = manager.list_tasks().await;
        assert_eq!(
            remaining_tasks.len(),
            1,
            "Should have 1 in-progress task remaining"
        );
        assert_eq!(remaining_tasks[0].id, task3.id);
    }

    /// T022: 存在しないタスクの取得
    #[tokio::test]
    async fn test_get_nonexistent_task() {
        let manager = DownloadTaskManager::new();
        let fake_id = Uuid::new_v4();

        let result = manager.get_task(fake_id).await;
        assert!(result.is_none(), "Non-existent task should return None");
    }

    /// T022: 存在しないタスクの更新
    #[tokio::test]
    async fn test_update_nonexistent_task() {
        let manager = DownloadTaskManager::new();
        let fake_id = Uuid::new_v4();

        let result = manager.update_progress(fake_id, 0.5, None).await;
        assert!(
            result.is_none(),
            "Updating non-existent task should return None"
        );
    }
}
