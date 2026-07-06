//! Self-update manager.
//!
//! This module implements:
//! - Update discovery via GitHub Releases
//! - Background download of the preferred payload for the current platform
//! - User-approved apply flow: drain inference requests, then restart into the new version
//! - Internal helper modes (`__internal`) to safely replace binaries / run installers
//! - Update scheduling (immediate / idle / time-based)
//! - Update history recording

mod apply;
mod cache;
mod check;
mod download;
mod dto;
mod github;
mod helper;
pub mod history;
#[cfg(target_os = "macos")]
mod macos_installer;
mod payload;
mod platform;
pub mod schedule;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod tray;
use cache::load_cache;
#[cfg(test)]
use cache::{save_cache, UpdateCacheFile};
#[cfg(test)]
use download::{asset_name_from_url, extract_archive, find_extracted_binary};
pub use dto::*;
#[cfg(test)]
use github::{parse_tag_to_version, GitHubAsset, GitHubRelease};
#[cfg(test)]
use helper::{
    detect_server_port, parse_port_from_args, record_auto_rollback_history, RestartArgsFile,
};
pub(crate) use helper::{internal_apply_update, internal_rollback, internal_run_installer};
use helper::{spawn_internal_rollback, write_restart_args_file};
#[cfg(test)]
use platform::{choose_apply_plan, is_dir_writable, select_assets, ApplyPlan, Platform};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use tray::notify_tray_failed;

/// 自己更新の状態を UI 層へ通知する抽象。
///
/// arch-review [M8]: update ドメインが `gui::tray::TrayEventProxy` へ直接依存して
/// いたため、通知先を trait として逆転させた（依存方向は gui → update）。
/// gui 側が本 trait を `TrayEventProxy` に実装する。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub trait UpdateNotifier: Send + Sync {
    /// 新しいバージョンが利用可能。
    fn notify_update_available(&self, latest: String);
    /// 更新ペイロードのダウンロードが完了し適用可能。
    fn notify_update_ready(&self);
    /// 更新フローが失敗。
    fn notify_update_failed(&self, message: String);
    /// 既に最新。
    fn notify_update_up_to_date(&self);
    /// 現在の更新スケジュール（`None` で表示クリア）。
    fn notify_schedule(&self, schedule: Option<schedule::UpdateSchedule>);
}

use crate::{inference_gate::InferenceGate, shutdown::ShutdownController};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use semver::Version;
#[cfg(test)]
use std::fs;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::sync::{Notify, RwLock};

/// Minimum interval between manual update checks (seconds).
const MANUAL_CHECK_COOLDOWN_SECS: u64 = 60;

/// Default drain timeout for normal update apply (seconds).
const DEFAULT_DRAIN_TIMEOUT_SECS: u64 = 300;
/// Default HTTP listen port for `llmlb serve`.
const DEFAULT_LISTEN_PORT: u16 = 32768;

const DEFAULT_OWNER: &str = "akiojin";
const DEFAULT_REPO: &str = "llmlb";
const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Clone)]
/// Self-update manager.
///
/// This component checks GitHub Releases in the background, prepares the preferred payload,
/// and applies the update after explicit user approval (drain inference requests, then restart).
pub struct UpdateManager {
    inner: Arc<UpdateManagerInner>,
}

struct UpdateManagerInner {
    started: AtomicBool,
    apply_request_mode: AtomicU8,
    apply_notify: Notify,

    current_version: Version,
    http_client: reqwest::Client,
    gate: InferenceGate,
    shutdown: ShutdownController,

    owner: String,
    repo: String,
    ttl: Duration,

    /// Override for GitHub API base URL (for testing).
    github_api_base_url: Option<String>,

    cache_path: PathBuf,
    updates_dir: PathBuf,

    state: RwLock<UpdateState>,

    /// Rate-limit: last time a manual check was performed.
    last_manual_check: Mutex<Option<tokio::time::Instant>>,

