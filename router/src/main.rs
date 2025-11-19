//! Ollama Router Server Entry Point

use or_router::{api, auth, balancer, health, logging, registry, tasks, AppState};
use std::net::SocketAddr;
use tracing::info;

#[derive(Clone)]
struct ServerConfig {
    host: String,
    port: u16,
}

impl ServerConfig {
    fn from_env() -> Self {
        let host = std::env::var("ROUTER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("ROUTER_PORT")
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
    logging::init().expect("failed to initialize logging");
    use or_router::gui::tray::{run_with_system_tray, TrayOptions};
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
    logging::init().expect("failed to initialize logging");
    run_server(ServerConfig::from_env()).await;
}

async fn run_server(config: ServerConfig) {
    info!("Ollama Router v{}", env!("CARGO_PKG_VERSION"));

    info!("Initializing storage at ~/.or/");
    let registry = registry::NodeRegistry::with_storage()
        .await
        .expect("Failed to initialize node registry");

    let load_manager = balancer::LoadManager::new(registry.clone());
    info!("Storage initialized successfully");

    let health_check_interval_secs: u64 = std::env::var("HEALTH_CHECK_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let node_timeout_secs: u64 = std::env::var("NODE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let health_monitor = health::HealthMonitor::new(
        registry.clone(),
        health_check_interval_secs,
        node_timeout_secs,
    );
    health_monitor.start();

    let load_balancer_mode =
        std::env::var("LOAD_BALANCER_MODE").unwrap_or_else(|_| "auto".to_string());
    info!("Load balancer mode: {}", load_balancer_mode);

    let request_history = std::sync::Arc::new(
        or_router::db::request_history::RequestHistoryStorage::new()
            .expect("Failed to initialize request history storage"),
    );
    or_router::db::request_history::start_cleanup_task(request_history.clone());

    let task_manager = tasks::DownloadTaskManager::new();

    // 認証システムを初期化
    // データベース接続プールを作成
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .expect("Failed to get home directory");
        format!("sqlite:{}/.or/router.db", home)
    });

    // SQLiteファイルはディレクトリが存在しないと作成できないため、先に作成しておく
    if let Some(path) = database_url.strip_prefix("sqlite:") {
        // `sqlite::memory:` のような特殊指定はスキップ
        if !path.starts_with(':') {
            // `sqlite://` 形式に備えてスラッシュを除去
            let normalized = path.trim_start_matches("//");
            let db_path = std::path::Path::new(normalized);
            if let Some(parent) = db_path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    panic!(
                        "Failed to create database directory {}: {}",
                        parent.display(),
                        err
                    );
                }
            }
        }
    }

    let db_pool = sqlx::SqlitePool::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // マイグレーションを実行
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run database migrations");

    // 管理者が存在しない場合は作成
    auth::bootstrap::ensure_admin_exists(&db_pool)
        .await
        .expect("Failed to ensure admin exists");

    // JWT秘密鍵を取得または生成
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        tracing::warn!("JWT_SECRET not set, using default (not recommended for production)");
        "default-jwt-secret-change-in-production".to_string()
    });

    info!("Authentication system initialized");

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };

    let router = api::create_router(state);

    let bind_addr = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind to address");

    info!("Router server listening on {}", bind_addr);

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("Server error");
}
