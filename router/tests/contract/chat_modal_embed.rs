//! Contract test: dashboard embeds chat console as fullscreen modal (no new tab)

use axum::{body::to_bytes, http::Request, Router};
use llm_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use tower::ServiceExt;

async fn build_router() -> Router {
    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = DownloadTaskManager::new();
    let db_pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run migrations");
    let jwt_secret = "test-secret".to_string();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };
    api::create_router(state)
}

#[tokio::test]
async fn dashboard_contains_chat_modal() {
    let router = build_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/dashboard")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 512 * 1024).await.unwrap();
    let html = String::from_utf8(bytes.to_vec()).expect("dashboard html should be utf-8");

    assert!(
        html.contains("id=\"chat-open\""),
        "chat open button not found"
    );
    assert!(
        html.contains("id=\"chat-modal\""),
        "chat modal container not found"
    );
    assert!(html.contains("id=\"chat-iframe\""), "chat iframe not found");
    assert!(
        html.contains("src=\"/chat\""),
        "chat iframe src should point to /chat"
    );
}