    /// ダッシュボードイベントバス（状態遷移時にUpdateStateChangedを発行）
    event_bus: OnceLock<crate::events::SharedEventBus>,

    /// Schedule persistence.
    schedule_store: schedule::ScheduleStore,
    /// History persistence.
    history_store: history::HistoryStore,

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    tray_proxy: RwLock<Option<Arc<dyn UpdateNotifier>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum ApplyRequestMode {
    None = 0,
    Normal = 1,
    Force = 2,
}

impl ApplyRequestMode {
    fn from_u8(value: u8) -> Self {
        match value {
            x if x == Self::Normal as u8 => Self::Normal,
            x if x == Self::Force as u8 => Self::Force,
            _ => Self::None,
        }
    }
}

impl std::fmt::Debug for UpdateManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateManager").finish()
    }
}

impl UpdateManager {
    /// Create a new update manager for the current running version.
    ///
    /// This does not start background tasks; call [`UpdateManager::start_background_tasks`].
    pub fn new(
        http_client: reqwest::Client,
        gate: InferenceGate,
        shutdown: ShutdownController,
    ) -> Result<Self> {
        Self::new_with_config(
            http_client,
            gate,
            shutdown,
            DEFAULT_OWNER.to_string(),
            DEFAULT_REPO.to_string(),
            None,
        )
    }

    /// Create a new update manager with custom owner/repo and optional API base URL.
    ///
    /// `github_api_base_url` overrides the GitHub API base URL (useful for tests with wiremock).
    pub fn new_with_config(
        http_client: reqwest::Client,
        gate: InferenceGate,
        shutdown: ShutdownController,
        owner: String,
        repo: String,
        github_api_base_url: Option<String>,
    ) -> Result<Self> {
        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .context("Failed to parse CARGO_PKG_VERSION as semver")?;

        let (cache_path, updates_dir) = default_paths()?;
        let data_dir = cache_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        Ok(Self {
            inner: Arc::new(UpdateManagerInner {
                started: AtomicBool::new(false),
                apply_request_mode: AtomicU8::new(ApplyRequestMode::None as u8),
                apply_notify: Notify::new(),
                current_version,
                http_client,
                gate,
                shutdown,
                owner,
                repo,
                ttl: DEFAULT_TTL,
                github_api_base_url,
                cache_path,
                updates_dir,
                state: RwLock::new(UpdateState::UpToDate { checked_at: None }),
                last_manual_check: Mutex::new(None),
                event_bus: OnceLock::new(),
                schedule_store: schedule::ScheduleStore::new(&data_dir),
                history_store: history::HistoryStore::new(&data_dir),
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                tray_proxy: RwLock::new(None),
            }),
        })
    }

    /// Create an `UpdateManager` with an explicit data directory (test-only).
    ///
    /// This avoids reading `LLMLB_DATA_DIR` from the environment, eliminating
    /// race conditions when tests run in parallel.
    #[cfg(test)]
    fn new_with_data_dir(
        http_client: reqwest::Client,
        gate: InferenceGate,
        shutdown: ShutdownController,
        data_dir: &Path,
    ) -> Result<Self> {
        Self::new_with_data_dir_and_config(http_client, gate, shutdown, data_dir, None)
    }

