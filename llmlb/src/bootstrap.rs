//! サーバー初期化ロジック
//!
//! データベース接続、レジストリ初期化、ヘルスチェッカー起動など
//! サーバー起動に必要なコンポーネントの初期化を担当する。

use crate::config::get_env_with_fallback_parse;
use crate::lock::ServerLock;
use crate::{auth, balancer, health, sync, AppState};
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;
use tracing::{info, warn};

/// サーバー初期化結果
///
/// `AppState` とサーバーロックをまとめて返す。
/// `_server_lock` はサーバープロセス終了まで保持する必要がある。
pub struct InitContext {
    /// アプリケーション状態
    pub state: AppState,
    /// サーバーロック（Dropでロック解除）
    pub _server_lock: ServerLock,
}

/// サーバー初期化を実行する
///
/// DB接続、マイグレーション、レジストリ初期化、ヘルスチェッカー起動など
/// サーバー起動に必要な全コンポーネントを初期化し、`InitContext` を返す。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub async fn initialize(
    port: u16,
    tray_proxy: Option<crate::gui::tray::TrayEventProxy>,
) -> InitContext {
    initialize_inner(port, tray_proxy).await
}

/// サーバー初期化を実行する（Linux版）
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub async fn initialize(port: u16) -> InitContext {
    initialize_inner(port).await
}

