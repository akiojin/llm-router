use super::*;

async fn setup_test_db() -> SqlitePool {
    crate::db::test_utils::test_db_pool().await
}

#[tokio::test]
async fn test_download_task_crud() {
    let pool = setup_test_db().await;

    // Create endpoint first (needed for foreign key)
    let endpoint = crate::types::endpoint::Endpoint::new(
        "Test Endpoint".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    crate::db::endpoints::create_endpoint(&pool, &endpoint)
        .await
        .unwrap();

    // Create download task
    let task = ModelDownloadTask::new(endpoint.id, "llama-3.2-1b".to_string());
    create_download_task(&pool, &task).await.unwrap();

    // Get
    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.model, "llama-3.2-1b");
    assert_eq!(fetched.status, DownloadStatus::Pending);
    assert_eq!(fetched.progress, 0.0);

    // Update progress
    update_download_progress(&pool, &task.id, 50.0, Some(10.5), Some(30))
        .await
        .unwrap();

    let updated = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(updated.status, DownloadStatus::Downloading);
    assert_eq!(updated.progress, 50.0);
    assert_eq!(updated.speed_mbps, Some(10.5));

    // Complete
    complete_download_task(&pool, &task.id, Some("model.gguf"))
        .await
        .unwrap();

    let completed = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(completed.status, DownloadStatus::Completed);
    assert_eq!(completed.progress, 100.0);
    assert_eq!(completed.filename, Some("model.gguf".to_string()));

    // Delete
    delete_download_task(&pool, &task.id).await.unwrap();
    let deleted = get_download_task(&pool, &task.id).await.unwrap();
    assert!(deleted.is_none());
}

#[tokio::test]
async fn test_list_active_tasks() {
    let pool = setup_test_db().await;

    // Create endpoint first
    let endpoint = crate::types::endpoint::Endpoint::new(
        "Test Endpoint".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    crate::db::endpoints::create_endpoint(&pool, &endpoint)
        .await
        .unwrap();

    // Create multiple tasks
    let task1 = ModelDownloadTask::new(endpoint.id, "model-1".to_string());
    let task2 = ModelDownloadTask::new(endpoint.id, "model-2".to_string());
    let task3 = ModelDownloadTask::new(endpoint.id, "model-3".to_string());

    create_download_task(&pool, &task1).await.unwrap();
    create_download_task(&pool, &task2).await.unwrap();
    create_download_task(&pool, &task3).await.unwrap();

    // Complete one
    complete_download_task(&pool, &task1.id, None)
        .await
        .unwrap();

    // List active
    let active = list_active_download_tasks(&pool, endpoint.id)
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
}

#[tokio::test]
async fn test_cancel_task() {
    let pool = setup_test_db().await;

    // Create endpoint first
    let endpoint = crate::types::endpoint::Endpoint::new(
        "Test Endpoint".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    crate::db::endpoints::create_endpoint(&pool, &endpoint)
        .await
        .unwrap();

    // Create task
    let task = ModelDownloadTask::new(endpoint.id, "model".to_string());
    create_download_task(&pool, &task).await.unwrap();

    // Cancel
    let cancelled = cancel_download_task(&pool, &task.id).await.unwrap();
    assert!(cancelled);

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, DownloadStatus::Cancelled);
}

/// Helper to create an endpoint for tests
async fn create_test_endpoint(pool: &SqlitePool) -> crate::types::endpoint::Endpoint {
    let endpoint = crate::types::endpoint::Endpoint::new(
        "Test EP".to_string(),
        "http://localhost:9090".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    crate::db::endpoints::create_endpoint(pool, &endpoint)
        .await
        .unwrap();
    endpoint
}

#[tokio::test]
async fn test_get_nonexistent_task() {
    let pool = setup_test_db().await;
    let result = get_download_task(&pool, "no-such-id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_task() {
    let pool = setup_test_db().await;
    let deleted = delete_download_task(&pool, "no-such-id").await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn test_fail_download_task() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "fail-model".to_string());
    create_download_task(&pool, &task).await.unwrap();

    let failed = fail_download_task(&pool, &task.id, "network error")
        .await
        .unwrap();
    assert!(failed);

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, DownloadStatus::Failed);
    assert_eq!(fetched.error_message, Some("network error".to_string()));
    assert!(fetched.completed_at.is_some());
}

#[tokio::test]
async fn test_cancel_completed_task_no_effect() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "completed-model".to_string());
    create_download_task(&pool, &task).await.unwrap();

    complete_download_task(&pool, &task.id, None).await.unwrap();

    // Cancelling a completed task should have no effect
    let cancelled = cancel_download_task(&pool, &task.id).await.unwrap();
    assert!(!cancelled);

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, DownloadStatus::Completed);
}

