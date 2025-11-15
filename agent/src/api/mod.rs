//! エージェントHTTP APIモジュール
//!
//! コーディネーターからの指示を受け取るHTTPエンドポイント

pub mod logs;
pub mod models;

use axum::{
    routing::{get, post},
    Router,
};
use models::AppState;

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/pull", post(models::pull_model))
        .route("/logs", get(logs::list_logs))
        .with_state(state)
}
