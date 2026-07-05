//! 自プロセスのサーバーロックを RAII で管理する `ServerLock`。
//!
//! ロックファイルの取得・情報参照・解放をカプセル化する。他プロセスの
//! 観測・停止は `inspect` 側の責務。

use super::{is_process_running, lock_dir, lock_path, read_lock_info, LockError, LockInfo};
use chrono::Utc;
use fs2::FileExt;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tracing::{debug, warn};

/// サーバーのファイルロックを管理する構造体
///
/// RAIIパターンでロック解除を保証します。
/// スコープを抜けるか、明示的に`release()`を呼び出すとロックが解除されます。
pub struct ServerLock {
    /// ロックを保持しているファイルハンドル
    lock_file: Option<File>,
    /// ロックファイルのパス
    lock_path: PathBuf,
    /// ロック情報
    info: LockInfo,
}

impl std::fmt::Debug for ServerLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerLock")
            .field("lock_path", &self.lock_path)
            .field("info", &self.info)
            .finish()
    }
}

impl ServerLock {
    /// ロックを取得する
    ///
    /// # Arguments
    ///
    /// * `port` - サーバーのリッスンポート番号
    ///
    /// # Returns
    ///
    /// - `Ok(ServerLock)`: ロック取得に成功した場合
    /// - `Err(LockError::AlreadyRunning)`: 同一ポートで既にサーバーが起動中の場合
    /// - `Err(LockError::AcquireFailed)`: ロック取得に失敗した場合
    /// - `Err(LockError::DirectoryCreationFailed)`: ロックディレクトリの作成に失敗した場合
    ///
    /// # 残留ロックの処理
    ///
    /// ロックファイルが存在し、記録されたPIDのプロセスが存在しない場合は、
    /// 残留ロックとして自動的に削除し、新しいロックを取得します。
    pub fn acquire(port: u16) -> Result<Self, LockError> {
        let dir = lock_dir();
        let path = lock_path(port);

        // ロックディレクトリを作成（存在しない場合）
        std::fs::create_dir_all(&dir).map_err(LockError::DirectoryCreationFailed)?;

        // 既存ロックファイルをチェック
        if let Some(existing_info) = read_lock_info(port)? {
            if is_process_running(existing_info.pid) {
                // 既存プロセスが生存中 → エラー
                return Err(LockError::AlreadyRunning {
                    port: existing_info.port,
                    pid: existing_info.pid,
                    started_at: existing_info.started_at,
                });
            } else {
                // 残留ロック（PID不存在）→ 削除して続行
                warn!(
                    "Stale lock file detected (PID {} not running), cleaning up",
                    existing_info.pid
                );
                std::fs::remove_file(&path).map_err(LockError::AcquireFailed)?;
            }
        }

        // ロックファイルを作成/オープン
        let mut file = File::create(&path).map_err(LockError::AcquireFailed)?;

        // flockを取得（非ブロッキング）
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                // 他プロセスがロック保持中（競合状態で発生する可能性）
                LockError::AcquireFailed(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "Lock is held by another process",
                ))
            } else {
                LockError::AcquireFailed(e)
            }
        })?;

        // LockInfoを作成
        let info = LockInfo {
            pid: std::process::id(),
            started_at: Utc::now(),
            port,
        };

        // JSON形式で書き込み
        let json = serde_json::to_string_pretty(&info)
            .map_err(|e| LockError::AcquireFailed(std::io::Error::other(e)))?;
        file.write_all(json.as_bytes())
            .map_err(LockError::AcquireFailed)?;
        file.flush().map_err(LockError::AcquireFailed)?;

        // パーミッションを600に設定（Unixのみ）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, permissions).map_err(LockError::AcquireFailed)?;
        }

        debug!("Lock acquired for port {} (PID: {})", port, info.pid);

        Ok(Self {
            lock_file: Some(file),
            lock_path: path,
            info,
        })
    }

    /// ロック情報への参照を取得
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// ロックを明示的に解除する
    ///
    /// この関数を呼び出すと、ロックが解除されロックファイルが削除されます。
    /// Dropトレイトでも同様の処理が行われるため、通常は明示的に呼び出す必要はありません。
    pub fn release(mut self) -> Result<(), LockError> {
        self.release_internal()
    }

    /// 内部的なロック解除処理
    fn release_internal(&mut self) -> Result<(), LockError> {
        if let Some(file) = self.lock_file.take() {
            // flockを解除
            file.unlock().map_err(LockError::ReleaseFailed)?;
            drop(file);

            // ロックファイルを削除
            if self.lock_path.exists() {
                std::fs::remove_file(&self.lock_path).map_err(LockError::ReleaseFailed)?;
            }

            debug!("Lock released for port {}", self.info.port);
        }
        Ok(())
    }
}

impl Drop for ServerLock {
    fn drop(&mut self) {
        if let Err(e) = self.release_internal() {
            // panicしない - エラーはログのみ
            tracing::error!("Failed to release lock on drop: {}", e);
        }
    }
}
