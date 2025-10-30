//! REST APIハンドラー
//!
//! エージェント登録、ヘルスチェック、プロキシAPI

pub mod agent;
pub mod health;
pub mod proxy;

use crate::AppState;
use axum::{routing::post, Router};

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/agents",
            post(agent::register_agent).get(agent::list_agents),
        )
        .route("/api/health", post(health::health_check))
        .route("/api/chat", post(proxy::proxy_chat))
        .route("/api/generate", post(proxy::proxy_generate))
        .with_state(state)
}