    #[cfg(test)]
    fn new_with_data_dir_and_config(
        http_client: reqwest::Client,
        gate: InferenceGate,
        shutdown: ShutdownController,
        data_dir: &Path,
        github_api_base_url: Option<String>,
    ) -> Result<Self> {
        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .context("Failed to parse CARGO_PKG_VERSION as semver")?;

        let cache_path = data_dir.join("update-check.json");
        let updates_dir = data_dir.join("updates");

        Ok(Self {
            inner: Arc::new(UpdateManagerInner {
                started: AtomicBool::new(false),
                apply_request_mode: AtomicU8::new(ApplyRequestMode::None as u8),
                apply_notify: Notify::new(),
                current_version,
                http_client,
                gate,
                shutdown,
                owner: DEFAULT_OWNER.to_string(),
                repo: DEFAULT_REPO.to_string(),
                ttl: DEFAULT_TTL,
                github_api_base_url,
                cache_path,
                updates_dir,
                state: RwLock::new(UpdateState::UpToDate { checked_at: None }),
                last_manual_check: Mutex::new(None),
                event_bus: OnceLock::new(),
                schedule_store: schedule::ScheduleStore::new(data_dir),
                history_store: history::HistoryStore::new(data_dir),
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                tray_proxy: RwLock::new(None),
            }),
        })
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    /// Attach an update notifier (typically the tray proxy) to publish update state.
    pub async fn set_tray_proxy(&self, proxy: Arc<dyn UpdateNotifier>) {
        let schedule = self.inner.schedule_store.load().ok().flatten();
        proxy.notify_schedule(schedule);
        *self.inner.tray_proxy.write().await = Some(proxy);
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn notify_tray_schedule(&self, schedule: Option<schedule::UpdateSchedule>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mgr = self.clone();
        handle.spawn(async move {
            if let Some(proxy) = mgr.inner.tray_proxy.read().await.clone() {
                proxy.notify_schedule(schedule);
            }
        });
    }

    /// ダッシュボードイベントバスを設定する。
    ///
    /// 設定後、状態遷移時に `UpdateStateChanged` イベントが自動発行される。
    pub fn set_event_bus(&self, bus: crate::events::SharedEventBus) {
        let _ = self.inner.event_bus.set(bus);
    }

    /// 状態遷移をダッシュボードに通知する。
    fn notify_state_changed(&self) {
        if let Some(bus) = self.inner.event_bus.get() {
            bus.publish(crate::events::DashboardEvent::UpdateStateChanged);
        }
    }

    /// Return the current update state snapshot.
    pub async fn state(&self) -> UpdateState {
        self.inner.state.read().await.clone()
    }

    /// Spawn a background task that downloads the update payload (if available).
    ///
    /// Returns immediately.  The download progress is reflected in
    /// `PayloadState::Downloading { downloaded_bytes, total_bytes }`.
    pub fn download_background(&self) {
        let mgr = self.clone();
        tokio::spawn(async move {
            if let Err(e) = mgr.ensure_payload_ready().await {
                tracing::warn!("background payload download failed: {e}");
            }
        });
    }

    /// Return `true` if a manual check was performed less than 60 s ago.
    pub fn is_manual_check_rate_limited(&self) -> bool {
        let guard = self.inner.last_manual_check.lock().unwrap();
        match *guard {
            Some(instant) => instant.elapsed() < Duration::from_secs(MANUAL_CHECK_COOLDOWN_SECS),
            None => false,
        }
    }

    /// Record that a manual check was just performed (for rate limiting).
    pub fn record_manual_check(&self) {
        let mut guard = self.inner.last_manual_check.lock().unwrap();
        *guard = Some(tokio::time::Instant::now());
    }

    /// Return the current in-flight inference request count.
    pub async fn in_flight(&self) -> usize {
        self.inner.gate.in_flight()
    }

    // ---- Schedule API ----

    /// Get the current update schedule (if any).
    pub fn get_schedule(&self) -> Result<Option<schedule::UpdateSchedule>> {
        self.inner.schedule_store.load()
    }

    /// Create a new schedule. Returns `Err` if a schedule already exists.
    pub fn create_schedule(
        &self,
        sched: schedule::UpdateSchedule,
    ) -> Result<schedule::UpdateSchedule> {
        if let Some(existing) = self.inner.schedule_store.load()? {
            return Err(anyhow!(
                "A schedule already exists (mode={:?}, target={})",
                existing.mode,
                existing.target_version
            ));
        }
        if sched.mode == schedule::ScheduleMode::Scheduled && sched.scheduled_at.is_none() {
            return Err(anyhow!("scheduled_at is required when mode is scheduled"));
        }
        self.inner.schedule_store.save(&sched)?;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.notify_tray_schedule(Some(sched.clone()));
        // If immediate, trigger apply right away.
        if sched.mode == schedule::ScheduleMode::Immediate {
            self.request_apply();
        }
        Ok(sched)
    }

    /// Cancel the current schedule. Returns `Err` if no schedule exists.
    pub fn cancel_schedule(&self) -> Result<()> {
        if !self.inner.schedule_store.remove()? {
            return Err(anyhow!("No schedule exists"));
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.notify_tray_schedule(None);
        Ok(())
    }

    /// Restore a persisted schedule on startup.
    ///
    /// If a schedule exists (e.g. after restart), it is re-activated:
    /// - `Immediate`: triggers apply right away.
    /// - `Idle` / `Scheduled`: the schedule loop will pick it up.
    fn restore_schedule(&self) {
        match self.inner.schedule_store.load() {
            Ok(Some(sched)) => {
                tracing::info!(
                    "restored update schedule: mode={:?}, target={}",
                    sched.mode,
                    sched.target_version
                );
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                self.notify_tray_schedule(Some(sched.clone()));
                if sched.mode == schedule::ScheduleMode::Immediate {
                    self.request_apply();
                }
                // Idle and Scheduled modes are handled by start_schedule_loop.
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("failed to restore update schedule: {e}");
            }
        }
    }

    /// Start the background schedule monitoring loop.
    ///
    /// This loop polls the schedule store every 5 seconds and triggers apply
    /// when schedule conditions are met:
    /// - `Idle`: triggers when `in_flight == 0` and an update is available.
    /// - `Scheduled`: triggers when the current time >= `scheduled_at`.
    fn start_schedule_loop(&self) {
        let mgr = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                let sched = match mgr.inner.schedule_store.load() {
                    Ok(Some(s)) => s,
                    _ => continue,
                };

                // Only trigger when the scheduled target version is still the latest available update.
                let latest_available = {
                    let st = mgr.inner.state.read().await;
                    match &*st {
                        UpdateState::Available { latest, .. } => Some(latest.clone()),
                        _ => None,
                    }
                };
                let Some(latest_available) = latest_available else {
                    continue;
                };
                if latest_available != sched.target_version {
                    continue;
                }

                let should_trigger = match sched.mode {
                    schedule::ScheduleMode::Immediate => {
                        // Immediate schedules are handled at creation and restore;
                        // if still present, trigger now.
                        true
                    }
                    schedule::ScheduleMode::Idle => mgr.inner.gate.in_flight() == 0,
                    schedule::ScheduleMode::Scheduled => {
                        if let Some(at) = sched.scheduled_at {
                            Utc::now() >= at
                        } else {
                            // Defensive: malformed persisted schedules must never trigger immediately.
                            false
                        }
                    }
                };

                if should_trigger {
                    tracing::info!(
                        "schedule triggered: mode={:?}, target={}",
                        sched.mode,
                        sched.target_version
                    );
                    // Remove the schedule before triggering to prevent re-trigger.
                    let _ = mgr.inner.schedule_store.remove();
                    #[cfg(any(target_os = "windows", target_os = "macos"))]
                    mgr.notify_tray_schedule(None);
                    mgr.request_apply();
                }
            }
        });
    }

