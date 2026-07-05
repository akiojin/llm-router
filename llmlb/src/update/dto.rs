//! 自己アップデートの公開シリアライズ状態型（ダッシュボード/トレイ/API 向けデータモデル）
//!
//! arch-review [H6]: update/mod.rs の god-object 化に対し、状態機械ロジックから
//! データモデル（状態列挙）を分離。親は `pub use dto::*` で再エクスポートし、
//! crate::update::UpdateState 等の既存パスとテストの参照を維持する。

use chrono::{DateTime, Utc};
use serde::Serialize;

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
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Starting => "Preparing update apply",
            Self::WaitingOldProcessExit => "Waiting for current process handoff",
            Self::RunningInstaller => "Installer is running",
            Self::Restarting => "Restarting service",
        }
    }
}
