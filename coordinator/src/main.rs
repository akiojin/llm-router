//! Ollama Coordinator Server Entry Point

use ollama_coordinator_coordinator::{AppState, api, registry};

#[tokio::main]
async fn main() {
    println!("Ollama Coordinator v{}", env!("CARGO_PKG_VERSION"));

    // アプリケーション状態を初期化
    let state = AppState {
        registry: registry::AgentRegistry::new(),
    };

    // ルーター作成
    let app = api::create_router(state);

    // サーバー起動
    let host = "0.0.0.0:8080";
    let listener = tokio::net::TcpListener::bind(host)
        .await
        .expect("Failed to bind to address");

    println!("Coordinator server listening on {}", host);

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