    /// Append a history entry.
    pub fn record_history(&self, entry: history::HistoryEntry) {
        if let Err(e) = self.inner.history_store.append(entry) {
            tracing::warn!("Failed to record update history: {e}");
        }
    }

    /// Load update history.
    pub fn get_history(&self) -> Vec<history::HistoryEntry> {
        self.inner.history_store.load().unwrap_or_default()
    }

    /// Check if a `.bak` file exists for rollback.
    pub fn rollback_available(&self) -> bool {
        let current_exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return false,
        };
        current_exe.with_extension("bak").exists()
    }

    /// Request a manual rollback to the previous version.
    ///
    /// Restores the `.bak` file and restarts. Returns `Err` if no `.bak` exists.
    pub fn request_rollback(&self) -> Result<()> {
        let current_exe =
            std::env::current_exe().context("Failed to resolve current executable path")?;
        let backup = current_exe.with_extension("bak");
        if !backup.exists() {
            return Err(anyhow!("No previous version available (.bak not found)"));
        }

        // Record rollback in history.
        let version = env!("CARGO_PKG_VERSION").to_string();
        self.record_history(history::HistoryEntry {
            kind: history::HistoryEventKind::Rollback,
            version: version.clone(),
            message: Some(format!("Manual rollback from {version}")),
            timestamp: Utc::now(),
        });

        // Spawn a helper process that waits for this process to exit, then restores the backup.
        let args_file =
            write_restart_args_file(&self.inner.updates_dir.join(format!("rollback-{version}")))?;
        spawn_internal_rollback(&current_exe, &backup, &args_file)?;
        self.inner.shutdown.request_shutdown();
        Ok(())
    }

    /// Request applying the update as soon as it is safe.
    ///
    /// The background task will:
    /// - (Re)check GitHub Releases
    /// - Ensure the payload is downloaded/prepared
    /// - Start rejecting new inference requests and drain in-flight requests
    /// - Spawn an internal helper to apply the update, then request shutdown
    pub fn request_apply(&self) {
        self.request_apply_mode(ApplyRequestMode::Normal);
    }

    /// Request a normal update apply.
    ///
    /// Returns `true` when the request is expected to be queued (e.g. payload not ready,
    /// in-flight requests exist, or apply cannot start immediately).
    pub async fn request_apply_normal(&self) -> bool {
        let queued = self.will_normal_apply_be_queued().await;
        self.request_apply_mode(ApplyRequestMode::Normal);
        queued
    }

    /// Request a force update apply.
    ///
    /// Force apply requires an update payload that is already prepared (`available` + `payload=ready`).
    /// Returns the number of currently in-flight inference requests that may be dropped.
    pub async fn request_apply_force(&self) -> Result<usize> {
        let dropped_in_flight = self.validate_force_apply_request().await?;
        self.request_apply_mode(ApplyRequestMode::Force);
        Ok(dropped_in_flight)
    }

    /// Start background update check loop, apply loop, and schedule loop (idempotent).
    pub fn start_background_tasks(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }

        // Restore any persisted schedule on startup.
        self.restore_schedule();

        // Start schedule monitoring loop.
        self.start_schedule_loop();

        let mgr = self.clone();
        tokio::spawn(async move {
            if let Err(e) = mgr.check_and_maybe_download(false).await {
                tracing::warn!("update check failed: {e}");
            }

            let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick completes immediately; consume it since we already checked on startup.
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = mgr.check_and_maybe_download(false).await {
                            tracing::warn!("update check failed: {e}");
                        }
                    }
                    _ = mgr.inner.apply_notify.notified() => {
                        let request_mode = mgr.take_apply_request_mode();
                        if request_mode == ApplyRequestMode::None {
                            continue;
                        }

                        // For normal apply, refresh state right before apply (first click after boot,
                        // or retry after a previous failure).
                        //
                        // Force apply intentionally skips refresh/download because request_apply_force()
                        // already validated `payload=ready`; re-checking here can delay or invalidate an
                        // already accepted immediate apply request.
                        if request_mode == ApplyRequestMode::Normal {
                            if let Err(e) = mgr.check_and_maybe_download(true).await {
                                tracing::warn!("update check failed before apply: {e}");

                                // If GitHub is temporarily unreachable, fall back to the cached state so we can still
                                // apply a previously discovered update.
                                let already_available = {
                                    let st = mgr.inner.state.read().await;
                                    matches!(&*st, UpdateState::Available { .. })
                                };
                                if !already_available {
                                    if let Some(cache) =
                                        load_cache(&mgr.inner.cache_path).ok().flatten()
                                    {
                                        if let Err(err) = mgr.apply_cache(cache).await {
                                            tracing::warn!(
                                                "update cache apply failed before apply: {err}"
                                            );
                                        }
                                    }
                                }
                            }

                            let is_available = {
                                let st = mgr.inner.state.read().await;
                                matches!(&*st, UpdateState::Available { .. })
                            };
                            if !is_available {
                                continue;
                            }
                        }

                        if let Err(err) = mgr.apply_flow(request_mode).await {
                            tracing::warn!("update apply failed: {err}");
                            mgr.inner.gate.stop_rejecting();
                            {
                                let mut st = mgr.inner.state.write().await;
                                let (latest, release_url) = match &*st {
                                    UpdateState::Available { latest, release_url, .. } => {
                                        (Some(latest.clone()), Some(release_url.clone()))
                                    }
                                    UpdateState::Draining { latest, .. } => (Some(latest.clone()), None),
                                    UpdateState::Applying { latest, .. } => (Some(latest.clone()), None),
                                    _ => (None, None),
                                };
                                *st = UpdateState::Failed {
                                    latest,
                                    release_url,
                                    message: err.to_string(),
                                    failed_at: Utc::now(),
                                };
                            }
                            #[cfg(any(target_os = "windows", target_os = "macos"))]
                            notify_tray_failed(&mgr.inner.tray_proxy, err.to_string()).await;
                        }
                    }
                }
            }
        });
    }

    fn request_apply_mode(&self, mode: ApplyRequestMode) {
        let requested = mode as u8;
        loop {
            let current = self.inner.apply_request_mode.load(Ordering::SeqCst);
            if current >= requested {
                break;
            }
            if self
                .inner
                .apply_request_mode
                .compare_exchange(current, requested, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
        self.inner.apply_notify.notify_waiters();
    }

    fn take_apply_request_mode(&self) -> ApplyRequestMode {
        ApplyRequestMode::from_u8(
            self.inner
                .apply_request_mode
                .swap(ApplyRequestMode::None as u8, Ordering::SeqCst),
        )
    }

    async fn will_normal_apply_be_queued(&self) -> bool {
        if self.inner.gate.in_flight() > 0 {
            return true;
        }

        let st = self.inner.state.read().await;
        match &*st {
            UpdateState::Available {
                payload: PayloadState::Ready { .. },
                ..
            } => false,
            UpdateState::Draining { .. } | UpdateState::Applying { .. } => true,
            _ => true,
        }
    }

    async fn validate_force_apply_request(&self) -> Result<usize> {
        let dropped_in_flight = self.inner.gate.in_flight();
        let st = self.inner.state.read().await;
        match &*st {
            UpdateState::Available {
                payload: PayloadState::Ready { .. },
                ..
            } => Ok(dropped_in_flight),
            UpdateState::Available { .. } => Err(anyhow!("Update payload is not ready")),
            UpdateState::Draining { .. } | UpdateState::Applying { .. } => {
                Err(anyhow!("Update is already in progress"))
            }
            _ => Err(anyhow!("No update is available")),
        }
    }
}

fn default_paths() -> Result<(PathBuf, PathBuf)> {
    let data_dir = if let Ok(dir) = std::env::var("LLMLB_DATA_DIR") {
        PathBuf::from(dir)
    } else {
        match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            Ok(home) => PathBuf::from(home).join(".llmlb"),
            // Some environments (systemd services, minimal containers) may not set HOME/USERPROFILE.
            // Self-update should not prevent startup, so use a best-effort temporary directory.
            Err(_) => std::env::temp_dir().join("llmlb"),
        }
    };
    Ok((data_dir.join("update-check.json"), data_dir.join("updates")))
}

#[cfg(test)]
mod tests;
