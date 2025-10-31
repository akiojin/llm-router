//! REST APIハンドラー
//!
//! エージェント登録、ヘルスチェック、プロキシAPI

pub mod agent;
pub mod dashboard;
pub mod health;
pub mod proxy;

use crate::AppState;
use axum::{
    routing::{delete, get, get_service, post, put},
    Router,
};
use tower_http::services::ServeDir;

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    let static_files = get_service(
        ServeDir::new("coordinator/src/web/static").append_index_html_on_directories(true),
    );

    Router::new()
        .route(
            "/api/agents",
            post(agent::register_agent).get(agent::list_agents),
        )
        .route("/api/agents/:agent_id", delete(agent::delete_agent))
        .route(
            "/api/agents/:agent_id/disconnect",
            post(agent::disconnect_agent),
        )
        .route(
            "/api/agents/:agent_id/settings",
            put(agent::update_agent_settings),
        )
        .route("/api/agents/metrics", get(agent::list_agent_metrics))
        .route("/api/metrics/summary", get(agent::metrics_summary))
        .route("/api/dashboard/agents", get(dashboard::get_agents))
        .route("/api/dashboard/stats", get(dashboard::get_stats))
        .route(
            "/api/dashboard/request-history",
            get(dashboard::get_request_history),
        )
        .route("/api/health", post(health::health_check))
        .route("/api/chat", post(proxy::proxy_chat))
        .route("/api/generate", post(proxy::proxy_generate))
        .nest_service("/dashboard", static_files)
        .with_state(state)
}
