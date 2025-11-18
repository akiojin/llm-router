use axum::{body::to_bytes, Router};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use tower::ServiceExt;

fn build_app() -> Router {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = ollama_coordinator_coordinator::tasks::DownloadTaskManager::new();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    };

    api::create_router(state)
}

#[tokio::test]
async fn modals_are_hidden_on_initial_load() {
    // スタティックHTMLを直接取得し、初期状態でモーダルが非表示になっていることを確認する
    let app = build_app();
    let body = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/dashboard/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .into_body();

    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&bytes);

    assert!(
        html.contains("id=\"agent-modal\" class=\"modal hidden\"")
            || html.contains("class=\"modal hidden\" id=\"agent-modal\""),
        "agent modal should be hidden by default",
    );
    assert!(
        html.contains("id=\"request-modal\" class=\"modal hidden\"")
            || html.contains("class=\"modal hidden\" id=\"request-modal\""),
        "request modal should be hidden by default",
    );
}
