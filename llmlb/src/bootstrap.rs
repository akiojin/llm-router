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
        update_manager
            .set_tray_proxy(std::sync::Arc::new(proxy))
            .await;
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
    crate::audit::start_periodic_verification_task(audit_log_storage.clone());

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
        let retention_days: i64 = std::env::var("LLMLB_AUDIT_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(90);
        crate::audit::start_archive_task(
            audit_log_storage.clone(),
            archive_pool.clone(),
            retention_days,
        );
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
mod tests;
