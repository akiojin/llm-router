//! エージェントHTTP APIモジュール
//!
//! コーディネーターからの指示を受け取るHTTPエンドポイント

pub mod logs;
pub mod models;
pub mod openai;

use axum::{
    routing::{get, post},
    Router,
};
use models::AppState;

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/pull", post(models::pull_model))
        .route("/logs", get(logs::list_logs)) // legacy
        .route("/api/logs", get(logs::list_logs))
        // OpenAI互換エンドポイント（コーディネーター経由の推論用）
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/completions", post(openai::completions))
        .route("/v1/embeddings", post(openai::embeddings))
        .route("/v1/models", get(openai::list_models))
        .with_state(state)
}
