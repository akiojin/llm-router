//! Ollama Coordinator Server Entry Point

use ollama_coordinator_coordinator::{api, balancer, health, registry, AppState};

#[tokio::main]
async fn main() {
    println!("Ollama Coordinator v{}", env!("CARGO_PKG_VERSION"));

    // ストレージディレクトリを初期化
    println!("Initializing storage at ~/.ollama-coordinator/");

    // エージェントレジストリを初期化（JSON file storage）
    let registry = registry::AgentRegistry::with_storage()
        .await
        .expect("Failed to initialize agent registry");

    // ロードマネージャー初期化
    let load_manager = balancer::LoadManager::new(registry.clone());

    println!("Storage initialized successfully");

    // ヘルスチェック設定
    let health_check_interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let agent_timeout_secs: u64 = std::env::var("AGENT_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    // ヘルスモニター起動
    let health_monitor = health::HealthMonitor::new(
        registry.clone(),
        health_check_interval_secs,
        agent_timeout_secs,
    );
    health_monitor.start();

    // アプリケーション状態を初期化
    let state = AppState {
        registry,
        load_manager,
    };

    // ルーター作成
    let app = api::create_router(state);

    // サーバー起動
    let host = std::env::var("COORDINATOR_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("COORDINATOR_PORT").unwrap_or_else(|_| "8080".to_string());
    let bind_addr = format!("{}:{}", host, port);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    println!("Coordinator server listening on {}", bind_addr);

    axum::serve(listener, app).await.expect("Server error");
}