async fn initialize_inner(
    port: u16,
    #[cfg(any(target_os = "windows", target_os = "macos"))] tray_proxy: Option<
        crate::gui::tray::TrayEventProxy,
    >,
) -> InitContext {
    info!("LLM Load Balancer v{}", env!("CARGO_PKG_VERSION"));
    maybe_raise_nofile_limit();

    // シングル実行制約: ロックを取得
    let server_lock = match ServerLock::acquire(port) {
        Ok(lock) => {
            info!("Lock acquired for port {} (PID: {})", port, lock.info().pid);
            lock
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // データベース接続プールを最初に作成（他コンポーネントが依存）
    let database_url = crate::config::get_env_with_fallback("LLMLB_DATABASE_URL", "DATABASE_URL")
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .expect("Failed to get home directory");
            format!("sqlite:{}/.llmlb/load balancer.db", home)
        });

    let db_pool = init_db_pool(&database_url)
        .await
        .expect("Failed to connect to database");

    // 起動時も共通の互換レイヤー経由でマイグレーションを実行する
    crate::db::migrations::run_migrations(&db_pool)
        .await
        .expect("Failed to run database migrations");

    info!("Initializing storage at ~/.llmlb/");

    // エンドポイントレジストリを初期化
    let endpoint_registry = crate::registry::endpoints::EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to initialize endpoint registry");
    let endpoint_registry_arc = std::sync::Arc::new(endpoint_registry.clone());

    // LoadManagerをEndpointRegistryで初期化
    let load_manager = balancer::LoadManager::new(endpoint_registry_arc.clone());
    info!("Storage initialized successfully");

    // HTTPクライアント（接続プーリング有効）を作成
    let http_client = reqwest::Client::builder()
        // 接続確立のタイムアウト（応答しないエンドポイントで無期限ハングするのを防ぐ）。
        // ストリーミングを壊さないよう全体のリクエストタイムアウトは設けない。
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(std::time::Duration::from_secs(60))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    // サーバー起動時のエンドポイントタイプ再検出
    redetect_all_endpoints(&endpoint_registry, &http_client).await;
    spawn_startup_model_backfill(endpoint_registry.clone(), http_client.clone());

    let health_check_interval_secs: u64 =
        get_env_with_fallback_parse("LLMLB_HEALTH_CHECK_INTERVAL", "HEALTH_CHECK_INTERVAL", 30);

    // エンドポイントヘルスチェッカーをバックグラウンドで開始
    let endpoint_health_checker = health::EndpointHealthChecker::new(endpoint_registry.clone())
        .with_load_manager(load_manager.clone())
        .with_interval(health_check_interval_secs);
    endpoint_health_checker.start();

    // リクエスト履歴ストレージを初期化（SQLite使用）
    let request_history = std::sync::Arc::new(
        crate::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    if let Err(err) = request_history.import_legacy_json_if_present().await {
        tracing::warn!("Failed to import legacy request history: {}", err);
    }
    // 起動時にDBからリクエスト履歴をseed（直近60分）
    match request_history.get_recent_history_by_minute(60).await {
        Ok(points) => {
            if !points.is_empty() {
                info!(
                    count = points.len(),
                    "Seeding request history from database"
                );
                load_manager.seed_history_from_db(points).await;
            }
        }
        Err(e) => {
            warn!("Failed to seed request history from database: {}", e);
        }
    }

    // 起動時にDBからTPS状態をseed（当日分）
    {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        match crate::db::endpoint_daily_stats::get_today_stats_all(&db_pool, &today).await {
            Ok(entries) => {
                if !entries.is_empty() {
                    info!(
                        count = entries.len(),
                        "Seeding TPS tracker from daily stats"
                    );
                    load_manager.seed_tps_from_db(entries).await;
                }
            }
            Err(e) => {
                warn!("Failed to seed TPS tracker from daily stats: {}", e);
            }
        }
    }

    crate::db::request_history::start_cleanup_task(request_history.clone());
    crate::db::endpoint_daily_stats::start_daily_stats_task(db_pool.clone());

    // 管理者が存在しない場合は作成
    auth::bootstrap::ensure_admin_exists(&db_pool)
        .await
        .expect("Failed to ensure admin exists");

    // JWT秘密鍵を取得または生成（ファイル永続化対応）
    let jwt_secret =
        crate::jwt_secret::get_or_create_jwt_secret().expect("Failed to get or create JWT secret");

    info!("Authentication system initialized");

    // Self-update components
    let inference_gate = crate::inference_gate::InferenceGate::default();
    let shutdown = crate::shutdown::ShutdownController::default();
    let update_manager = crate::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to initialize update manager");

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let mgr = update_manager.clone();
        crate::gui::tray::set_update_apply_handler(move || mgr.request_apply());
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if let Some(proxy) = tray_proxy {
        update_manager.set_tray_proxy(proxy).await;
    }
    update_manager.start_background_tasks();

    info!(
        "Endpoint registry initialized with {} endpoints",
        endpoint_registry.count().await
    );

    // 監査ログシステムの初期化 (SPEC-8301d106)
    let audit_log_storage =
        std::sync::Arc::new(crate::db::audit_log::AuditLogStorage::new(db_pool.clone()));
    let audit_log_writer = crate::audit::writer::AuditLogWriter::new(
        crate::db::audit_log::AuditLogStorage::new(db_pool.clone()),
        crate::audit::writer::AuditLogWriterConfig::default(),
    );
    info!("Audit log system initialized");

    // 起動時ハッシュチェーン検証 (SPEC-8301d106)
    {
        let storage_ref = &*audit_log_storage;
        match crate::audit::hash_chain::verify_chain(storage_ref).await {
            Ok(result) => {
                if result.valid {
                    info!(
                        batches_checked = result.batches_checked,
                        "Audit log hash chain verification passed"
                    );
                } else {
                    warn!(
                        tampered_batch = ?result.tampered_batch,
                        message = ?result.message,
                        "Audit log hash chain verification FAILED - tampering detected"
                    );
                }
            }
            Err(e) => {
                warn!("Audit log hash chain verification error: {}", e);
            }
        }
    }

    // 24時間ごとの定期ハッシュチェーン検証タスク (SPEC-8301d106)
    {
        let periodic_storage = audit_log_storage.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
            // 最初のtickはスキップ（起動時検証は上で実施済み）
            interval.tick().await;
            loop {
                interval.tick().await;
                match crate::audit::hash_chain::verify_chain(&periodic_storage).await {
                    Ok(result) => {
                        if result.valid {
                            info!(
                                batches_checked = result.batches_checked,
                                "Periodic audit log hash chain verification passed"
                            );
                        } else {
                            warn!(
                                tampered_batch = ?result.tampered_batch,
                                message = ?result.message,
                                "Periodic audit log hash chain verification FAILED"
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Periodic audit log hash chain verification error: {}", e);
                    }
                }
            }
        });
    }

    // アーカイブDBプールの初期化 (SPEC-8301d106)
    let archive_path = std::env::var("LLMLB_AUDIT_ARCHIVE_PATH").unwrap_or_else(|_| {
        let db_path =
            std::env::var("LLMLB_DB_PATH").unwrap_or_else(|_| "load_balancer.db".to_string());
        let parent = std::path::Path::new(&db_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        parent
            .join("audit_archive.db")
            .to_string_lossy()
            .to_string()
    });
    let audit_archive_pool = match crate::db::audit_log::create_archive_pool(&archive_path).await {
        Ok(pool) => {
            info!(path = %archive_path, "Audit archive DB initialized");
            Some(pool)
        }
        Err(e) => {
            warn!("Failed to initialize audit archive DB: {}", e);
            None
        }
    };

    // 24時間ごとの定期アーカイブタスク (SPEC-8301d106)
    if let Some(ref archive_pool) = audit_archive_pool {
        let archive_storage = audit_log_storage.clone();
        let archive_pool_clone = archive_pool.clone();
        let retention_days: i64 = std::env::var("LLMLB_AUDIT_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(90);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
            interval.tick().await; // 最初のtickをスキップ
            loop {
                interval.tick().await;
                match archive_storage
                    .archive_old_entries(retention_days, &archive_pool_clone)
                    .await
                {
                    Ok(count) => {
                        if count > 0 {
                            info!(count, retention_days, "Archived old audit log entries");
                        }
                    }
                    Err(e) => {
                        warn!("Audit log archive task error: {}", e);
                    }
                }
            }
        });
    }

    let state = AppState {
        load_manager,
        request_history,
        db_pool,
        jwt_secret,
        http_client,
        event_bus: {
            let bus = crate::events::create_shared_event_bus();
            update_manager.set_event_bus(bus.clone());
            bus
        },
        endpoint_registry,
        inference_gate,
        shutdown: shutdown.clone(),
        update_manager,
        audit_log_writer,
        audit_log_storage,
        audit_archive_pool,
    };

    InitContext {
        state,
        _server_lock: server_lock,
    }
}

/// SQLite接続プールを初期化する
pub async fn init_db_pool(database_url: &str) -> sqlx::Result<sqlx::SqlitePool> {
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

fn maybe_raise_nofile_limit() {
    #[cfg(unix)]
    {
        use std::cmp::min;

        // macOS では launchd 起動時に open files 上限が 256 など低く設定されることがあり、
        // 受け付け不能 (EMFILE) や SQLite の open 失敗 (SQLITE_CANTOPEN) につながる。
        const DESIRED_NOFILE: libc::rlim_t = 65_536;

        unsafe {
            let mut current = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut current) != 0 {
                tracing::debug!(
                    "getrlimit(RLIMIT_NOFILE) failed: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }

            let target = min(DESIRED_NOFILE, current.rlim_max);
            if current.rlim_cur >= target {
                return;
            }

            let updated = libc::rlimit {
                rlim_cur: target,
                rlim_max: current.rlim_max,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &updated) != 0 {
                tracing::warn!(
                    "Failed to raise RLIMIT_NOFILE from {} to {}: {}",
                    current.rlim_cur,
                    target,
                    std::io::Error::last_os_error()
                );
                return;
            }

            tracing::info!(
                "Raised RLIMIT_NOFILE from {} to {}",
                current.rlim_cur,
                target
            );
        }
    }
}

/// サーバー起動時に全エンドポイントのタイプを再検出する
///
/// 前回起動時から変更されている可能性があるため、登録済みの全エンドポイントに対して
/// タイプ検出を実行し、変更があれば更新する。
/// 検出失敗時は既存設定を保持し、ヘルスチェックでの再評価に委ねる。
async fn redetect_all_endpoints(
    registry: &crate::registry::endpoints::EndpointRegistry,
    http_client: &reqwest::Client,
) {
    use crate::detection::detect_endpoint_type_with_client;

    const STARTUP_REDETECTION_TIMEOUT_SECS: u64 = 10;

    let endpoints = registry.list().await;
    let total = endpoints.len();

    if total == 0 {
        info!("No endpoints registered; skipping startup re-detection");
        return;
    }

    info!(
        total = total,
        "Starting endpoint type re-detection on server startup"
    );

    let mut failed: usize = 0;
    let mut updated: usize = 0;

    for ep in &endpoints {
        match tokio::time::timeout(
            std::time::Duration::from_secs(STARTUP_REDETECTION_TIMEOUT_SECS),
            detect_endpoint_type_with_client(http_client, &ep.base_url, ep.api_key.as_deref()),
        )
        .await
        {
            Ok(Ok(result)) => {
                if result.endpoint_type == ep.endpoint_type {
                    continue;
                }
                if registry
                    .update_endpoint_type(ep.id, result.endpoint_type)
                    .await
                    .is_ok()
                {
                    info!(
                        endpoint_id = %ep.id,
                        name = %ep.name,
                        old_type = ?ep.endpoint_type,
                        new_type = ?result.endpoint_type,
                        "Endpoint type changed during re-detection"
                    );
                    updated += 1;
                }
            }
            Ok(Err(err)) => {
                warn!(
                    endpoint_id = %ep.id,
                    name = %ep.name,
                    error = %err,
                    "Endpoint type re-detection failed on startup; keeping existing configuration"
                );
                failed += 1;
            }
            Err(_) => {
                warn!(
                    endpoint_id = %ep.id,
                    name = %ep.name,
                    timeout_secs = STARTUP_REDETECTION_TIMEOUT_SECS,
                    "Endpoint type re-detection timed out on startup; keeping existing configuration"
                );
                failed += 1;
            }
        }
    }

    info!(total, failed, updated, "Endpoint re-detection complete");
}

fn spawn_startup_model_backfill(
    registry: crate::registry::endpoints::EndpointRegistry,
    http_client: reqwest::Client,
) {
    tokio::spawn(async move {
        let endpoints = registry.list_online().await;
        if endpoints.is_empty() {
            info!("No online endpoints found; skipping startup model backfill");
            return;
        }

        info!(
            total = endpoints.len(),
            "Starting startup model backfill for online endpoints"
        );

        let mut succeeded = 0usize;
        let mut failed = 0usize;

        for ep in endpoints {
            match sync::sync_models_with_type(
                registry.pool(),
                &http_client,
                ep.id,
                &ep.base_url,
                ep.api_key.as_deref(),
                ep.inference_timeout_secs as u64,
                Some(ep.endpoint_type),
            )
            .await
            {
                Ok(result) => match registry.refresh_model_mappings(ep.id).await {
                    Ok(()) => {
                        succeeded += 1;
                        info!(
                            endpoint_id = %ep.id,
                            endpoint_name = %ep.name,
                            added = result.added,
                            removed = result.removed,
                            updated = result.updated,
                            "Startup model backfill completed"
                        );
                    }
                    Err(e) => {
                        failed += 1;
                        warn!(
                            endpoint_id = %ep.id,
                            endpoint_name = %ep.name,
                            error = %e,
                            "Startup model backfill failed: refresh model mappings error"
                        );
                    }
                },
                Err(e) => {
                    failed += 1;
                    warn!(
                        endpoint_id = %ep.id,
                        endpoint_name = %ep.name,
                        error = %e,
                        "Startup model backfill failed"
                    );
                }
            }
        }

        info!(
            total = succeeded + failed,
            succeeded, failed, "Startup model backfill complete"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_db_pool_creates_sqlite_file_when_missing() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("load balancer.db");
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

    // =======================================================================
    // init_db_pool: in-memory SQLite
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_in_memory() {
        let pool = init_db_pool("sqlite::memory:")
            .await
            .expect("in-memory sqlite should succeed");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed on in-memory db");
    }

    // =======================================================================
    // init_db_pool: creates nested parent directories
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_creates_nested_directories() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let nested_path = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("test.db");
        let db_url = format!("sqlite:{}", nested_path.display());

        assert!(
            !nested_path.exists(),
            "database file should not exist before init"
        );

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should create nested directories");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed");

        assert!(nested_path.exists());
    }

    // =======================================================================
    // init_db_pool: path with spaces in directory name
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_handles_spaces_in_path() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("dir with spaces").join("test db.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should handle spaces in path");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed");

        assert!(db_path.exists());
    }

    // =======================================================================
    // init_db_pool: sqlite:// (double slash) prefix
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_double_slash_prefix() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("double_slash.db");
        let db_url = format!("sqlite://{}", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should handle sqlite:// prefix");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed");
    }

    // =======================================================================
    // init_db_pool: existing file is reused
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_reuses_existing_file() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("existing.db");
        let db_url = format!("sqlite:{}", db_path.display());

        // First pool creates the file
        let pool1 = init_db_pool(&db_url)
            .await
            .expect("first init should succeed");
        sqlx::query("CREATE TABLE test_reuse (id INTEGER PRIMARY KEY)")
            .execute(&pool1)
            .await
            .expect("table creation should succeed");
        drop(pool1);

        // Second pool should reuse the existing file and see the table
        let pool2 = init_db_pool(&db_url)
            .await
            .expect("second init should succeed");
        sqlx::query("INSERT INTO test_reuse (id) VALUES (1)")
            .execute(&pool2)
            .await
            .expect("insert into existing table should succeed");
    }

    // =======================================================================
    // init_db_pool: URL with query parameters
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_with_query_params() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("parameterized.db");
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should handle query params");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed");
    }

    // =======================================================================
    // InitContext struct layout
    // =======================================================================
    #[test]
    fn init_context_has_expected_fields() {
        // Compile-time check: InitContext has state and _server_lock fields
        // This test verifies the struct definition by referencing the fields.
        fn _assert_fields(ctx: &InitContext) {
            let _state = &ctx.state;
            let _lock = &ctx._server_lock;
        }
    }

    // =======================================================================
    // maybe_raise_nofile_limit: just ensure no panic
    // =======================================================================
    #[test]
    fn maybe_raise_nofile_limit_does_not_panic() {
        // This function is no-op on non-Unix or if limits are already high
        maybe_raise_nofile_limit();
    }

    // =======================================================================
    // init_db_pool: multiple concurrent pools on same db
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_concurrent_pools() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("concurrent.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let pool1 = init_db_pool(&db_url)
            .await
            .expect("first pool should succeed");

        let pool2 = init_db_pool(&db_url)
            .await
            .expect("second pool should succeed");

        // Both pools should be able to execute queries
        sqlx::query("SELECT 1")
            .execute(&pool1)
            .await
            .expect("query on pool1 should succeed");
        sqlx::query("SELECT 1")
            .execute(&pool2)
            .await
            .expect("query on pool2 should succeed");
    }

    // =======================================================================
    // init_db_pool: special characters in filename
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_special_characters_in_filename() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("test-db_v2.1.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should handle special characters");

        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("basic query should succeed");

        assert!(db_path.exists());
    }

    // =======================================================================
    // init_db_pool: basic SQL operations
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_supports_basic_operations() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("operations.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should succeed");

        // CREATE
        sqlx::query("CREATE TABLE ops_test (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("CREATE TABLE should succeed");

        // INSERT
        sqlx::query("INSERT INTO ops_test (id, name) VALUES (1, 'test')")
            .execute(&pool)
            .await
            .expect("INSERT should succeed");

        // SELECT
        let row: (i64, String) = sqlx::query_as("SELECT id, name FROM ops_test WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("SELECT should succeed");
        assert_eq!(row.0, 1);
        assert_eq!(row.1, "test");

        // UPDATE
        sqlx::query("UPDATE ops_test SET name = 'updated' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("UPDATE should succeed");

        // DELETE
        sqlx::query("DELETE FROM ops_test WHERE id = 1")
            .execute(&pool)
            .await
            .expect("DELETE should succeed");
    }

    // =======================================================================
    // init_db_pool: WAL mode
    // =======================================================================
    #[tokio::test]
    async fn init_db_pool_default_journal_mode() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("journal_test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let pool = init_db_pool(&db_url)
            .await
            .expect("init_db_pool should succeed");

        // Just verify the pool is functional
        let row: (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("PRAGMA should succeed");
        // journal_mode could be "wal", "delete", etc. depending on SQLite default
        assert!(!row.0.is_empty(), "journal mode should be non-empty");
    }
}
