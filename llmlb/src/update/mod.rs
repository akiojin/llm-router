//! Self-update manager.
//!
//! This module implements:
//! - Update discovery via GitHub Releases
//! - Background download of the preferred payload for the current platform
//! - User-approved apply flow: drain inference requests, then restart into the new version
//! - Internal helper modes (`__internal`) to safely replace binaries / run installers
//! - Update scheduling (immediate / idle / time-based)
//! - Update history recording

pub mod history;
pub mod schedule;

use crate::{inference_gate::InferenceGate, shutdown::ShutdownController};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use futures::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCacheFile {
    last_checked_at: DateTime<Utc>,
    latest_version: Option<String>,
    release_url: Option<String>,
    portable_asset_url: Option<String>,
    installer_asset_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
/// Current self-update state exposed to the dashboard/tray.
///
/// This state is intentionally serializable (snake_case) to make it easy to consume from the UI.
pub enum UpdateState {
    /// No update is available (or not yet checked).
    UpToDate {
        /// When the last update check completed (if known).
        checked_at: Option<DateTime<Utc>>,
    },
    /// A newer version is available on GitHub Releases.
    Available {
        /// Current running version.
        current: String,
        /// Latest available version.
        latest: String,
        /// Release page URL.
        release_url: String,
        /// Preferred portable payload URL for this platform, if present.
        portable_asset_url: Option<String>,
        /// Preferred installer payload URL for this platform, if present.
        installer_asset_url: Option<String>,
        /// Current payload download/preparation status.
        payload: PayloadState,
        /// When this update was last checked.
        checked_at: DateTime<Utc>,
    },
    /// Apply was requested; new inference requests are rejected while in-flight requests drain.
    Draining {
        /// Latest version being applied.
        latest: String,
        /// Current in-flight inference request count.
        in_flight: usize,
        /// When apply was requested.
        requested_at: DateTime<Utc>,
        /// When the drain will time out and be cancelled.
        timeout_at: DateTime<Utc>,
    },
    /// Update is being applied by an internal helper process.
    Applying {
        /// Latest version being applied.
        latest: String,
        /// Apply method chosen for this platform/install.
        method: ApplyMethod,
        /// Current apply phase for operator visibility.
        phase: ApplyPhase,
        /// Human-readable phase description.
        phase_message: String,
        /// When apply entered `state=applying`.
        started_at: DateTime<Utc>,
        /// Optional timeout deadline for the current phase.
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_at: Option<DateTime<Utc>>,
    },
    /// Update check/download/apply failed (best-effort; the server should keep running).
    Failed {
        /// Latest version (if known).
        latest: Option<String>,
        /// Release page URL (if known).
        release_url: Option<String>,
        /// Human-readable failure message.
        message: String,
        /// When the failure was recorded.
        failed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "payload", rename_all = "snake_case")]
/// Status of the update payload (portable archive or installer).
pub enum PayloadState {
    /// Payload is not downloaded/prepared yet.
    NotReady,
    /// Payload is being downloaded/extracted.
    Downloading {
        /// When the download/extraction started.
        started_at: DateTime<Utc>,
        /// Bytes downloaded so far (if known).
        #[serde(skip_serializing_if = "Option::is_none")]
        downloaded_bytes: Option<u64>,
        /// Total bytes expected (from Content-Length, if known).
        #[serde(skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
    },
    /// Payload is ready to apply.
    Ready {
        /// Prepared payload kind.
        kind: PayloadKind,
    },
    /// Payload download/extraction failed.
    Error {
        /// Human-readable error message.
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Prepared update payload kind.
pub enum PayloadKind {
    /// Portable archive extracted; `binary_path` points to the new executable.
    Portable {
        /// Path to the extracted new executable.
        binary_path: String,
    },
    /// Installer downloaded; `installer_path` points to the installer file.
    Installer {
        /// Path to the downloaded installer file.
        installer_path: String,
        /// Installer kind (OS-dependent).
        kind: InstallerKind,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Installer kind (OS-dependent).
pub enum InstallerKind {
    /// macOS `.pkg`.
    MacPkg,
    /// Windows setup `.exe` (Inno Setup).
    WindowsSetup,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Apply method used for the current update.
pub enum ApplyMethod {
    /// Replace the running executable with the extracted portable binary.
    PortableReplace,
    /// Run a macOS `.pkg` installer.
    MacPkg,
    /// Run a Windows setup `.exe` installer.
    WindowsSetup,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Detailed phase of the applying state.
pub enum ApplyPhase {
    /// Apply flow has started and is preparing to execute.
    Starting,
    /// Waiting for the previous process handoff.
    WaitingOldProcessExit,
    /// Installer is running.
    RunningInstaller,
    /// Restart is being initiated.
    Restarting,
}

impl ApplyPhase {
    fn message(&self) -> &'static str {
        match self {
            Self::Starting => "Preparing update apply",
            Self::WaitingOldProcessExit => "Waiting for current process handoff",
            Self::RunningInstaller => "Installer is running",
            Self::Restarting => "Restarting service",
        }
    }
}

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
    tray_proxy: RwLock<Option<crate::gui::tray::TrayEventProxy>>,
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
    /// Attach a tray event proxy to publish update state (best-effort).
    pub async fn set_tray_proxy(&self, proxy: crate::gui::tray::TrayEventProxy) {
        *self.inner.tray_proxy.write().await = Some(proxy.clone());
        let schedule = self.inner.schedule_store.load().ok().flatten();
        proxy.notify_schedule(schedule.map(|s| schedule_to_tray_info(&s)));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn notify_tray_schedule(&self, schedule: Option<schedule::UpdateSchedule>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mgr = self.clone();
        handle.spawn(async move {
            if let Some(proxy) = mgr.inner.tray_proxy.read().await.clone() {
                proxy.notify_schedule(schedule.map(|s| schedule_to_tray_info(&s)));
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

    /// Check GitHub for a newer release (synchronous, no download).
    ///
    /// This only queries the GitHub Releases API (timeout 5 s) and updates the
    /// internal state.  It intentionally does **not** start downloading the
    /// payload so the caller can return a fast response.
    pub async fn check_only(&self, force: bool) -> Result<UpdateState> {
        if !force {
            if let Some(cache) = load_cache(&self.inner.cache_path).ok().flatten() {
                let age = Utc::now().signed_duration_since(cache.last_checked_at);
                if age.to_std().unwrap_or(Duration::MAX) < self.inner.ttl {
                    self.apply_cache(cache).await?;
                    return Ok(self.state().await);
                }
            }
        }

        let timeout = Duration::from_secs(5);
        let release = match fetch_latest_release(
            &self.inner.http_client,
            &self.inner.owner,
            &self.inner.repo,
            timeout,
            self.inner.github_api_base_url.as_deref(),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                // GitHub API failure (429 rate limit, timeout, etc.):
                // preserve existing Available state (especially payload: Ready)
                // or fall back to cached data.
                tracing::warn!("GitHub API failed, falling back to cache: {e}");
                let current = self.state().await;
                if matches!(&current, UpdateState::Available { .. }) {
                    return Ok(current);
                }
                if let Some(cache) = load_cache(&self.inner.cache_path).ok().flatten() {
                    self.apply_cache(cache).await?;
                    return Ok(self.state().await);
                }
                return Err(e);
            }
        };
        let latest = parse_tag_to_version(&release.tag_name)?;
        if latest <= self.inner.current_version {
            *self.inner.state.write().await = UpdateState::UpToDate {
                checked_at: Some(Utc::now()),
            };
            save_cache(
                &self.inner.cache_path,
                UpdateCacheFile {
                    last_checked_at: Utc::now(),
                    latest_version: Some(latest.to_string()),
                    release_url: Some(release.html_url.clone()),
                    portable_asset_url: None,
                    installer_asset_url: None,
                },
            )?;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            notify_tray_up_to_date(&self.inner.tray_proxy).await;
            return Ok(self.state().await);
        }

        let platform = Platform::detect()?;
        let (portable_asset, installer_asset) = select_assets(&release, &platform);

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some(latest.to_string()),
            release_url: Some(release.html_url.clone()),
            portable_asset_url: portable_asset
                .as_ref()
                .map(|a| a.browser_download_url.clone()),
            installer_asset_url: installer_asset
                .as_ref()
                .map(|a| a.browser_download_url.clone()),
        };
        save_cache(&self.inner.cache_path, cache.clone())?;

        {
            *self.inner.state.write().await = UpdateState::Available {
                current: self.inner.current_version.to_string(),
                latest: latest.to_string(),
                release_url: release.html_url,
                portable_asset_url: cache.portable_asset_url.clone(),
                installer_asset_url: cache.installer_asset_url.clone(),
                payload: PayloadState::NotReady,
                checked_at: cache.last_checked_at,
            };
        }

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        notify_tray_available(&self.inner.tray_proxy, latest.to_string()).await;

        Ok(self.state().await)
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

    async fn check_and_maybe_download(&self, force: bool) -> Result<()> {
        if !force {
            if let Some(cache) = load_cache(&self.inner.cache_path).ok().flatten() {
                let age = Utc::now().signed_duration_since(cache.last_checked_at);
                if age.to_std().unwrap_or(Duration::MAX) < self.inner.ttl {
                    self.apply_cache(cache).await?;
                    #[cfg(any(target_os = "windows", target_os = "macos"))]
                    {
                        match &*self.inner.state.read().await {
                            UpdateState::Available { latest, .. } => {
                                notify_tray_available(&self.inner.tray_proxy, latest.clone()).await;
                            }
                            UpdateState::UpToDate { .. } => {
                                notify_tray_up_to_date(&self.inner.tray_proxy).await;
                            }
                            _ => {}
                        }
                    }
                    // Start download if update is available.
                    if matches!(
                        self.inner.state.read().await.clone(),
                        UpdateState::Available { .. }
                    ) {
                        let _ = self.ensure_payload_ready().await;
                    }
                    return Ok(());
                }
            }
        }

        let timeout = if force {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(2)
        };
        let release = fetch_latest_release(
            &self.inner.http_client,
            &self.inner.owner,
            &self.inner.repo,
            timeout,
            self.inner.github_api_base_url.as_deref(),
        )
        .await?;
        let latest = parse_tag_to_version(&release.tag_name)?;
        if latest <= self.inner.current_version {
            *self.inner.state.write().await = UpdateState::UpToDate {
                checked_at: Some(Utc::now()),
            };
            save_cache(
                &self.inner.cache_path,
                UpdateCacheFile {
                    last_checked_at: Utc::now(),
                    latest_version: Some(latest.to_string()),
                    release_url: Some(release.html_url.clone()),
                    portable_asset_url: None,
                    installer_asset_url: None,
                },
            )?;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            notify_tray_up_to_date(&self.inner.tray_proxy).await;
            return Ok(());
        }

        let platform = Platform::detect()?;
        let (portable_asset, installer_asset) = select_assets(&release, &platform);

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some(latest.to_string()),
            release_url: Some(release.html_url.clone()),
            portable_asset_url: portable_asset
                .as_ref()
                .map(|a| a.browser_download_url.clone()),
            installer_asset_url: installer_asset
                .as_ref()
                .map(|a| a.browser_download_url.clone()),
        };
        save_cache(&self.inner.cache_path, cache.clone())?;

        {
            let mut st = self.inner.state.write().await;
            *st = UpdateState::Available {
                current: self.inner.current_version.to_string(),
                latest: latest.to_string(),
                release_url: release.html_url,
                portable_asset_url: cache.portable_asset_url.clone(),
                installer_asset_url: cache.installer_asset_url.clone(),
                payload: PayloadState::NotReady,
                checked_at: cache.last_checked_at,
            };
        }

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        notify_tray_available(&self.inner.tray_proxy, latest.to_string()).await;

        // Background download (best-effort).
        let _ = self.ensure_payload_ready().await;
        Ok(())
    }

    async fn apply_cache(&self, cache: UpdateCacheFile) -> Result<()> {
        let latest_version = cache.latest_version.clone().unwrap_or_default();
        if latest_version.is_empty() {
            *self.inner.state.write().await = UpdateState::UpToDate {
                checked_at: Some(cache.last_checked_at),
            };
            return Ok(());
        }
        let latest = Version::parse(&latest_version).context("cached latest_version is invalid")?;
        if latest <= self.inner.current_version {
            *self.inner.state.write().await = UpdateState::UpToDate {
                checked_at: Some(cache.last_checked_at),
            };
            return Ok(());
        }
        let release_url = cache.release_url.clone().unwrap_or_else(|| {
            format!(
                "https://github.com/{}/{}/releases/latest",
                self.inner.owner, self.inner.repo
            )
        });
        *self.inner.state.write().await = UpdateState::Available {
            current: self.inner.current_version.to_string(),
            latest: latest.to_string(),
            release_url,
            portable_asset_url: cache.portable_asset_url.clone(),
            installer_asset_url: cache.installer_asset_url.clone(),
            payload: PayloadState::NotReady,
            checked_at: cache.last_checked_at,
        };
        Ok(())
    }

    /// Record an update check failure.
    ///
    /// Preserves an already-discovered `Available` state even if a subsequent
    /// manual check temporarily fails.
    pub async fn record_check_failure(&self, message: String) {
        {
            let mut st = self.inner.state.write().await;
            // Keep an already discovered update actionable even if a subsequent
            // manual check temporarily fails (e.g., transient GitHub outage).
            if matches!(&*st, UpdateState::Available { .. }) {
                return;
            }

            let (latest, release_url) = match &*st {
                UpdateState::Draining { latest, .. } => (Some(latest.clone()), None),
                UpdateState::Applying { latest, .. } => (Some(latest.clone()), None),
                UpdateState::Failed {
                    latest,
                    release_url,
                    ..
                } => (latest.clone(), release_url.clone()),
                _ => (None, None),
            };

            *st = UpdateState::Failed {
                latest,
                release_url,
                message: message.clone(),
                failed_at: Utc::now(),
            };
        }

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        notify_tray_failed(&self.inner.tray_proxy, message).await;
    }

    async fn ensure_payload_ready(&self) -> Result<PayloadKind> {
        let (latest, release_url, portable, installer) = {
            let st = self.inner.state.read().await;
            match &*st {
                UpdateState::Available {
                    latest,
                    release_url,
                    portable_asset_url,
                    installer_asset_url,
                    ..
                } => (
                    latest.clone(),
                    release_url.clone(),
                    portable_asset_url.clone(),
                    installer_asset_url.clone(),
                ),
                _ => return Err(anyhow!("No update is available")),
            }
        };

        {
            let mut st = self.inner.state.write().await;
            if let UpdateState::Available {
                payload: PayloadState::Ready { kind },
                ..
            } = &*st
            {
                return Ok(kind.clone());
            }
            if let UpdateState::Available { payload, .. } = &mut *st {
                *payload = PayloadState::Downloading {
                    started_at: Utc::now(),
                    downloaded_bytes: None,
                    total_bytes: None,
                };
            }
        }

        let platform = Platform::detect()?;
        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("llmlb"));
        let plan = choose_apply_plan(
            &platform,
            &current_exe,
            portable.as_deref(),
            installer.as_deref(),
        );

        let Some(plan) = plan else {
            let dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
            let writable = is_dir_writable(dir).unwrap_or(false);
            let msg = if !writable && installer.is_none() {
                format!(
                    "Automatic update is not supported because '{}' is not writable. Please reinstall from: {}",
                    dir.display(),
                    release_url
                )
            } else {
                format!(
                    "No suitable update asset found for this platform. Please download from: {}",
                    release_url
                )
            };
            self.set_payload_error(msg.clone()).await;
            return Err(anyhow!(msg));
        };

        let update_dir = self.inner.updates_dir.join(&latest);
        fs::create_dir_all(&update_dir).ok();

        let state_ref = self.inner.clone();
        let progress_cb: ProgressCallback = Box::new(move |downloaded, total| {
            if let Ok(mut st) = state_ref.state.try_write() {
                if let UpdateState::Available { payload, .. } = &mut *st {
                    if matches!(payload, PayloadState::Downloading { .. }) {
                        *payload = PayloadState::Downloading {
                            started_at: Utc::now(),
                            downloaded_bytes: Some(downloaded),
                            total_bytes: total,
                        };
                    }
                }
            }
        });

        let kind = match plan {
            ApplyPlan::Portable { url } => {
                let asset_name =
                    asset_name_from_url(&url).unwrap_or_else(|| "llmlb-update".to_string());
                let archive_path = update_dir.join(&asset_name);
                download_to_path(
                    &self.inner.http_client,
                    &url,
                    &archive_path,
                    Some(progress_cb),
                )
                .await?;
                let extract_dir = update_dir.join("extract");
                if extract_dir.exists() {
                    fs::remove_dir_all(&extract_dir).ok();
                }
                fs::create_dir_all(&extract_dir)?;
                extract_archive(&archive_path, &extract_dir)?;
                let binary_name = platform.binary_name();
                let binary_path = find_extracted_binary(&extract_dir, &binary_name)?
                    .ok_or_else(|| anyhow!("Extracted archive did not contain {binary_name}"))?;
                PayloadKind::Portable {
                    binary_path: binary_path.to_string_lossy().to_string(),
                }
            }
            ApplyPlan::Installer { url, kind } => {
                let asset_name =
                    asset_name_from_url(&url).unwrap_or_else(|| "llmlb-installer".to_string());
                let installer_path = update_dir.join(&asset_name);
                download_to_path(&self.inner.http_client, &url, &installer_path, None).await?;
                PayloadKind::Installer {
                    installer_path: installer_path.to_string_lossy().to_string(),
                    kind,
                }
            }
        };

        {
            let mut st = self.inner.state.write().await;
            if let UpdateState::Available { payload, .. } = &mut *st {
                *payload = PayloadState::Ready { kind: kind.clone() };
            }
        }

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        notify_tray_ready(&self.inner.tray_proxy).await;

        Ok(kind)
    }

    async fn set_payload_error(&self, msg: String) {
        let mut st = self.inner.state.write().await;
        if let UpdateState::Available { payload, .. } = &mut *st {
            *payload = PayloadState::Error { message: msg };
        }
    }

    async fn require_ready_payload(&self) -> Result<PayloadKind> {
        let st = self.inner.state.read().await;
        match &*st {
            UpdateState::Available {
                payload: PayloadState::Ready { kind },
                ..
            } => Ok(kind.clone()),
            UpdateState::Available { .. } => Err(anyhow!("Update payload is not ready")),
            UpdateState::Draining { .. } | UpdateState::Applying { .. } => {
                Err(anyhow!("Update is already in progress"))
            }
            _ => Err(anyhow!("No update is available")),
        }
    }

    async fn set_applying_state(
        &self,
        latest: &str,
        method: ApplyMethod,
        phase: ApplyPhase,
        started_at: DateTime<Utc>,
        timeout_at: Option<DateTime<Utc>>,
    ) {
        *self.inner.state.write().await = UpdateState::Applying {
            latest: latest.to_string(),
            method,
            phase: phase.clone(),
            phase_message: phase.message().to_string(),
            started_at,
            timeout_at,
        };
        self.notify_state_changed();
    }

    async fn apply_flow(&self, mode: ApplyRequestMode) -> Result<()> {
        let payload = match mode {
            ApplyRequestMode::Normal => self.ensure_payload_ready().await?,
            ApplyRequestMode::Force => self.require_ready_payload().await?,
            ApplyRequestMode::None => return Err(anyhow!("No apply request mode")),
        };
        let apply_method = match &payload {
            PayloadKind::Portable { .. } => ApplyMethod::PortableReplace,
            PayloadKind::Installer { kind, .. } => match kind {
                InstallerKind::MacPkg => ApplyMethod::MacPkg,
                InstallerKind::WindowsSetup => ApplyMethod::WindowsSetup,
            },
        };
        let latest = {
            let st = self.inner.state.read().await;
            match &*st {
                UpdateState::Available { latest, .. } => latest.clone(),
                _ => return Err(anyhow!("No update is available")),
            }
        };

        // Start draining after payload is ready to minimize downtime.
        self.inner.gate.start_rejecting();
        let applying_started_at = Utc::now();

        if mode == ApplyRequestMode::Force {
            // Mark as in-progress before waiting so UI/API cannot trigger duplicate apply actions.
            self.set_applying_state(
                &latest,
                apply_method.clone(),
                ApplyPhase::Starting,
                applying_started_at,
                None,
            )
            .await;
            // Force mode cancels active in-flight work instead of waiting for drain completion.
            self.inner.gate.abort_in_flight();
            if tokio::time::timeout(Duration::from_secs(3), self.inner.gate.wait_for_idle())
                .await
                .is_err()
            {
                tracing::warn!(
                    "force apply proceeding while in-flight requests are still unwinding"
                );
            }
        }

        if mode == ApplyRequestMode::Normal {
            let requested_at = Utc::now();
            let drain_timeout = Duration::from_secs(DEFAULT_DRAIN_TIMEOUT_SECS);
            let timeout_at =
                requested_at + chrono::Duration::seconds(DEFAULT_DRAIN_TIMEOUT_SECS as i64);
            let deadline = tokio::time::Instant::now() + drain_timeout;

            loop {
                let in_flight = self.inner.gate.in_flight();
                if in_flight == 0 {
                    break;
                }
                {
                    *self.inner.state.write().await = UpdateState::Draining {
                        latest: latest.clone(),
                        in_flight,
                        requested_at,
                        timeout_at,
                    };
                    self.notify_state_changed();
                }
                if tokio::time::timeout_at(deadline, self.inner.gate.wait_for_idle())
                    .await
                    .is_err()
                {
                    // Drain timed out — cancel and restore normal operation.
                    tracing::warn!(
                        "drain timed out after {}s with {} in-flight requests",
                        DEFAULT_DRAIN_TIMEOUT_SECS,
                        self.inner.gate.in_flight()
                    );
                    self.inner.gate.stop_rejecting();
                    *self.inner.state.write().await = UpdateState::Failed {
                        latest: Some(latest.clone()),
                        release_url: None,
                        message: format!("Drain timed out after {}s", DEFAULT_DRAIN_TIMEOUT_SECS),
                        failed_at: Utc::now(),
                    };
                    self.notify_state_changed();
                    return Err(anyhow!(
                        "Drain timed out after {}s",
                        DEFAULT_DRAIN_TIMEOUT_SECS
                    ));
                }
            }
            self.set_applying_state(
                &latest,
                apply_method.clone(),
                ApplyPhase::Starting,
                applying_started_at,
                None,
            )
            .await;
        }

        let current_exe =
            std::env::current_exe().context("Failed to resolve current executable path")?;
        let args_file = write_restart_args_file(&self.inner.updates_dir.join(&latest))?;

        match payload {
            PayloadKind::Portable { binary_path } => {
                self.set_applying_state(
                    &latest,
                    apply_method.clone(),
                    ApplyPhase::Restarting,
                    applying_started_at,
                    None,
                )
                .await;
                spawn_internal_apply_update(&current_exe, &binary_path, &args_file)?;
                self.inner.shutdown.request_shutdown();
                Ok(())
            }
            PayloadKind::Installer {
                installer_path,
                kind,
            } => {
                self.set_applying_state(
                    &latest,
                    apply_method.clone(),
                    ApplyPhase::RunningInstaller,
                    applying_started_at,
                    None,
                )
                .await;
                spawn_internal_run_installer(&current_exe, &installer_path, kind, &args_file)?;
                self.inner.shutdown.request_shutdown();
                Ok(())
            }
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

fn load_cache(path: &Path) -> Result<Option<UpdateCacheFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    let cache: UpdateCacheFile = serde_json::from_str(&content)?;
    Ok(Some(cache))
}

fn save_cache(path: &Path, cache: UpdateCacheFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&cache)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

async fn fetch_latest_release(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    timeout: Duration,
    api_base_url: Option<&str>,
) -> Result<GitHubRelease> {
    let base = api_base_url.unwrap_or("https://api.github.com");
    let url = format!("{base}/repos/{owner}/{repo}/releases/latest");
    let user_agent = format!("llmlb/{}", env!("CARGO_PKG_VERSION"));
    let res = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", user_agent)
        .timeout(timeout)
        .send()
        .await
        .context("Failed to call GitHub Releases API")?;
    if !res.status().is_success() {
        return Err(anyhow!("GitHub API returned {}", res.status().as_u16()));
    }
    let parsed: GitHubReleaseResponse = res
        .json()
        .await
        .context("Failed to parse GitHub release JSON")?;
    Ok(GitHubRelease {
        tag_name: parsed.tag_name,
        html_url: parsed.html_url,
        assets: parsed.assets,
    })
}

fn parse_tag_to_version(tag: &str) -> Result<Version> {
    let normalized = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(normalized).map_err(|e| anyhow!("Invalid tag semver: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Platform {
    os: String,
    arch: String,
}

impl Platform {
    fn detect() -> Result<Self> {
        Ok(Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        })
    }

    fn artifact(&self) -> Option<&'static str> {
        match (self.os.as_str(), self.arch.as_str()) {
            ("linux", "x86_64") => Some("linux-x86_64"),
            ("linux", "aarch64") => Some("linux-arm64"),
            ("macos", "x86_64") => Some("macos-x86_64"),
            ("macos", "aarch64") => Some("macos-arm64"),
            ("windows", "x86_64") => Some("windows-x86_64"),
            _ => None,
        }
    }

    fn binary_name(&self) -> String {
        if self.os == "windows" {
            "llmlb.exe".to_string()
        } else {
            "llmlb".to_string()
        }
    }

    fn portable_asset_name(&self) -> Option<String> {
        self.artifact().map(|a| {
            if self.os == "windows" {
                format!("llmlb-{a}.zip")
            } else {
                format!("llmlb-{a}.tar.gz")
            }
        })
    }

    fn installer_asset_name(&self) -> Option<(String, InstallerKind)> {
        let artifact = self.artifact()?;
        match self.os.as_str() {
            "macos" => Some((format!("llmlb-{artifact}.pkg"), InstallerKind::MacPkg)),
            "windows" => Some((
                format!("llmlb-{artifact}-setup.exe"),
                InstallerKind::WindowsSetup,
            )),
            _ => None,
        }
    }
}

fn select_assets(
    release: &GitHubRelease,
    platform: &Platform,
) -> (Option<GitHubAsset>, Option<GitHubAsset>) {
    let portable_name = platform.portable_asset_name();
    let installer = platform.installer_asset_name();

    let portable_asset =
        portable_name.and_then(|name| release.assets.iter().find(|a| a.name == name).cloned());

    let installer_asset =
        installer.and_then(|(name, _kind)| release.assets.iter().find(|a| a.name == name).cloned());

    (portable_asset, installer_asset)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApplyPlan {
    Portable { url: String },
    Installer { url: String, kind: InstallerKind },
}

fn choose_apply_plan(
    platform: &Platform,
    current_exe: &Path,
    portable_url: Option<&str>,
    installer_url: Option<&str>,
) -> Option<ApplyPlan> {
    let dir = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let writable = is_dir_writable(dir).unwrap_or(false);

    // If we cannot replace the current executable in-place, prefer installer when available.
    if !writable {
        if let Some(url) = installer_url {
            let kind = platform.installer_asset_name().map(|(_, k)| k)?;
            return Some(ApplyPlan::Installer {
                url: url.to_string(),
                kind,
            });
        }
        // No installer available and we cannot replace in-place.
        return None;
    }

    if let Some(url) = portable_url {
        return Some(ApplyPlan::Portable {
            url: url.to_string(),
        });
    }

    if let Some(url) = installer_url {
        let kind = platform.installer_asset_name().map(|(_, k)| k)?;
        return Some(ApplyPlan::Installer {
            url: url.to_string(),
            kind,
        });
    }

    None
}

fn is_dir_writable(dir: &Path) -> Result<bool> {
    fs::create_dir_all(dir).ok();
    let probe = dir.join(".llmlb_write_probe");
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map(|_| true)
        .or_else(|e| {
            if matches!(e.kind(), io::ErrorKind::PermissionDenied) {
                Ok(false)
            } else {
                Err(e)
            }
        })?;
    if result {
        let _ = fs::remove_file(&probe);
    }
    Ok(result)
}

fn asset_name_from_url(url: &str) -> Option<String> {
    url.split('/').next_back().map(|s| s.to_string())
}

/// Progress callback for streaming downloads: `(downloaded_bytes, total_bytes)`.
type ProgressCallback = Box<dyn Fn(u64, Option<u64>) + Send + Sync>;

async fn download_to_path(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let res = client
        .get(url)
        .timeout(Duration::from_secs(300))
        .send()
        .await?;
    if !res.status().is_success() {
        return Err(anyhow!("download failed with status {}", res.status()));
    }
    let total_bytes = res.content_length();
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading download stream")?;
        io::Write::write_all(&mut file, &chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(ref cb) = on_progress {
            cb(downloaded, total_bytes);
        }
    }
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    if name.ends_with(".tar.gz") {
        let file = fs::File::open(archive_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(dest_dir)?;
        return Ok(());
    }

    if name.ends_with(".zip") {
        let file = fs::File::open(archive_path)?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(dest_dir)?;
        return Ok(());
    }

    Err(anyhow!("unsupported archive format: {name}"))
}

fn find_extracted_binary(extract_dir: &Path, binary_name: &str) -> Result<Option<PathBuf>> {
    // Expected layout: dist/llmlb-<artifact>/<binary>
    let mut candidates = Vec::<PathBuf>::new();
    for entry in fs::read_dir(extract_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            candidates.push(path.join(binary_name));
        } else if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
            candidates.push(path);
        }
    }

    for c in candidates {
        if c.exists() {
            return Ok(Some(c));
        }
    }

    // Fallback: deep search.
    let mut stack = vec![extract_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .unwrap_or_else(|_| fs::read_dir(extract_dir).unwrap())
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
                return Ok(Some(path));
            }
        }
    }

    Ok(None)
}

#[derive(Debug, Serialize, Deserialize)]
struct RestartArgsFile {
    args: Vec<String>,
    cwd: String,
}

fn write_restart_args_file(update_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(update_dir).ok();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    let payload = RestartArgsFile { args, cwd };
    let path = update_dir.join("restart_args.json");
    let tmp = update_dir.join("restart_args.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&payload)?)?;
    fs::rename(tmp, &path)?;
    Ok(path)
}

fn spawn_internal_apply_update(
    current_exe: &Path,
    new_binary_path: &str,
    args_file: &Path,
) -> Result<()> {
    let pid = std::process::id().to_string();
    let target = current_exe.to_string_lossy().to_string();
    Command::new(current_exe)
        .arg("__internal")
        .arg("apply-update")
        .arg("--old-pid")
        .arg(pid)
        .arg("--target")
        .arg(target)
        .arg("--new-binary")
        .arg(new_binary_path)
        .arg("--args-file")
        .arg(args_file)
        .spawn()
        .context("Failed to spawn internal apply-update")?;
    Ok(())
}

fn spawn_internal_run_installer(
    current_exe: &Path,
    installer_path: &str,
    kind: InstallerKind,
    args_file: &Path,
) -> Result<()> {
    let pid = std::process::id().to_string();
    let target = current_exe.to_string_lossy().to_string();

    // Internal helper process executes installer for each OS.
    // Other platforms: best-effort (may fail due to missing privileges).
    Command::new(current_exe)
        .arg("__internal")
        .arg("run-installer")
        .arg("--old-pid")
        .arg(pid)
        .arg("--target")
        .arg(target)
        .arg("--installer")
        .arg(installer_path)
        .arg("--installer-kind")
        .arg(match kind {
            InstallerKind::MacPkg => "mac_pkg",
            InstallerKind::WindowsSetup => "windows_setup",
        })
        .arg("--args-file")
        .arg(args_file)
        .spawn()
        .context("Failed to spawn internal run-installer")?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn sh_single_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

#[cfg(target_os = "macos")]
fn escape_applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> Result<()> {
    let started = std::time::Instant::now();
    while crate::lock::is_process_running(pid) {
        if started.elapsed() > timeout {
            return Err(anyhow!("Timed out waiting for process {pid} to exit"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn restore_backup(backup: &Path, target: &Path) -> Result<()> {
    let started = std::time::Instant::now();
    let retry_timeout = Duration::from_secs(3);

    loop {
        match try_restore_backup(backup, target) {
            Ok(()) => return Ok(()),
            Err(err) if should_retry_restore_backup(&err) && started.elapsed() < retry_timeout => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(err) => return Err(err).context("Failed to restore backup"),
        }
    }
}

fn try_restore_backup(backup: &Path, target: &Path) -> io::Result<()> {
    if let Err(err) = fs::rename(backup, target) {
        if err.kind() == io::ErrorKind::CrossesDevices {
            fs::copy(backup, target)?;
            let _ = fs::remove_file(backup);
            return Ok(());
        }

        #[cfg(windows)]
        if should_remove_target_before_restore(&err) {
            remove_target_for_restore(target)?;
            fs::rename(backup, target)?;
            return Ok(());
        }

        return Err(err);
    }

    Ok(())
}

#[cfg(windows)]
fn should_remove_target_before_restore(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(5 | 32 | 80 | 183))
}

#[cfg(windows)]
fn remove_target_for_restore(target: &Path) -> io::Result<()> {
    if target.exists() {
        match fs::remove_file(target) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn should_retry_restore_backup(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::PermissionDenied {
        return true;
    }

    #[cfg(windows)]
    {
        if let Some(code) = err.raw_os_error() {
            return matches!(code, 5 | 32 | 33);
        }
    }

    false
}

pub(crate) fn internal_apply_update(
    old_pid: u32,
    target: PathBuf,
    new_binary: PathBuf,
    args_file: PathBuf,
) -> Result<()> {
    wait_for_pid_exit(old_pid, Duration::from_secs(300))?;

    // Backup target (best-effort).
    let backup = target.with_extension("bak");
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    if target.exists() {
        let _ = fs::rename(&target, &backup);
    }

    if let Err(e) = fs::rename(&new_binary, &target) {
        // Cross-device rename fallback.
        if e.kind() == io::ErrorKind::CrossesDevices {
            fs::copy(&new_binary, &target)?;
            let _ = fs::remove_file(&new_binary);
        } else {
            return Err(e).context("Failed to replace target executable");
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target, perms).ok();
    }

    restart_from_args_file(&target, &args_file)?;

    // T265: Monitor the new process for 30 seconds. If it doesn't respond to
    // health check, restore the backup and restart with the old version.
    if let Err(e) = wait_for_health_check(&args_file, Duration::from_secs(30)) {
        eprintln!("Health check failed after update: {e}");
        eprintln!("Rolling back to previous version...");
        if backup.exists() {
            // Kill the new (broken) process if it's running.
            // We don't know the PID, but we can try to restore the backup.
            if let Err(restore_err) = restore_backup(&backup, &target) {
                eprintln!("Failed to restore backup: {restore_err}");
                return Err(restore_err)
                    .context("Failed to restore backup after health check failure");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&target) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&target, perms).ok();
                }
            }
            // Record rollback in history (best-effort).
            record_auto_rollback_history(&args_file, &e.to_string());
            restart_from_args_file(&target, &args_file)?;
        }
        return Err(e);
    }

    Ok(())
}

/// Wait for the new process to respond to a health check on `/api/version`.
fn wait_for_health_check(args_file: &Path, timeout: Duration) -> Result<()> {
    let port = detect_server_port(args_file);
    let url = format!("http://127.0.0.1:{port}/api/version");
    let started = std::time::Instant::now();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("Failed to create HTTP client for health check")?;

    loop {
        if started.elapsed() > timeout {
            return Err(anyhow!(
                "Health check timed out after {}s (no response from {url})",
                timeout.as_secs()
            ));
        }
        match client.get(&url).send() {
            Ok(res) if res.status().is_success() => return Ok(()),
            _ => {}
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Best-effort: detect server port from restart args or environment.
fn detect_server_port(args_file: &Path) -> u16 {
    detect_server_port_from_args_file(args_file)
        .or_else(|| {
            // Fall back to env var if restart args do not include explicit port.
            std::env::var("LLMLB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(DEFAULT_LISTEN_PORT)
}

fn detect_server_port_from_args_file(args_file: &Path) -> Option<u16> {
    let content = fs::read_to_string(args_file).ok()?;
    let parsed: RestartArgsFile = serde_json::from_str(&content).ok()?;
    parse_port_from_args(&parsed.args)
}

fn parse_port_from_args(args: &[String]) -> Option<u16> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--port=") {
            if let Ok(port) = value.parse::<u16>() {
                return Some(port);
            }
        }
        if arg == "--port" || arg == "-p" {
            if let Some(value) = iter.next() {
                if let Ok(port) = value.parse::<u16>() {
                    return Some(port);
                }
            }
        }
    }

    None
}

/// Best-effort: record auto-rollback in history.
fn record_auto_rollback_history(args_file: &Path, reason: &str) {
    // Try to find the data dir from the args file's parent.
    let data_dir = args_file
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .unwrap_or_else(|| Path::new("."));
    let store = history::HistoryStore::new(data_dir);
    let _ = store.append(history::HistoryEntry {
        kind: history::HistoryEventKind::Rollback,
        version: env!("CARGO_PKG_VERSION").to_string(),
        message: Some(format!("Auto-rollback: {reason}")),
        timestamp: Utc::now(),
    });
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn internal_run_installer(
    old_pid: u32,
    target: PathBuf,
    installer: PathBuf,
    installer_kind: InstallerKind,
    args_file: PathBuf,
) -> Result<()> {
    wait_for_pid_exit(old_pid, Duration::from_secs(300))?;

    match installer_kind {
        InstallerKind::MacPkg => {
            #[cfg(target_os = "macos")]
            {
                run_macos_pkg_installer_with_privileges(&installer)?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(anyhow!("mac_pkg installer can only run on macOS"));
            }
        }
        InstallerKind::WindowsSetup => {
            #[cfg(target_os = "windows")]
            {
                let status = Command::new(&installer)
                    .args(["/VERYSILENT", "/CLOSEAPPLICATIONS", "/SUPPRESSMSGBOXES"])
                    .status()
                    .context("Failed to run Windows setup installer")?;
                if !status.success() {
                    return Err(anyhow!("Windows setup installer exited with {}", status));
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                return Err(anyhow!("windows_setup installer can only run on Windows"));
            }
        }
    }

    restart_from_args_file(&target, &args_file)?;
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn internal_run_installer(
    _old_pid: u32,
    _target: PathBuf,
    _installer: PathBuf,
    _installer_kind: InstallerKind,
    _args_file: PathBuf,
) -> Result<()> {
    Err(anyhow!(
        "installer updates are only supported on macOS/Windows"
    ))
}

fn spawn_internal_rollback(current_exe: &Path, backup: &Path, args_file: &Path) -> Result<()> {
    let pid = std::process::id().to_string();
    let target = current_exe.to_string_lossy().to_string();
    Command::new(current_exe)
        .arg("__internal")
        .arg("rollback")
        .arg("--old-pid")
        .arg(pid)
        .arg("--target")
        .arg(&target)
        .arg("--backup")
        .arg(backup)
        .arg("--args-file")
        .arg(args_file)
        .spawn()
        .context("Failed to spawn internal rollback")?;
    Ok(())
}

/// Rollback: wait for old process to exit, restore `.bak`, restart.
pub(crate) fn internal_rollback(
    old_pid: u32,
    target: PathBuf,
    backup: PathBuf,
    args_file: PathBuf,
) -> Result<()> {
    wait_for_pid_exit(old_pid, Duration::from_secs(60))?;

    // Restore backup.
    if !backup.exists() {
        return Err(anyhow!("Backup file does not exist: {}", backup.display()));
    }
    restore_backup(&backup, &target)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&target) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&target, perms).ok();
        }
    }

    restart_from_args_file(&target, &args_file)?;
    Ok(())
}

fn restart_from_args_file(target: &Path, args_file: &Path) -> Result<()> {
    let content = fs::read_to_string(args_file).context("Failed to read args-file")?;
    let parsed: RestartArgsFile =
        serde_json::from_str(&content).context("Invalid args-file JSON")?;

    let mut cmd = Command::new(target);
    cmd.args(parsed.args);
    if !parsed.cwd.is_empty() {
        cmd.current_dir(parsed.cwd);
    }
    cmd.spawn().context("Failed to spawn restarted process")?;
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn schedule_to_tray_info(schedule: &schedule::UpdateSchedule) -> crate::gui::tray::ScheduleInfo {
    let mode = match schedule.mode {
        schedule::ScheduleMode::Immediate => "Immediate",
        schedule::ScheduleMode::Idle => "Idle",
        schedule::ScheduleMode::Scheduled => "Scheduled",
    }
    .to_string();

    crate::gui::tray::ScheduleInfo {
        mode,
        scheduled_at: schedule.scheduled_at.as_ref().cloned(),
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn notify_tray_available(
    tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>,
    latest: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_available(latest);
    }
}

#[cfg(target_os = "macos")]
fn run_macos_pkg_installer_with_privileges(installer: &Path) -> Result<()> {
    let installer_path = installer.to_string_lossy().to_string();
    // Run /usr/sbin/installer as admin via AppleScript. Keep this helper process non-root so restart
    // happens under the invoking user account.
    let shell_cmd = format!(
        "/usr/sbin/installer -pkg {} -target /",
        sh_single_quote(&installer_path)
    );
    let applescript_cmd = format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript_string(&shell_cmd)
    );
    let status = Command::new("osascript")
        .arg("-e")
        .arg(applescript_cmd)
        .status()
        .context("Failed to run macOS installer via osascript")?;
    if !status.success() {
        return Err(anyhow!("osascript installer exited with {}", status));
    }
    Ok(())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn notify_tray_ready(tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_ready();
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn notify_tray_failed(
    tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>,
    message: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_failed(message);
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn notify_tray_up_to_date(tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_up_to_date();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available_state_with_payload(payload: PayloadState) -> UpdateState {
        UpdateState::Available {
            current: "4.5.0".to_string(),
            latest: "4.5.1".to_string(),
            release_url: "https://example.com/release".to_string(),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
            payload,
            checked_at: Utc::now(),
        }
    }

    #[test]
    fn test_platform_asset_names() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(
            p.portable_asset_name(),
            Some("llmlb-linux-x86_64.tar.gz".to_string())
        );
        assert_eq!(p.installer_asset_name(), None);

        let p = Platform {
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(
            p.portable_asset_name(),
            Some("llmlb-windows-x86_64.zip".to_string())
        );
        assert_eq!(
            p.installer_asset_name(),
            Some((
                "llmlb-windows-x86_64-setup.exe".to_string(),
                InstallerKind::WindowsSetup
            ))
        );
    }

    #[test]
    fn test_parse_tag_to_version() {
        assert_eq!(
            parse_tag_to_version("v3.1.0").unwrap(),
            Version::parse("3.1.0").unwrap()
        );
        assert_eq!(
            parse_tag_to_version("3.1.0").unwrap(),
            Version::parse("3.1.0").unwrap()
        );
    }

    #[tokio::test]
    async fn record_check_failure_preserves_available_payload() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        let ready_payload = PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        };

        {
            *manager.inner.state.write().await =
                available_state_with_payload(ready_payload.clone());
        }

        manager
            .record_check_failure("temporary network outage".to_string())
            .await;

        match manager.state().await {
            UpdateState::Available {
                latest, payload, ..
            } => {
                assert_eq!(latest, "4.5.1");
                assert_eq!(payload, ready_payload);
            }
            other => panic!("expected available state, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn record_check_failure_transitions_non_available_to_failed() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        {
            *manager.inner.state.write().await = UpdateState::UpToDate { checked_at: None };
        }

        manager
            .record_check_failure("check failed".to_string())
            .await;

        match manager.state().await {
            UpdateState::Failed {
                latest,
                release_url,
                message,
                ..
            } => {
                assert_eq!(latest, None);
                assert_eq!(release_url, None);
                assert_eq!(message, "check failed");
            }
            other => panic!("expected failed state, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_apply_normal_reports_not_queued_when_ready_and_idle() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::Ready {
                    kind: PayloadKind::Portable {
                        binary_path: "/tmp/llmlb-new".to_string(),
                    },
                });
        }

        let queued = manager.request_apply_normal().await;
        assert!(!queued);
        assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Normal);
    }

    #[tokio::test]
    async fn request_apply_normal_reports_queued_when_payload_not_ready() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        let queued = manager.request_apply_normal().await;
        assert!(queued);
        assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Normal);
    }

    #[tokio::test]
    async fn request_apply_force_requires_ready_payload() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        let err = manager
            .request_apply_force()
            .await
            .expect_err("force apply should fail when payload is not ready");
        assert!(err.to_string().contains("not ready"));
    }

    #[tokio::test]
    async fn request_apply_force_promotes_pending_normal_request() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::Ready {
                    kind: PayloadKind::Portable {
                        binary_path: "/tmp/llmlb-new".to_string(),
                    },
                });
        }

        manager.request_apply();
        let dropped = manager
            .request_apply_force()
            .await
            .expect("force apply request should be accepted");
        assert_eq!(dropped, 0);
        assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Force);
    }

    #[test]
    fn applying_state_serializes_phase_metadata() {
        let state = UpdateState::Applying {
            latest: "4.5.1".to_string(),
            method: ApplyMethod::WindowsSetup,
            phase: ApplyPhase::RunningInstaller,
            phase_message: "Installer is running".to_string(),
            started_at: Utc::now(),
            timeout_at: None,
        };

        let json = serde_json::to_value(state).expect("serialize applying state");
        assert_eq!(json["state"], "applying");
        assert_eq!(json["phase"], "running_installer");
        assert!(json.get("phase_message").is_some());
        assert!(json.get("started_at").is_some());
        assert!(json.get("timeout_at").is_none());
    }

    // =======================================================================
    // T210: check_only — GitHub APIチェックのみ同期、DLは行わない
    // =======================================================================
    #[tokio::test]
    async fn check_only_does_not_download_payload() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v99.0.0",
                "html_url": "https://github.com/test-owner/test-repo/releases/tag/v99.0.0",
                "assets": [{
                    "name": format!("llmlb-{}.tar.gz", Platform::detect().unwrap().artifact().unwrap_or("linux-x86_64")),
                    "browser_download_url": format!("{}/download/portable.tar.gz", mock_server.uri()),
                }]
            })))
            .mount(&mock_server)
            .await;

        let manager = UpdateManager::new_with_config(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
            "test-owner".to_string(),
            "test-repo".to_string(),
            Some(mock_server.uri()),
        )
        .expect("create update manager");

        let state = manager.check_only(true).await.expect("check_only");

        // Should discover the update.
        match &state {
            UpdateState::Available {
                latest, payload, ..
            } => {
                assert_eq!(latest, "99.0.0");
                // check_only must NOT start downloading.
                assert_eq!(*payload, PayloadState::NotReady);
            }
            other => panic!("expected available, got {other:?}"),
        }
    }

    // =======================================================================
    // T211: download_background — バックグラウンドDL開始、進捗更新
    // =======================================================================
    #[tokio::test]
    async fn download_background_transitions_to_downloading() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Serve a tiny payload so download completes.
        Mock::given(method("GET"))
            .and(path("/download/portable.tar.gz"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(vec![0u8; 100])
                    .insert_header("content-length", "100"),
            )
            .mount(&mock_server)
            .await;

        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        // Pre-seed available state with a portable asset URL pointing to mock.
        {
            let mut st = manager.inner.state.write().await;
            *st = UpdateState::Available {
                current: "4.5.0".to_string(),
                latest: "4.5.1".to_string(),
                release_url: "https://example.com/release".to_string(),
                portable_asset_url: Some(format!("{}/download/portable.tar.gz", mock_server.uri())),
                installer_asset_url: None,
                payload: PayloadState::NotReady,
                checked_at: Utc::now(),
            };
        }

        // Start background download.
        manager.download_background();

        // Give some time for async task to start and update state.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let state = manager.state().await;
        match &state {
            UpdateState::Available { payload, .. } => {
                // Should be Downloading or Ready (if completed quickly).
                assert!(
                    matches!(
                        payload,
                        PayloadState::Downloading { .. } | PayloadState::Ready { .. }
                    ),
                    "expected Downloading or Ready, got {payload:?}"
                );
            }
            other => panic!("expected available, got {other:?}"),
        }
    }

    // =======================================================================
    // T212: レートリミット判定
    // =======================================================================
    #[tokio::test]
    async fn rate_limit_rejects_within_60_seconds() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        // First call should succeed (not rate-limited).
        assert!(
            !manager.is_manual_check_rate_limited(),
            "first call should not be rate-limited"
        );
        manager.record_manual_check();

        // Immediate second call should be rate-limited.
        assert!(
            manager.is_manual_check_rate_limited(),
            "second call within 60s should be rate-limited"
        );
    }

    #[tokio::test]
    async fn rate_limit_allows_after_cooldown() {
        use tokio::time;

        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        manager.record_manual_check();
        assert!(manager.is_manual_check_rate_limited());

        // Advance time past 60 seconds.
        time::pause();
        time::advance(Duration::from_secs(61)).await;

        assert!(
            !manager.is_manual_check_rate_limited(),
            "should allow check after 60s cooldown"
        );
    }

    // =======================================================================
    // T250: ドレインタイムアウト — タイムアウト超過でキャンセル＋ゲート再開＋failed遷移
    // =======================================================================
    #[tokio::test]
    async fn drain_timeout_cancels_and_transitions_to_failed() {
        use tokio::time;

        time::pause();

        let gate = InferenceGate::default();
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            gate.clone(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        // Set up available state with ready payload.
        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::Ready {
                    kind: PayloadKind::Portable {
                        binary_path: "/tmp/llmlb-new".to_string(),
                    },
                });
        }

        // Simulate an in-flight request that never completes.
        let _guard = gate.begin_for_test();

        // Start apply_flow in a task — it will try to drain.
        let mgr = manager.clone();
        let apply_task =
            tokio::spawn(async move { mgr.apply_flow(ApplyRequestMode::Normal).await });

        // Let the drain start.
        time::advance(Duration::from_millis(100)).await;
        tokio::task::yield_now().await;

        // Verify we're in Draining state.
        let state = manager.state().await;
        assert!(
            matches!(state, UpdateState::Draining { .. }),
            "expected draining, got {state:?}"
        );

        // Advance time past the drain timeout (300s).
        time::advance(Duration::from_secs(301)).await;
        tokio::task::yield_now().await;

        // apply_flow should return an error.
        let result = apply_task.await.expect("task should complete");
        assert!(result.is_err(), "apply_flow should fail on drain timeout");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "error should mention timeout: {err_msg}"
        );

        // State should be Failed.
        let state = manager.state().await;
        match &state {
            UpdateState::Failed { message, .. } => {
                assert!(
                    message.contains("timed out"),
                    "failed message should mention timeout: {message}"
                );
            }
            other => panic!("expected failed state, got {other:?}"),
        }

        // Gate should no longer be rejecting.
        assert!(
            !gate.is_rejecting(),
            "gate should stop rejecting after drain timeout"
        );
    }

    // T250 supplemental: drain that completes before timeout succeeds.
    #[tokio::test]
    async fn drain_completes_before_timeout() {
        use tokio::time;

        time::pause();

        let gate = InferenceGate::default();
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            gate.clone(),
            ShutdownController::default(),
        )
        .expect("create update manager");

        // Set up available state with ready payload.
        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::Ready {
                    kind: PayloadKind::Portable {
                        binary_path: "/tmp/llmlb-new".to_string(),
                    },
                });
        }

        // Simulate an in-flight request.
        let guard = gate.begin_for_test();

        let mgr = manager.clone();
        let apply_task =
            tokio::spawn(async move { mgr.apply_flow(ApplyRequestMode::Normal).await });

        // Let drain start.
        time::advance(Duration::from_millis(100)).await;
        tokio::task::yield_now().await;

        // Complete the request before timeout.
        drop(guard);
        time::advance(Duration::from_millis(100)).await;
        tokio::task::yield_now().await;

        // apply_flow will fail because it tries to spawn a real binary,
        // but it should NOT fail due to timeout.
        let result = apply_task.await.expect("task should complete");
        // The error (if any) should be about spawning, not timeout.
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("timed out"),
                "should not time out: {e}"
            );
        }

        // State should NOT be Failed due to timeout.
        let state = manager.state().await;
        assert!(
            !matches!(
                &state,
                UpdateState::Failed { message, .. } if message.contains("timed out")
            ),
            "should not be in timeout-failed state: {state:?}"
        );
    }

    /// Helper to create an UpdateManager with an isolated temp data dir for testing.
    ///
    /// Uses a unique env var approach with per-test isolation.
    fn test_manager_with_gate(gate: InferenceGate) -> (UpdateManager, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(tmp.path()).expect("create data dir");
        let manager = UpdateManager::new_with_data_dir(
            reqwest::Client::new(),
            gate,
            ShutdownController::default(),
            tmp.path(),
        )
        .expect("create update manager");
        (manager, tmp)
    }

    // =======================================================================
    // T232: アイドル時適用トリガー — in_flight=0でスケジュール起動
    // =======================================================================
    #[tokio::test]
    async fn idle_schedule_triggers_when_in_flight_zero() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate.clone());

        // Set up available state.
        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        // Create idle schedule.
        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Idle,
            scheduled_at: None,
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager
            .create_schedule(sched)
            .expect("schedule should be created");

        // No in-flight requests → in_flight == 0.
        assert_eq!(gate.in_flight(), 0);

        // Start schedule loop.
        manager.start_schedule_loop();

        // Give the loop time to detect idle and trigger.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Schedule should have been removed (triggered).
        assert!(
            manager.get_schedule().unwrap().is_none(),
            "schedule should be consumed after idle trigger"
        );

        // Apply request should have been triggered.
        let mode = manager.take_apply_request_mode();
        assert_eq!(
            mode,
            ApplyRequestMode::Normal,
            "idle schedule should trigger normal apply"
        );
    }

    #[tokio::test]
    async fn idle_schedule_does_not_trigger_while_busy() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate.clone());

        // Set up available state.
        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        // Simulate in-flight request.
        let _guard = gate.begin_for_test();

        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Idle,
            scheduled_at: None,
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager
            .create_schedule(sched)
            .expect("schedule should be created");

        manager.start_schedule_loop();
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Schedule should still exist (not triggered).
        assert!(
            manager.get_schedule().unwrap().is_some(),
            "schedule should remain while requests are in-flight"
        );

        // No apply request should be pending.
        let mode = manager.take_apply_request_mode();
        assert_eq!(
            mode,
            ApplyRequestMode::None,
            "should not trigger while busy"
        );
    }

    #[test]
    fn scheduled_mode_requires_scheduled_at() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Scheduled,
            scheduled_at: None,
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };

        let err = manager
            .create_schedule(sched)
            .expect_err("scheduled mode without scheduled_at must be rejected");
        assert!(
            err.to_string()
                .contains("scheduled_at is required when mode is scheduled"),
            "unexpected error: {err}"
        );
    }

    // =======================================================================
    // T233: 時刻指定適用トリガー — 指定時刻到達でドレイン開始
    // =======================================================================
    #[tokio::test]
    async fn scheduled_time_triggers_when_past_due() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        // Schedule for 1 second ago (already past due).
        let scheduled_at = Utc::now() - chrono::Duration::seconds(1);
        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Scheduled,
            scheduled_at: Some(scheduled_at),
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager
            .create_schedule(sched)
            .expect("schedule should be created");

        manager.start_schedule_loop();

        // Give the loop time to detect and trigger.
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            manager.get_schedule().unwrap().is_none(),
            "schedule should be consumed after scheduled_at"
        );

        let mode = manager.take_apply_request_mode();
        assert_eq!(
            mode,
            ApplyRequestMode::Normal,
            "scheduled trigger should request normal apply"
        );
    }

    #[tokio::test]
    async fn scheduled_time_does_not_trigger_when_target_version_mismatch() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        {
            let mut state = available_state_with_payload(PayloadState::NotReady);
            if let UpdateState::Available { latest, .. } = &mut state {
                *latest = "4.5.2".to_string();
            }
            *manager.inner.state.write().await = state;
        }

        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Scheduled,
            scheduled_at: Some(Utc::now() - chrono::Duration::seconds(1)),
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager
            .create_schedule(sched)
            .expect("schedule should be created");

        manager.start_schedule_loop();
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            manager.get_schedule().unwrap().is_some(),
            "schedule should remain when target version no longer matches latest"
        );
        assert_eq!(
            manager.take_apply_request_mode(),
            ApplyRequestMode::None,
            "target version mismatch must not trigger apply"
        );
    }

    #[tokio::test]
    async fn malformed_scheduled_without_time_does_not_trigger() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        // Simulate malformed persisted data from an older version.
        let malformed = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Scheduled,
            scheduled_at: None,
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager.inner.schedule_store.save(&malformed).unwrap();

        manager.start_schedule_loop();
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            manager.get_schedule().unwrap().is_some(),
            "malformed scheduled entry should not be consumed automatically"
        );
        assert_eq!(
            manager.take_apply_request_mode(),
            ApplyRequestMode::None,
            "malformed scheduled entry must never trigger apply"
        );
    }

    // =======================================================================
    // T260: ヘルパー起動監視 — .bakから復元ロジックのテスト
    // =======================================================================
    #[test]
    fn internal_rollback_restores_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("llmlb");
        let backup = dir.path().join("llmlb.bak");
        let args_file = dir.path().join("restart_args.json");

        // Simulate a freshly updated target that rollback must replace.
        fs::write(&target, b"new-binary-content").unwrap();
        // Create a fake "old" binary.
        fs::write(&backup, b"old-binary-content").unwrap();
        // Create a fake args file (needed for restart_from_args_file).
        let args = RestartArgsFile {
            args: vec![],
            cwd: dir.path().to_string_lossy().to_string(),
        };
        fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();

        // internal_rollback expects the old process to have exited.
        // Using PID 0 or a non-existent PID: use current PID which is alive.
        // Instead, use PID 1 which is always running on Unix — let's use a non-existent PID.
        // PID u32::MAX is unlikely to exist.
        let result = internal_rollback(u32::MAX, target.clone(), backup.clone(), args_file);

        // The rollback should have restored the backup to the target path.
        assert!(target.exists(), "target should be restored from backup");
        assert!(!backup.exists(), "backup should be consumed (renamed)");
        let content = fs::read(&target).unwrap();
        assert_eq!(content, b"old-binary-content");

        // The restart_from_args_file call will fail because the target is not
        // executable, but the backup restoration should have succeeded.
        // We check if the result is Err (from failed spawn) but not from rollback.
        if let Err(e) = result {
            // Expected: spawn failure because we wrote fake content, not a real binary.
            assert!(
                !e.to_string().contains("Backup file does not exist"),
                "should not fail due to missing backup: {e}"
            );
        }
    }

    #[test]
    fn internal_rollback_fails_without_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("llmlb");
        let backup = dir.path().join("llmlb.bak");
        let args_file = dir.path().join("restart_args.json");

        let result = internal_rollback(u32::MAX, target, backup, args_file);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Backup file does not exist"));
    }

    // =======================================================================
    // T262: ロールバック結果の update-history.json 記録
    // =======================================================================
    #[test]
    fn record_auto_rollback_history_writes_entry() {
        let dir = tempfile::tempdir().unwrap();
        // Create directory structure: data_dir/updates/rollback-X.Y.Z/restart_args.json
        let updates_dir = dir.path().join("updates").join("rollback-test");
        fs::create_dir_all(&updates_dir).unwrap();
        let args_file = updates_dir.join("restart_args.json");
        fs::write(&args_file, "{}").unwrap();

        super::record_auto_rollback_history(&args_file, "health check failed");

        let store = history::HistoryStore::new(dir.path());
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, history::HistoryEventKind::Rollback);
        assert!(entries[0]
            .message
            .as_ref()
            .unwrap()
            .contains("health check failed"));
    }

    #[test]
    fn detect_server_port_reads_restart_args_file() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("restart_args.json");
        let args = RestartArgsFile {
            args: vec![
                "serve".to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
                "--port".to_string(),
                "40123".to_string(),
            ],
            cwd: dir.path().to_string_lossy().to_string(),
        };
        fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();

        assert_eq!(detect_server_port(&args_file), 40123);
    }

    #[test]
    fn parse_port_from_args_supports_equals_style() {
        let args = vec!["serve".to_string(), "--port=40124".to_string()];
        assert_eq!(parse_port_from_args(&args), Some(40124));
    }

    #[tokio::test]
    async fn scheduled_time_does_not_trigger_before_time() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        // Schedule for 60 seconds from now (far future, won't trigger in test).
        let scheduled_at = Utc::now() + chrono::Duration::seconds(60);
        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Scheduled,
            scheduled_at: Some(scheduled_at),
            scheduled_by: "admin".to_string(),
            target_version: "4.5.1".to_string(),
            created_at: Utc::now(),
        };
        manager
            .create_schedule(sched)
            .expect("schedule should be created");

        manager.start_schedule_loop();

        // Wait a bit — should NOT trigger (still 60s away).
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            manager.get_schedule().unwrap().is_some(),
            "schedule should not trigger before scheduled_at"
        );
        assert_eq!(
            manager.take_apply_request_mode(),
            ApplyRequestMode::None,
            "should not trigger before time"
        );
    }

    // =======================================================================
    // check_only: GitHub API失敗時にキャッシュフォールバック
    // (SPEC-a6e55b37 ユーザーストーリー10シナリオ4)
    // =======================================================================
    // =======================================================================
    // parse_tag_to_version: edge cases
    // =======================================================================
    #[test]
    fn parse_tag_to_version_with_v_prefix() {
        let v = parse_tag_to_version("v1.2.3").unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn parse_tag_to_version_without_prefix() {
        let v = parse_tag_to_version("1.2.3").unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn parse_tag_to_version_prerelease() {
        let v = parse_tag_to_version("v2.0.0-beta.1").unwrap();
        assert_eq!(v.major, 2);
        assert!(!v.pre.is_empty());
    }

    #[test]
    fn parse_tag_to_version_invalid() {
        assert!(parse_tag_to_version("not-a-version").is_err());
    }

    #[test]
    fn parse_tag_to_version_empty() {
        assert!(parse_tag_to_version("").is_err());
    }

    #[test]
    fn parse_tag_to_version_v_only() {
        assert!(parse_tag_to_version("v").is_err());
    }

    #[test]
    fn parse_tag_to_version_partial() {
        // semver requires major.minor.patch
        assert!(parse_tag_to_version("v1.2").is_err());
    }

    // =======================================================================
    // Platform tests
    // =======================================================================
    #[test]
    fn platform_detect_returns_current_os() {
        let p = Platform::detect().unwrap();
        assert_eq!(p.os, std::env::consts::OS);
        assert_eq!(p.arch, std::env::consts::ARCH);
    }

    #[test]
    fn platform_artifact_linux_x86_64() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.artifact(), Some("linux-x86_64"));
    }

    #[test]
    fn platform_artifact_linux_arm64() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "aarch64".to_string(),
        };
        assert_eq!(p.artifact(), Some("linux-arm64"));
    }

    #[test]
    fn platform_artifact_macos_arm64() {
        let p = Platform {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
        };
        assert_eq!(p.artifact(), Some("macos-arm64"));
        assert_eq!(
            p.portable_asset_name(),
            Some("llmlb-macos-arm64.tar.gz".to_string())
        );
        assert_eq!(
            p.installer_asset_name(),
            Some(("llmlb-macos-arm64.pkg".to_string(), InstallerKind::MacPkg))
        );
    }

    #[test]
    fn platform_artifact_macos_x86_64() {
        let p = Platform {
            os: "macos".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.artifact(), Some("macos-x86_64"));
        assert_eq!(
            p.installer_asset_name(),
            Some(("llmlb-macos-x86_64.pkg".to_string(), InstallerKind::MacPkg))
        );
    }

    #[test]
    fn platform_artifact_unknown() {
        let p = Platform {
            os: "freebsd".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.artifact(), None);
        assert_eq!(p.portable_asset_name(), None);
        assert_eq!(p.installer_asset_name(), None);
    }

    #[test]
    fn platform_binary_name_unix() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.binary_name(), "llmlb");
    }

    #[test]
    fn platform_binary_name_windows() {
        let p = Platform {
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.binary_name(), "llmlb.exe");
    }

    // =======================================================================
    // asset_name_from_url
    // =======================================================================
    #[test]
    fn asset_name_from_url_extracts_filename() {
        assert_eq!(
            asset_name_from_url("https://example.com/downloads/llmlb-linux-x86_64.tar.gz"),
            Some("llmlb-linux-x86_64.tar.gz".to_string())
        );
    }

    #[test]
    fn asset_name_from_url_single_segment() {
        assert_eq!(
            asset_name_from_url("llmlb.tar.gz"),
            Some("llmlb.tar.gz".to_string())
        );
    }

    #[test]
    fn asset_name_from_url_empty() {
        assert_eq!(asset_name_from_url(""), Some("".to_string()));
    }

    // =======================================================================
    // select_assets
    // =======================================================================
    #[test]
    fn select_assets_finds_matching_portable() {
        let release = GitHubRelease {
            tag_name: "v5.0.0".to_string(),
            html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
            assets: vec![
                GitHubAsset {
                    name: "llmlb-linux-x86_64.tar.gz".to_string(),
                    browser_download_url: "https://dl.example.com/llmlb-linux-x86_64.tar.gz"
                        .to_string(),
                },
                GitHubAsset {
                    name: "llmlb-windows-x86_64.zip".to_string(),
                    browser_download_url: "https://dl.example.com/llmlb-windows-x86_64.zip"
                        .to_string(),
                },
            ],
        };

        let platform = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        let (portable, installer) = select_assets(&release, &platform);
        assert!(portable.is_some());
        assert_eq!(portable.unwrap().name, "llmlb-linux-x86_64.tar.gz");
        assert!(installer.is_none()); // linux has no installer
    }

    #[test]
    fn select_assets_finds_both_on_windows() {
        let release = GitHubRelease {
            tag_name: "v5.0.0".to_string(),
            html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
            assets: vec![
                GitHubAsset {
                    name: "llmlb-windows-x86_64.zip".to_string(),
                    browser_download_url: "https://dl.example.com/llmlb-windows-x86_64.zip"
                        .to_string(),
                },
                GitHubAsset {
                    name: "llmlb-windows-x86_64-setup.exe".to_string(),
                    browser_download_url: "https://dl.example.com/llmlb-windows-x86_64-setup.exe"
                        .to_string(),
                },
            ],
        };

        let platform = Platform {
            os: "windows".to_string(),
            arch: "x86_64".to_string(),
        };
        let (portable, installer) = select_assets(&release, &platform);
        assert!(portable.is_some());
        assert!(installer.is_some());
        assert_eq!(portable.unwrap().name, "llmlb-windows-x86_64.zip");
        assert_eq!(installer.unwrap().name, "llmlb-windows-x86_64-setup.exe");
    }

    #[test]
    fn select_assets_returns_none_when_no_match() {
        let release = GitHubRelease {
            tag_name: "v5.0.0".to_string(),
            html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
            assets: vec![GitHubAsset {
                name: "llmlb-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://dl.example.com/llmlb-linux-x86_64.tar.gz"
                    .to_string(),
            }],
        };

        let platform = Platform {
            os: "freebsd".to_string(),
            arch: "x86_64".to_string(),
        };
        let (portable, installer) = select_assets(&release, &platform);
        assert!(portable.is_none());
        assert!(installer.is_none());
    }

    // =======================================================================
    // choose_apply_plan
    // =======================================================================
    #[test]
    fn choose_apply_plan_prefers_portable_when_writable() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("llmlb");
        fs::write(&exe_path, b"dummy").unwrap();

        let platform = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        let plan = choose_apply_plan(
            &platform,
            &exe_path,
            Some("https://example.com/portable.tar.gz"),
            None,
        );
        assert_eq!(
            plan,
            Some(ApplyPlan::Portable {
                url: "https://example.com/portable.tar.gz".to_string()
            })
        );
    }

    #[test]
    fn choose_apply_plan_returns_none_when_no_urls() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("llmlb");
        fs::write(&exe_path, b"dummy").unwrap();

        let platform = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        let plan = choose_apply_plan(&platform, &exe_path, None, None);
        assert!(plan.is_none());
    }

    #[test]
    fn choose_apply_plan_falls_back_to_installer_when_writable() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("llmlb");
        fs::write(&exe_path, b"dummy").unwrap();

        let platform = Platform {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
        };
        let plan = choose_apply_plan(
            &platform,
            &exe_path,
            None,
            Some("https://example.com/installer.pkg"),
        );
        assert!(matches!(plan, Some(ApplyPlan::Installer { .. })));
    }

    // =======================================================================
    // is_dir_writable
    // =======================================================================
    #[test]
    fn is_dir_writable_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_dir_writable(dir.path()).unwrap());
    }

    // =======================================================================
    // load_cache / save_cache roundtrip
    // =======================================================================
    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");

        // No cache initially
        assert!(load_cache(&cache_path).unwrap().is_none());

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("5.0.0".to_string()),
            release_url: Some("https://example.com/release".to_string()),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
        };
        save_cache(&cache_path, cache.clone()).unwrap();

        let loaded = load_cache(&cache_path).unwrap().unwrap();
        assert_eq!(loaded.latest_version, cache.latest_version);
        assert_eq!(loaded.release_url, cache.release_url);
        assert_eq!(loaded.portable_asset_url, cache.portable_asset_url);
        assert_eq!(loaded.installer_asset_url, cache.installer_asset_url);
    }

    #[test]
    fn load_cache_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.json");
        assert!(load_cache(&cache_path).unwrap().is_none());
    }

    #[test]
    fn save_cache_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("subdir").join("nested").join("cache.json");

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: None,
            release_url: None,
            portable_asset_url: None,
            installer_asset_url: None,
        };
        save_cache(&cache_path, cache).unwrap();
        assert!(cache_path.exists());
    }

    // =======================================================================
    // UpdateCacheFile serialization
    // =======================================================================
    #[test]
    fn update_cache_file_serialization() {
        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("5.0.0".to_string()),
            release_url: Some("https://example.com/release".to_string()),
            portable_asset_url: None,
            installer_asset_url: None,
        };
        let json = serde_json::to_string(&cache).unwrap();
        let deserialized: UpdateCacheFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.latest_version, cache.latest_version);
        assert_eq!(deserialized.release_url, cache.release_url);
    }

    // =======================================================================
    // ApplyRequestMode
    // =======================================================================
    #[test]
    fn apply_request_mode_from_u8_round_trip() {
        assert_eq!(ApplyRequestMode::from_u8(0), ApplyRequestMode::None);
        assert_eq!(ApplyRequestMode::from_u8(1), ApplyRequestMode::Normal);
        assert_eq!(ApplyRequestMode::from_u8(2), ApplyRequestMode::Force);
    }

    #[test]
    fn apply_request_mode_from_u8_unknown_defaults_to_none() {
        assert_eq!(ApplyRequestMode::from_u8(3), ApplyRequestMode::None);
        assert_eq!(ApplyRequestMode::from_u8(255), ApplyRequestMode::None);
    }

    #[test]
    fn apply_request_mode_ordering() {
        assert!(ApplyRequestMode::None < ApplyRequestMode::Normal);
        assert!(ApplyRequestMode::Normal < ApplyRequestMode::Force);
    }

    // =======================================================================
    // ApplyPhase::message
    // =======================================================================
    #[test]
    fn apply_phase_messages_are_non_empty() {
        let phases = [
            ApplyPhase::Starting,
            ApplyPhase::WaitingOldProcessExit,
            ApplyPhase::RunningInstaller,
            ApplyPhase::Restarting,
        ];
        for phase in &phases {
            assert!(
                !phase.message().is_empty(),
                "phase {:?} has empty message",
                phase
            );
        }
    }

    #[test]
    fn apply_phase_starting_message() {
        assert_eq!(ApplyPhase::Starting.message(), "Preparing update apply");
    }

    #[test]
    fn apply_phase_restarting_message() {
        assert_eq!(ApplyPhase::Restarting.message(), "Restarting service");
    }

    // =======================================================================
    // UpdateState serialization
    // =======================================================================
    #[test]
    fn update_state_up_to_date_serialization() {
        let state = UpdateState::UpToDate {
            checked_at: Some(Utc::now()),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "up_to_date");
        assert!(json.get("checked_at").is_some());
    }

    #[test]
    fn update_state_up_to_date_none_checked_at() {
        let state = UpdateState::UpToDate { checked_at: None };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "up_to_date");
    }

    #[test]
    fn update_state_available_serialization() {
        let state = UpdateState::Available {
            current: "5.0.0".to_string(),
            latest: "5.1.0".to_string(),
            release_url: "https://example.com/release".to_string(),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
            payload: PayloadState::NotReady,
            checked_at: Utc::now(),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "available");
        assert_eq!(json["current"], "5.0.0");
        assert_eq!(json["latest"], "5.1.0");
        // PayloadState is internally tagged, nested as {"payload": "not_ready"}
        assert_eq!(json["payload"]["payload"], "not_ready");
    }

    #[test]
    fn update_state_draining_serialization() {
        let state = UpdateState::Draining {
            latest: "5.1.0".to_string(),
            in_flight: 5,
            requested_at: Utc::now(),
            timeout_at: Utc::now() + chrono::Duration::seconds(300),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "draining");
        assert_eq!(json["in_flight"], 5);
    }

    #[test]
    fn update_state_failed_serialization() {
        let state = UpdateState::Failed {
            latest: Some("5.1.0".to_string()),
            release_url: Some("https://example.com/release".to_string()),
            message: "download failed".to_string(),
            failed_at: Utc::now(),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "failed");
        assert_eq!(json["message"], "download failed");
    }

    #[test]
    fn update_state_failed_with_none_fields() {
        let state = UpdateState::Failed {
            latest: None,
            release_url: None,
            message: "unknown error".to_string(),
            failed_at: Utc::now(),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "failed");
        assert!(json["latest"].is_null());
        assert!(json["release_url"].is_null());
    }

    // =======================================================================
    // PayloadState serialization
    // =======================================================================
    #[test]
    fn payload_state_not_ready_serialization() {
        let ps = PayloadState::NotReady;
        let json = serde_json::to_value(&ps).unwrap();
        assert_eq!(json["payload"], "not_ready");
    }

    #[test]
    fn payload_state_downloading_serialization() {
        let ps = PayloadState::Downloading {
            started_at: Utc::now(),
            downloaded_bytes: Some(1024),
            total_bytes: Some(2048),
        };
        let json = serde_json::to_value(&ps).unwrap();
        assert_eq!(json["payload"], "downloading");
        assert_eq!(json["downloaded_bytes"], 1024);
        assert_eq!(json["total_bytes"], 2048);
    }

    #[test]
    fn payload_state_downloading_skips_none_bytes() {
        let ps = PayloadState::Downloading {
            started_at: Utc::now(),
            downloaded_bytes: None,
            total_bytes: None,
        };
        let json = serde_json::to_value(&ps).unwrap();
        assert_eq!(json["payload"], "downloading");
        // skip_serializing_if = "Option::is_none" means no key at all
        assert!(json.get("downloaded_bytes").is_none());
        assert!(json.get("total_bytes").is_none());
    }

    #[test]
    fn payload_state_ready_portable_serialization() {
        let ps = PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        };
        let json = serde_json::to_value(&ps).unwrap();
        assert_eq!(json["payload"], "ready");
    }

    #[test]
    fn payload_state_error_serialization() {
        let ps = PayloadState::Error {
            message: "download failed".to_string(),
        };
        let json = serde_json::to_value(&ps).unwrap();
        assert_eq!(json["payload"], "error");
        assert_eq!(json["message"], "download failed");
    }

    // =======================================================================
    // PayloadKind serialization
    // =======================================================================
    #[test]
    fn payload_kind_portable_serialization() {
        let kind = PayloadKind::Portable {
            binary_path: "/usr/local/bin/llmlb".to_string(),
        };
        let json = serde_json::to_value(&kind).unwrap();
        // Externally tagged: {"portable": {"binary_path": "..."}}
        assert_eq!(json["portable"]["binary_path"], "/usr/local/bin/llmlb");
    }

    #[test]
    fn payload_kind_installer_serialization() {
        let kind = PayloadKind::Installer {
            installer_path: "/tmp/llmlb-setup.exe".to_string(),
            kind: InstallerKind::WindowsSetup,
        };
        let json = serde_json::to_value(&kind).unwrap();
        // Externally tagged: {"installer": {"installer_path": "...", "kind": "..."}}
        assert_eq!(json["installer"]["installer_path"], "/tmp/llmlb-setup.exe");
        assert_eq!(json["installer"]["kind"], "windows_setup");
    }

    // =======================================================================
    // InstallerKind serialization
    // =======================================================================
    #[test]
    fn installer_kind_serialization() {
        let mac = InstallerKind::MacPkg;
        let win = InstallerKind::WindowsSetup;
        assert_eq!(
            serde_json::to_value(&mac).unwrap(),
            serde_json::json!("mac_pkg")
        );
        assert_eq!(
            serde_json::to_value(&win).unwrap(),
            serde_json::json!("windows_setup")
        );
    }

    // =======================================================================
    // ApplyMethod serialization
    // =======================================================================
    #[test]
    fn apply_method_serialization() {
        assert_eq!(
            serde_json::to_value(&ApplyMethod::PortableReplace).unwrap(),
            serde_json::json!("portable_replace")
        );
        assert_eq!(
            serde_json::to_value(&ApplyMethod::MacPkg).unwrap(),
            serde_json::json!("mac_pkg")
        );
        assert_eq!(
            serde_json::to_value(&ApplyMethod::WindowsSetup).unwrap(),
            serde_json::json!("windows_setup")
        );
    }

    // =======================================================================
    // RestartArgsFile serialization
    // =======================================================================
    #[test]
    fn restart_args_file_roundtrip() {
        let raf = RestartArgsFile {
            args: vec![
                "serve".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ],
            cwd: "/home/user".to_string(),
        };
        let json = serde_json::to_string(&raf).unwrap();
        let deserialized: RestartArgsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.args, raf.args);
        assert_eq!(deserialized.cwd, raf.cwd);
    }

    #[test]
    fn restart_args_file_empty_args() {
        let raf = RestartArgsFile {
            args: vec![],
            cwd: ".".to_string(),
        };
        let json = serde_json::to_string(&raf).unwrap();
        let deserialized: RestartArgsFile = serde_json::from_str(&json).unwrap();
        assert!(deserialized.args.is_empty());
    }

    // =======================================================================
    // write_restart_args_file
    // =======================================================================
    #[test]
    fn write_restart_args_file_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let update_dir = dir.path().join("updates").join("5.0.0");
        let result = write_restart_args_file(&update_dir);
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "restart_args.json");

        // Verify content is valid JSON
        let content = fs::read_to_string(&path).unwrap();
        let parsed: RestartArgsFile = serde_json::from_str(&content).unwrap();
        assert!(!parsed.cwd.is_empty());
    }

    // =======================================================================
    // parse_port_from_args
    // =======================================================================
    #[test]
    fn parse_port_from_args_flag_style() {
        let args = vec![
            "serve".to_string(),
            "--port".to_string(),
            "9090".to_string(),
        ];
        assert_eq!(parse_port_from_args(&args), Some(9090));
    }

    #[test]
    fn parse_port_from_args_short_flag() {
        let args = vec!["serve".to_string(), "-p".to_string(), "9090".to_string()];
        assert_eq!(parse_port_from_args(&args), Some(9090));
    }

    #[test]
    fn parse_port_from_args_equals_style() {
        let args = vec!["serve".to_string(), "--port=12345".to_string()];
        assert_eq!(parse_port_from_args(&args), Some(12345));
    }

    #[test]
    fn parse_port_from_args_no_port() {
        let args = vec![
            "serve".to_string(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
        ];
        assert_eq!(parse_port_from_args(&args), None);
    }

    #[test]
    fn parse_port_from_args_empty() {
        let args: Vec<String> = vec![];
        assert_eq!(parse_port_from_args(&args), None);
    }

    #[test]
    fn parse_port_from_args_invalid_port_value() {
        let args = vec![
            "serve".to_string(),
            "--port".to_string(),
            "not_a_number".to_string(),
        ];
        assert_eq!(parse_port_from_args(&args), None);
    }

    #[test]
    fn parse_port_from_args_port_at_end_without_value() {
        let args = vec!["serve".to_string(), "--port".to_string()];
        assert_eq!(parse_port_from_args(&args), None);
    }

    // =======================================================================
    // detect_server_port
    // =======================================================================
    #[test]
    fn detect_server_port_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let nonexistent = dir.path().join("nonexistent.json");
        let port = detect_server_port(&nonexistent);
        assert_eq!(port, DEFAULT_LISTEN_PORT);
    }

    #[test]
    fn detect_server_port_from_args_file_reads_port() {
        let dir = tempfile::tempdir().unwrap();
        let args_file = dir.path().join("restart_args.json");
        let args = RestartArgsFile {
            args: vec!["serve".to_string(), "-p".to_string(), "55555".to_string()],
            cwd: dir.path().to_string_lossy().to_string(),
        };
        fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();
        assert_eq!(detect_server_port(&args_file), 55555);
    }

    // =======================================================================
    // find_extracted_binary
    // =======================================================================
    #[test]
    fn find_extracted_binary_at_root() {
        let dir = tempfile::tempdir().unwrap();
        let binary_path = dir.path().join("llmlb");
        fs::write(&binary_path, b"binary content").unwrap();

        let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), binary_path);
    }

    #[test]
    fn find_extracted_binary_in_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let sub_dir = dir.path().join("llmlb-linux-x86_64");
        fs::create_dir_all(&sub_dir).unwrap();
        let binary_path = sub_dir.join("llmlb");
        fs::write(&binary_path, b"binary content").unwrap();

        let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), binary_path);
    }

    #[test]
    fn find_extracted_binary_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn find_extracted_binary_deep_nested() {
        let dir = tempfile::tempdir().unwrap();
        let deep_dir = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep_dir).unwrap();
        let binary_path = deep_dir.join("llmlb");
        fs::write(&binary_path, b"binary content").unwrap();

        let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
        assert!(result.is_some());
    }

    // =======================================================================
    // extract_archive: unsupported format
    // =======================================================================
    #[test]
    fn extract_archive_unsupported_format_fails() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("archive.7z");
        fs::write(&archive_path, b"some content").unwrap();
        let dest = dir.path().join("extract");
        fs::create_dir_all(&dest).unwrap();

        let result = extract_archive(&archive_path, &dest);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported archive format"));
    }

    // =======================================================================
    // UpdateManager: state transitions
    // =======================================================================
    #[tokio::test]
    async fn update_manager_initial_state_is_up_to_date() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        match manager.state().await {
            UpdateState::UpToDate { checked_at } => {
                assert!(checked_at.is_none());
            }
            other => panic!("expected up_to_date, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn set_applying_state_updates_correctly() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let started = Utc::now();
        manager
            .set_applying_state(
                "5.0.0",
                ApplyMethod::PortableReplace,
                ApplyPhase::Starting,
                started,
                None,
            )
            .await;

        match manager.state().await {
            UpdateState::Applying {
                latest,
                method,
                phase,
                timeout_at,
                ..
            } => {
                assert_eq!(latest, "5.0.0");
                assert_eq!(method, ApplyMethod::PortableReplace);
                assert_eq!(phase, ApplyPhase::Starting);
                assert!(timeout_at.is_none());
            }
            other => panic!("expected applying, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn set_payload_error_sets_error_on_available_state() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        manager
            .set_payload_error("download timeout".to_string())
            .await;

        match manager.state().await {
            UpdateState::Available { payload, .. } => {
                assert_eq!(
                    payload,
                    PayloadState::Error {
                        message: "download timeout".to_string()
                    }
                );
            }
            other => panic!("expected available with error payload, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn set_payload_error_noop_on_non_available_state() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        // State is UpToDate (default), set_payload_error should be a no-op
        manager.set_payload_error("some error".to_string()).await;

        match manager.state().await {
            UpdateState::UpToDate { .. } => {} // unchanged
            other => panic!("expected up_to_date unchanged, got {other:?}"),
        }
    }

    // =======================================================================
    // UpdateManager: require_ready_payload
    // =======================================================================
    #[tokio::test]
    async fn require_ready_payload_returns_kind_when_ready() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let expected_kind = PayloadKind::Portable {
            binary_path: "/tmp/new-llmlb".to_string(),
        };
        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::Ready {
                    kind: expected_kind.clone(),
                });
        }

        let kind = manager.require_ready_payload().await.unwrap();
        assert_eq!(kind, expected_kind);
    }

    #[tokio::test]
    async fn require_ready_payload_errors_when_not_ready() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        {
            *manager.inner.state.write().await =
                available_state_with_payload(PayloadState::NotReady);
        }

        let err = manager.require_ready_payload().await.unwrap_err();
        assert!(err.to_string().contains("not ready"));
    }

    #[tokio::test]
    async fn require_ready_payload_errors_when_no_update() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let err = manager.require_ready_payload().await.unwrap_err();
        assert!(err.to_string().contains("No update is available"));
    }

    // =======================================================================
    // UpdateManager: validate_force_apply_request
    // =======================================================================
    #[tokio::test]
    async fn validate_force_apply_rejects_draining_state() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        {
            *manager.inner.state.write().await = UpdateState::Draining {
                latest: "5.0.0".to_string(),
                in_flight: 3,
                requested_at: Utc::now(),
                timeout_at: Utc::now() + chrono::Duration::seconds(300),
            };
        }

        let err = manager.validate_force_apply_request().await.unwrap_err();
        assert!(err.to_string().contains("already in progress"));
    }

    #[tokio::test]
    async fn validate_force_apply_rejects_up_to_date() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let err = manager.validate_force_apply_request().await.unwrap_err();
        assert!(err.to_string().contains("No update is available"));
    }

    // =======================================================================
    // UpdateManager: apply_cache
    // =======================================================================
    #[tokio::test]
    async fn apply_cache_empty_version_stays_up_to_date() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("".to_string()),
            release_url: None,
            portable_asset_url: None,
            installer_asset_url: None,
        };
        manager.apply_cache(cache).await.unwrap();

        match manager.state().await {
            UpdateState::UpToDate { checked_at } => {
                assert!(checked_at.is_some());
            }
            other => panic!("expected up_to_date, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_cache_none_version_stays_up_to_date() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: None,
            release_url: None,
            portable_asset_url: None,
            installer_asset_url: None,
        };
        manager.apply_cache(cache).await.unwrap();

        assert!(matches!(
            manager.state().await,
            UpdateState::UpToDate { .. }
        ));
    }

    #[tokio::test]
    async fn apply_cache_invalid_version_errors() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("not-semver".to_string()),
            release_url: None,
            portable_asset_url: None,
            installer_asset_url: None,
        };
        let err = manager.apply_cache(cache).await;
        assert!(err.is_err());
    }

    // =======================================================================
    // UpdateManager: record_check_failure from Draining and Applying states
    // =======================================================================
    #[tokio::test]
    async fn record_check_failure_from_draining_preserves_latest() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        {
            *manager.inner.state.write().await = UpdateState::Draining {
                latest: "5.0.0".to_string(),
                in_flight: 2,
                requested_at: Utc::now(),
                timeout_at: Utc::now() + chrono::Duration::seconds(300),
            };
        }

        manager
            .record_check_failure("error during drain".to_string())
            .await;

        match manager.state().await {
            UpdateState::Failed {
                latest, message, ..
            } => {
                assert_eq!(latest, Some("5.0.0".to_string()));
                assert_eq!(message, "error during drain");
            }
            other => panic!("expected failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn record_check_failure_from_applying_preserves_latest() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        {
            *manager.inner.state.write().await = UpdateState::Applying {
                latest: "5.0.0".to_string(),
                method: ApplyMethod::PortableReplace,
                phase: ApplyPhase::Starting,
                phase_message: "test".to_string(),
                started_at: Utc::now(),
                timeout_at: None,
            };
        }

        manager
            .record_check_failure("error during apply".to_string())
            .await;

        match manager.state().await {
            UpdateState::Failed { latest, .. } => {
                assert_eq!(latest, Some("5.0.0".to_string()));
            }
            other => panic!("expected failed, got {other:?}"),
        }
    }

    // =======================================================================
    // UpdateManager: start_background_tasks is idempotent
    // =======================================================================
    #[tokio::test]
    async fn start_background_tasks_is_idempotent() {
        let manager = UpdateManager::new(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
        )
        .unwrap();

        // First call should not panic
        manager.start_background_tasks();
        // Second call should be a no-op (idempotent)
        manager.start_background_tasks();
        // Just verify it doesn't panic or deadlock
        assert!(manager.inner.started.load(Ordering::SeqCst));
    }

    // =======================================================================
    // UpdateManager: cancel_schedule errors when no schedule exists
    // =======================================================================
    #[test]
    fn cancel_schedule_errors_when_empty() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        let err = manager
            .cancel_schedule()
            .expect_err("should error when no schedule");
        assert!(err.to_string().contains("No schedule exists"));
    }

    // =======================================================================
    // UpdateManager: create_schedule errors on duplicate
    // =======================================================================
    #[test]
    fn create_schedule_errors_on_duplicate() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        let sched = schedule::UpdateSchedule {
            mode: schedule::ScheduleMode::Idle,
            scheduled_at: None,
            scheduled_by: "admin".to_string(),
            target_version: "5.0.0".to_string(),
            created_at: Utc::now(),
        };

        manager.create_schedule(sched.clone()).unwrap();

        let err = manager
            .create_schedule(sched)
            .expect_err("should error on duplicate schedule");
        assert!(err.to_string().contains("already exists"));
    }

    // =======================================================================
    // UpdateManager: history roundtrip
    // =======================================================================
    #[test]
    fn record_and_get_history() {
        let gate = InferenceGate::default();
        let (manager, _tmp) = test_manager_with_gate(gate);

        assert!(manager.get_history().is_empty());

        manager.record_history(history::HistoryEntry {
            kind: history::HistoryEventKind::Applied,
            version: "5.0.0".to_string(),
            message: Some("applied successfully".to_string()),
            timestamp: Utc::now(),
        });

        let h = manager.get_history();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].version, "5.0.0");
    }

    // =======================================================================
    // UpdateManager: new_with_data_dir isolation
    // =======================================================================
    #[test]
    fn new_with_data_dir_uses_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let manager = UpdateManager::new_with_data_dir(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
            dir.path(),
        )
        .unwrap();

        assert!(manager.inner.cache_path.starts_with(dir.path()));
        assert!(manager.inner.updates_dir.starts_with(dir.path()));
    }

    #[tokio::test]
    async fn check_only_github_error_cache_fallback() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // GitHub API が 429 を返すようモック
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/releases/latest"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&mock_server)
            .await;

        let tmp = tempfile::tempdir().expect("create temp dir");
        let manager = UpdateManager::new_with_data_dir_and_config(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
            tmp.path(),
            Some(mock_server.uri()),
        )
        .expect("create update manager");

        // --- ケース1: キャッシュなし → エラーが返る ---
        assert!(
            !manager.inner.cache_path.exists(),
            "cache should not exist in fresh temp dir"
        );
        let result = manager.check_only(true).await;
        assert!(
            result.is_err(),
            "check_only should fail when no cache and GitHub returns 429"
        );

        // --- ケース2: キャッシュあり → フォールバックで成功 ---
        save_cache(
            &manager.inner.cache_path,
            UpdateCacheFile {
                last_checked_at: Utc::now(),
                latest_version: Some("99.0.0".to_string()),
                release_url: Some(
                    "https://github.com/test-owner/test-repo/releases/tag/v99.0.0".to_string(),
                ),
                portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
                installer_asset_url: None,
            },
        )
        .expect("save cache");

        // force=true でもキャッシュフォールバックすべき
        let state = manager
            .check_only(true)
            .await
            .expect("check_only should succeed via cache fallback");

        match &state {
            UpdateState::Available { latest, .. } => {
                assert_eq!(latest, "99.0.0");
            }
            other => panic!("expected Available from cache fallback, got {other:?}"),
        }

        // --- ケース3: 既にAvailable(payload=Ready)なら状態を保持 ---
        {
            let mut st = manager.inner.state.write().await;
            *st = UpdateState::Available {
                current: "5.0.0".to_string(),
                latest: "99.0.0".to_string(),
                release_url: "https://example.com/release".to_string(),
                portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
                installer_asset_url: None,
                payload: PayloadState::Ready {
                    kind: PayloadKind::Portable {
                        binary_path: "/tmp/llmlb-new".to_string(),
                    },
                },
                checked_at: Utc::now(),
            };
        }

        let state = manager
            .check_only(true)
            .await
            .expect("check_only should preserve existing Available state");

        match &state {
            UpdateState::Available { payload, .. } => {
                assert!(
                    matches!(payload, PayloadState::Ready { .. }),
                    "payload should remain Ready, got {payload:?}"
                );
            }
            other => panic!("expected Available with Ready payload, got {other:?}"),
        }
    }

    /// Regression test: `check_and_maybe_download` must not deadlock by holding
    /// the state write guard across the `ensure_payload_ready().await` call.
    ///
    /// Before the fix, the write guard in `check_and_maybe_download` was not
    /// dropped before calling `ensure_payload_ready`, which tried to acquire a
    /// read lock on the same `RwLock` — causing an irrecoverable deadlock.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn check_and_maybe_download_does_not_deadlock() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Return a release with a version higher than current.
        let release_json = serde_json::json!({
            "tag_name": "v99.0.0",
            "html_url": "https://github.com/test-owner/test-repo/releases/tag/v99.0.0",
            "assets": []
        });
        Mock::given(method("GET"))
            .and(path("/repos/akiojin/llmlb/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&release_json))
            .mount(&mock_server)
            .await;

        let tmp = tempfile::tempdir().expect("create temp dir");
        let manager = UpdateManager::new_with_data_dir_and_config(
            reqwest::Client::new(),
            InferenceGate::default(),
            ShutdownController::default(),
            tmp.path(),
            Some(mock_server.uri()),
        )
        .expect("create update manager");

        // check_and_maybe_download with force=true triggers the code path
        // that previously deadlocked. Use a timeout to detect deadlocks.
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            manager.check_and_maybe_download(true),
        )
        .await;

        assert!(
            result.is_ok(),
            "check_and_maybe_download should not deadlock (timed out)"
        );

        // Verify we can still read state (would hang if write lock is held).
        let state = tokio::time::timeout(Duration::from_secs(2), manager.state())
            .await
            .expect("state() should not deadlock");

        match &state {
            UpdateState::Available { latest, .. } => {
                assert_eq!(latest, "99.0.0");
            }
            other => panic!("expected Available state, got {other:?}"),
        }
    }
}
