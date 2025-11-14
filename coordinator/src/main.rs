//! Ollama Coordinator Server Entry Point

use ollama_coordinator_coordinator::{api, balancer, health, registry, tasks, AppState};

#[derive(Clone)]
struct ServerConfig {
    host: String,
    port: u16,
}

impl ServerConfig {
    fn from_env() -> Self {
        let host = std::env::var("COORDINATOR_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("COORDINATOR_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .unwrap_or(8080);
        Self { host, port }
    }

    fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl ServerConfig {
    fn local_host(&self) -> String {
        match self.host.as_str() {
            "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
            other => other.to_string(),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}:{}", self.local_host(), self.port)
    }

    fn dashboard_url(&self) -> String {
        format!("{}/dashboard", self.base_url())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn main() {
    use ollama_coordinator_coordinator::gui::tray::{run_with_system_tray, TrayOptions};
    use std::thread;
    use tokio::runtime::Builder;

    let config = ServerConfig::from_env();
    let tray_options = TrayOptions::new(&config.base_url(), &config.dashboard_url());

    run_with_system_tray(tray_options, move |proxy| {
        let server_config = config.clone();
        thread::spawn(move || {
            let runtime = Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build Tokio runtime for system tray mode");
            runtime.block_on(run_server(server_config));
            proxy.notify_server_exit();
        });
    });
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[tokio::main]
async fn main() {
    run_server(ServerConfig::from_env()).await;
}

async fn run_server(config: ServerConfig) {
    println!("Ollama Coordinator v{}", env!("CARGO_PKG_VERSION"));

    println!("Initializing storage at ~/.ollama-coordinator/");
    let registry = registry::AgentRegistry::with_storage()
        .await
        .expect("Failed to initialize agent registry");

    let load_manager = balancer::LoadManager::new(registry.clone());
    println!("Storage initialized successfully");

    let health_check_interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let agent_timeout_secs: u64 = std::env::var("AGENT_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let health_monitor = health::HealthMonitor::new(
        registry.clone(),
        health_check_interval_secs,
        agent_timeout_secs,
    );
    health_monitor.start();

    let load_balancer_mode =
        std::env::var("LOAD_BALANCER_MODE").unwrap_or_else(|_| "auto".to_string());
    println!("Load balancer mode: {}", load_balancer_mode);

    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new()
            .expect("Failed to initialize request history storage"),
    );
    ollama_coordinator_coordinator::db::request_history::start_cleanup_task(
        request_history.clone(),
    );

    let task_manager = tasks::DownloadTaskManager::new();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    };

    let app = api::create_router(state);

    let bind_addr = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    println!("Coordinator server listening on {}", bind_addr);

    axum::serve(listener, app).await.expect("Server error");
}
