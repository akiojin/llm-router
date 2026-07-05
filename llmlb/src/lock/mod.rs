//! サーバーインスタンスの排他制御（シングル実行制約）
//!
//! 同一ポートでのサーバー重複起動を防止するためのファイルロック機構を提供します。
//!
//! # 機能
//!
//! - クロスプラットフォームファイルロック（fs2）
//! - ロックファイルにJSON形式でPID・起動時刻・ポートを記録
//! - 残留ロックの自動検出と解除（PID検証）
//! - グレースフルシャットダウン対応（Dropトレイト）

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod guard;
mod inspect;
pub use guard::ServerLock;
pub use inspect::{is_process_running, list_all_locks, read_lock_info, stop_process};

/// ロックファイルに保存されるサーバー情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockInfo {
    /// サーバープロセスのPID
    pub pid: u32,
    /// サーバー起動時刻（UTC）
    pub started_at: DateTime<Utc>,
    /// リッスンポート番号
    pub port: u16,
}

/// ロック操作に関するエラー型
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// 同一ポートで既にサーバーが起動中
    #[error("Server already running on port {port} (PID: {pid}, started: {started_at})\n\nTo stop: llmlb stop --port {port}\nOr:      kill -TERM {pid}")]
    AlreadyRunning {
        /// ポート番号
        port: u16,
        /// 既存プロセスのPID
        pid: u32,
        /// 起動時刻
        started_at: DateTime<Utc>,
    },

    /// ロック取得に失敗
    #[error("Failed to acquire lock: {0}")]
    AcquireFailed(#[source] std::io::Error),

    /// ロック解除に失敗
    #[error("Failed to release lock: {0}")]
    ReleaseFailed(#[source] std::io::Error),

    /// ロックファイルが破損
    #[error("Lock file corrupted: {0}")]
    Corrupted(String),

    /// ロックディレクトリの作成に失敗
    #[error("Failed to create lock directory: {0}")]
    DirectoryCreationFailed(#[source] std::io::Error),

    /// ロックファイルが他のプロセスによってロック中 (Windows専用)
    #[error("Server already running on port {port} (lock file is held by another process)\n\nTo stop: llmlb stop --port {port}")]
    FileLocked {
        /// ポート番号
        port: u16,
    },
}

/// ロックディレクトリのパスを取得
///
/// OS標準の一時ディレクトリ配下に `llmlb` ディレクトリを返します。
/// - Unix: `/tmp/llmlb/`
/// - Windows: `%TEMP%\llmlb\`
pub fn lock_dir() -> PathBuf {
    std::env::temp_dir().join("llmlb")
}

/// 指定ポートのロックファイルパスを取得
///
/// # Arguments
///
/// * `port` - サーバーのリッスンポート番号
///
/// # Returns
///
/// ロックファイルのフルパス（例: `/tmp/llmlb/serve_8000.lock`）
pub fn lock_path(port: u16) -> PathBuf {
    lock_dir().join(format!("serve_{}.lock", port))
}

#[cfg(test)]
mod tests;