#[tokio::test]
async fn test_list_download_tasks_for_endpoint() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let t1 = ModelDownloadTask::new(ep.id, "m1".to_string());
    let t2 = ModelDownloadTask::new(ep.id, "m2".to_string());
    create_download_task(&pool, &t1).await.unwrap();
    create_download_task(&pool, &t2).await.unwrap();

    let tasks = list_download_tasks(&pool, ep.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn test_list_download_tasks_empty_for_other_endpoint() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "m1".to_string());
    create_download_task(&pool, &task).await.unwrap();

    // Different endpoint should have no tasks
    let other_id = Uuid::new_v4();
    let tasks = list_download_tasks(&pool, other_id).await.unwrap();
    assert!(tasks.is_empty());
}

#[tokio::test]
async fn test_update_progress_nonexistent_returns_false() {
    let pool = setup_test_db().await;
    let updated = update_download_progress(&pool, "ghost", 50.0, None, None)
        .await
        .unwrap();
    assert!(!updated);
}

#[tokio::test]
async fn test_complete_with_filename_override() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "dl-model".to_string());
    create_download_task(&pool, &task).await.unwrap();

    complete_download_task(&pool, &task.id, Some("final-name.gguf"))
        .await
        .unwrap();

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.filename, Some("final-name.gguf".to_string()));
    assert_eq!(fetched.progress, 100.0);
}

#[tokio::test]
async fn test_fail_nonexistent_task_returns_false() {
    let pool = setup_test_db().await;
    let result = fail_download_task(&pool, "ghost-task", "error")
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_cancel_nonexistent_task_returns_false() {
    let pool = setup_test_db().await;
    let result = cancel_download_task(&pool, "ghost-task").await.unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_complete_nonexistent_task_returns_false() {
    let pool = setup_test_db().await;
    let result = complete_download_task(&pool, "ghost-task", None)
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_cleanup_old_tasks_removes_old_completed() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "old-task".to_string());
    create_download_task(&pool, &task).await.unwrap();

    // Manually set completed_at to 10 days ago
    let old_time = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
    sqlx::query(
        "UPDATE model_download_tasks SET status = 'completed', completed_at = ? WHERE id = ?",
    )
    .bind(&old_time)
    .bind(&task.id)
    .execute(&pool)
    .await
    .unwrap();

    let cleaned = cleanup_old_download_tasks(&pool).await.unwrap();
    assert_eq!(cleaned, 1);

    let fetched = get_download_task(&pool, &task.id).await.unwrap();
    assert!(fetched.is_none());
}

#[tokio::test]
async fn test_cleanup_does_not_remove_active_tasks() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "active-task".to_string());
    create_download_task(&pool, &task).await.unwrap();

    // Active (pending) tasks should not be cleaned up
    let cleaned = cleanup_old_download_tasks(&pool).await.unwrap();
    assert_eq!(cleaned, 0);

    let fetched = get_download_task(&pool, &task.id).await.unwrap();
    assert!(fetched.is_some());
}

#[tokio::test]
async fn test_download_task_row_conversion() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let mut task = ModelDownloadTask::new(ep.id, "conv-model".to_string());
    task.speed_mbps = Some(25.5);
    task.eta_seconds = Some(120);
    create_download_task(&pool, &task).await.unwrap();

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.endpoint_id, ep.id);
    assert_eq!(fetched.model, "conv-model");
    assert_eq!(fetched.status, DownloadStatus::Pending);
    assert_eq!(fetched.progress, 0.0);
}

#[tokio::test]
async fn test_cancel_failed_task_no_effect() {
    let pool = setup_test_db().await;
    let ep = create_test_endpoint(&pool).await;

    let task = ModelDownloadTask::new(ep.id, "fail-then-cancel".to_string());
    create_download_task(&pool, &task).await.unwrap();

    fail_download_task(&pool, &task.id, "broken").await.unwrap();

    // Failed tasks cannot be cancelled
    let cancelled = cancel_download_task(&pool, &task.id).await.unwrap();
    assert!(!cancelled);

    let fetched = get_download_task(&pool, &task.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, DownloadStatus::Failed);
}
