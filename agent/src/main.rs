//! Ollama Coordinator Agent Entry Point

use ollama_coordinator_agent::{ollama::OllamaManager, client::CoordinatorClient, metrics::MetricsCollector};
use ollama_coordinator_common::protocol::{RegisterRequest, HealthCheckRequest};
use tokio::time::{interval, Duration};

#[tokio::main]
async fn main() {
    println!("Ollama Coordinator Agent v{}", env!("CARGO_PKG_VERSION"));

    // 設定（将来的には設定ファイルから読み込む）
    let coordinator_url = std::env::var("COORDINATOR_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());
    let ollama_port = std::env::var("OLLAMA_PORT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(11434);
    let heartbeat_interval_secs = 10;

    // Ollamaマネージャーを初期化
    let mut ollama_manager = OllamaManager::new(ollama_port);

    println!("Ensuring Ollama is running...");
    if let Err(e) = ollama_manager.ensure_running().await {
        eprintln!("Failed to start Ollama: {}", e);
        return;
    }

    // マシン情報を取得
    let machine_name = whoami::devicename();
    let ip_address = get_local_ip().unwrap_or_else(|| "127.0.0.1".parse().unwrap());
    let ollama_version = ollama_manager.get_version().await.unwrap_or_else(|_| "unknown".to_string());

    println!("Machine: {}", machine_name);
    println!("IP: {}", ip_address);
    println!("Ollama version: {}", ollama_version);

    // Coordinatorクライアントを初期化
    let mut coordinator_client = CoordinatorClient::new(coordinator_url);

    // エージェント登録
    let register_req = RegisterRequest {
        machine_name: machine_name.clone(),
        ip_address,
        ollama_version,
        ollama_port,
    };

    println!("Registering with Coordinator...");
    let register_response = match coordinator_client.register(register_req).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Failed to register with Coordinator: {}", e);
            return;
        }
    };

    let agent_id = register_response.agent_id;
    println!("Registered successfully! Agent ID: {}", agent_id);

    // メトリクスコレクターを初期化
    let mut metrics_collector = MetricsCollector::new();

    // ハートビート送信タスク
    let mut heartbeat_timer = interval(Duration::from_secs(heartbeat_interval_secs));

    println!("Starting heartbeat loop...");
    loop {
        heartbeat_timer.tick().await;

        // メトリクス収集
        let (cpu_usage, memory_usage) = match metrics_collector.collect_metrics() {
            Ok(metrics) => metrics,
            Err(e) => {
                eprintln!("Failed to collect metrics: {}", e);
                continue;
            }
        };

        // ハートビート送信
        let heartbeat_req = HealthCheckRequest {
            agent_id,
            cpu_usage,
            memory_usage,
            active_requests: 0, // TODO: 実際のリクエスト数をカウント
        };

        if let Err(e) = coordinator_client.send_heartbeat(heartbeat_req).await {
            eprintln!("Failed to send heartbeat: {}", e);
        } else {
            println!(
                "Heartbeat sent - CPU: {:.1}%, Memory: {:.1}%",
                cpu_usage, memory_usage
            );
        }
    }
}

/// ローカルIPアドレスを取得
fn get_local_ip() -> Option<std::net::IpAddr> {
    use std::net::{IpAddr, UdpSocket};

    // ダミーのUDP接続を作成してローカルIPを取得
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;

    Some(addr.ip())
}
