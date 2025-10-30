//! Ollama Coordinator Server Entry Point

use ollama_coordinator_coordinator::{AppState, api, registry, db};

#[tokio::main]
async fn main() {
    println!("Ollama Coordinator v{}", env!("CARGO_PKG_VERSION"));

    // データベースURL
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://coordinator.db".to_string());

    println!("Connecting to database: {}", database_url);

    // データベース接続プールを作成
    let db_pool = db::create_pool(&database_url)
        .await
        .expect("Failed to create database pool");

    println!("Database connected successfully");

    // エージェントレジストリを初期化（DB付き）
    let registry = registry::AgentRegistry::with_database(db_pool)
        .await
        .expect("Failed to initialize agent registry");

    // アプリケーション状態を初期化
    let state = AppState { registry };

    // ルーター作成
    let app = api::create_router(state);

    // サーバー起動
    let host = std::env::var("COORDINATOR_HOST")
        .unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("COORDINATOR_PORT")
        .unwrap_or_else(|_| "8080".to_string());
    let bind_addr = format!("{}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    println!("Coordinator server listening on {}", bind_addr);

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
