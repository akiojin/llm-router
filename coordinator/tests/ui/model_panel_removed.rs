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
async fn dashboard_html_has_no_model_panel() {
    // minimal router serving static files
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

    assert!(html.contains("Ollama Coordinator"));
    assert!(
        !html.contains("available-models-list"),
        "model panel should be removed"
    );
    assert!(
        !html.contains("loaded-models-list"),
        "model load panel should be removed"
    );
}
