//! LLM Router Server Entry Point

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use clap::Parser;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use llm_router::cli::{Cli, Commands};
use llm_router::config::{get_env_with_fallback_or, get_env_with_fallback_parse};
use llm_router::{api, auth, balancer, health, logging, registry, tasks, AppState};
use sqlx::sqlite::SqliteConnectOptions;
use std::net::SocketAddr;
use std::str::FromStr;
use tracing::info;

#[derive(Clone)]
struct ServerConfig {
    host: String,
    port: u16,
}

impl ServerConfig {
    fn from_env() -> Self {
        let host = get_env_with_fallback_or("LLM_ROUTER_HOST", "ROUTER_HOST", "0.0.0.0");
        let port = get_env_with_fallback_parse("LLM_ROUTER_PORT", "ROUTER_PORT", 8080);
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
    use llm_router::gui::tray::{run_with_system_tray, TrayOptions};
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
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::User { command }) => {
            // User commands don't need full logging
            handle_user_command(command).await;
        }
        None => {
            // No command = start server
            logging::init().expect("failed to initialize logging");
            run_server(ServerConfig::from_env()).await;
        }
    }
}

/// Handle user management CLI commands
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
async fn handle_user_command(command: llm_router::cli::user::UserCommand) {
    use llm_router::cli::user::UserCommand;
    use llm_router::db;
    use llm_router_common::auth::UserRole;

    // Initialize database
    let database_url = std::env::var("LLM_ROUTER_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .expect("Failed to get home directory");
            format!("sqlite:{}/.llm-router/router.db", home)
        });

    let db_pool = match init_db_pool(&database_url).await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("Error: Failed to connect to database: {}", e);
            std::process::exit(1);
        }
    };

    // Run migrations
    if let Err(e) = sqlx::migrate!("./migrations").run(&db_pool).await {
        eprintln!("Error: Failed to run database migrations: {}", e);
        std::process::exit(1);
    }

    match command {
        UserCommand::List => match db::users::list(&db_pool).await {
            Ok(users) => {
                if users.is_empty() {
                    println!("No users registered.");
                } else {
                    println!("{:<20} {:<10}", "USERNAME", "ROLE");
                    println!("{}", "-".repeat(30));
                    for user in users {
                        let role_str = match user.role {
                            UserRole::Admin => "admin",
                            UserRole::Viewer => "viewer",
                        };
                        println!("{:<20} {:<10}", user.username, role_str);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: Failed to list users: {}", e);
                std::process::exit(1);
            }
        },
        UserCommand::Add(add) => {
            // Validate password length
            if add.password.len() < 8 {
                eprintln!("Error: Password must be at least 8 characters.");
                std::process::exit(1);
            }

            // Hash password
            let password_hash = match auth::password::hash_password(&add.password) {
                Ok(hash) => hash,
                Err(e) => {
                    eprintln!("Error: Failed to hash password: {}", e);
                    std::process::exit(1);
                }
            };

            match db::users::create(&db_pool, &add.username, &password_hash, UserRole::Viewer).await
            {
                Ok(_) => {
                    println!("User '{}' created successfully.", add.username);
                }
                Err(e) => {
                    eprintln!("Error: Failed to create user: {}", e);
                    std::process::exit(1);
                }
            }
        }
        UserCommand::Delete(delete) => {
            // Find user by username first
            match db::users::find_by_username(&db_pool, &delete.username).await {
                Ok(Some(user)) => {
                    // Delete by ID
                    match db::users::delete(&db_pool, user.id).await {
                        Ok(()) => {
                            println!("User '{}' deleted successfully.", delete.username);
                        }
                        Err(e) => {
                            eprintln!("Error: Failed to delete user: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Ok(None) => {
                    eprintln!("Error: User '{}' not found.", delete.username);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Error: Failed to find user: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

async fn init_db_pool(database_url: &str) -> sqlx::Result<sqlx::SqlitePool> {
    // SQLiteファイルはディレクトリが存在しないと作成できないため、先に作成しておく
    if let Some(path) = database_url.strip_prefix("sqlite:") {
        // `sqlite::memory:` のような特殊指定はスキップ
        if !path.starts_with(':') {
            // `sqlite://` 形式に備えてスラッシュを除去し、クエリ部分を除外
            let normalized = path.trim_start_matches("//");
            let path_without_params = normalized.split('?').next().unwrap_or(normalized);
            let db_path = std::path::Path::new(path_without_params);
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

    let connect_options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

    sqlx::SqlitePool::connect_with(connect_options).await
}

async fn run_server(config: ServerConfig) {
    info!("LLM Router v{}", env!("CARGO_PKG_VERSION"));

    info!("Initializing storage at ~/.llm-router/");
    let registry = registry::NodeRegistry::with_storage()
        .await
        .expect("Failed to initialize node registry");

    let load_manager = balancer::LoadManager::new(registry.clone());
    info!("Storage initialized successfully");

    let health_check_interval_secs: u64 = get_env_with_fallback_parse(
        "LLM_ROUTER_HEALTH_CHECK_INTERVAL",
        "HEALTH_CHECK_INTERVAL",
        30,
    );
    let node_timeout_secs: u64 =
        get_env_with_fallback_parse("LLM_ROUTER_NODE_TIMEOUT", "NODE_TIMEOUT", 60);

    let health_monitor = health::HealthMonitor::new(
        registry.clone(),
        health_check_interval_secs,
        node_timeout_secs,
    );
    health_monitor.start();

    let load_balancer_mode = get_env_with_fallback_or(
        "LLM_ROUTER_LOAD_BALANCER_MODE",
        "LOAD_BALANCER_MODE",
        "auto",
    );
    info!("Load balancer mode: {}", load_balancer_mode);

    let request_history = std::sync::Arc::new(
        llm_router::db::request_history::RequestHistoryStorage::new()
            .expect("Failed to initialize request history storage"),
    );
    llm_router::db::request_history::start_cleanup_task(request_history.clone());

    let task_manager = tasks::DownloadTaskManager::new();

    // 認証システムを初期化
    // データベース接続プールを作成
    let database_url =
        llm_router::config::get_env_with_fallback("LLM_ROUTER_DATABASE_URL", "DATABASE_URL")
            .unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .expect("Failed to get home directory");
                format!("sqlite:{}/.llm-router/router.db", home)
            });

    let db_pool = init_db_pool(&database_url)
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

    // JWT秘密鍵を取得または生成（ファイル永続化対応）
    let jwt_secret = llm_router::jwt_secret::get_or_create_jwt_secret()
        .expect("Failed to get or create JWT secret");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_db_pool_creates_sqlite_file_when_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("router.db");
        let db_url = format!("sqlite:{}", db_path.display());

        assert!(
            !db_path.exists(),
            "database file should not exist before initialization"
        );

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should create missing sqlite file");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed after initialization");

        assert!(
            db_path.exists(),
            "database file should be created by init_db_pool"
        );
    }
}
