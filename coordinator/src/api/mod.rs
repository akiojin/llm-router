//! REST APIハンドラー
//!
//! エージェント登録、ヘルスチェック、プロキシAPI

pub mod agent;

use axum::{
    routing::post,
    Router,
};
use crate::AppState;

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/agents", post(agent::register_agent))
        .with_state(state)
}
