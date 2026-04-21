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
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tracing::{debug, warn};

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

/// 指定PIDのプロセスが存在するか確認
///
/// # Arguments
///
/// * `pid` - 確認対象のプロセスID
///
/// # Returns
///
/// プロセスが存在する場合は `true`、存在しない場合は `false`
pub fn is_process_running(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.process(Pid::from_u32(pid)).is_some()
}

/// ロックファイルからロック情報を読み取る
///
/// # Arguments
///
/// * `port` - サーバーのリッスンポート番号
///
/// # Returns
///
/// - `Ok(Some(LockInfo))`: ロックファイルが存在し、正常に読み取れた場合
/// - `Ok(None)`: ロックファイルが存在しない場合
/// - `Err(LockError::Corrupted)`: ロックファイルが破損している場合
/// - `Err(LockError::FileLocked)`: Windowsでファイルがロック中の場合
pub fn read_lock_info(port: u16) -> Result<Option<LockInfo>, LockError> {
    let path = lock_path(port);
    if !path.exists() {
        return Ok(None);
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            // Windowsでファイルがロック中の場合 (ERROR_LOCK_VIOLATION = 33)
            // または他のプロセスがファイルを使用中の場合 (ERROR_SHARING_VIOLATION = 32)
            #[cfg(windows)]
            {
                if let Some(code) = e.raw_os_error() {
                    if code == 33 || code == 32 {
                        // ファイルがロックされている = 誰かが使用中
                        return Err(LockError::FileLocked { port });
                    }
                }
            }
            return Err(LockError::Corrupted(format!(
                "Failed to read lock file: {}",
                e
            )));
        }
    };

    let info: LockInfo = serde_json::from_str(&content)
        .map_err(|e| LockError::Corrupted(format!("Invalid JSON in lock file: {}", e)))?;

    Ok(Some(info))
}

/// 全てのロックファイルを列挙し、生存中のサーバー情報を返す
///
/// # Returns
///
/// 生存中のサーバーの `LockInfo` のベクタ。
/// ロックファイルが存在しないか、すべてのサーバーが停止している場合は空のベクタを返します。
pub fn list_all_locks() -> Vec<LockInfo> {
    let dir = lock_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut locks = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                // serve_*.lock パターンにマッチするかチェック
                if filename.starts_with("serve_") && filename.ends_with(".lock") {
                    // ポート番号を抽出
                    let port_str = filename
                        .trim_start_matches("serve_")
                        .trim_end_matches(".lock");
                    if let Ok(port) = port_str.parse::<u16>() {
                        // ロック情報を読み取り
                        match read_lock_info(port) {
                            // PIDが生存中のもののみ追加
                            Ok(Some(info)) if is_process_running(info.pid) => {
                                locks.push(info);
                            }
                            // Windowsでファイルがロック中の場合
                            Err(LockError::FileLocked { port }) => {
                                // ロック中 = 誰かが使用中なのでリストに追加
                                // PIDは不明なので0、時刻は現在時刻
                                locks.push(LockInfo {
                                    pid: 0,
                                    started_at: chrono::Utc::now(),
                                    port,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // ポート番号でソート
    locks.sort_by_key(|info| info.port);
    locks
}

/// 指定PIDのプロセスを停止する
///
/// # Arguments
///
/// * `pid` - 停止対象のプロセスID
///
/// # Returns
///
/// - `Ok(())`: シグナル送信に成功した場合
/// - `Err`: シグナル送信に失敗した場合
///
/// # Platform
///
/// - Unix: SIGTERM を送信
/// - Windows: taskkill /PID /F を実行
#[cfg(unix)]
pub fn stop_process(pid: u32) -> Result<(), std::io::Error> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// 指定されたPIDのプロセスを停止します (Windows版)
///
/// # Arguments
/// * `pid` - 停止するプロセスのPID
///
/// # Returns
/// * `Ok(())` - プロセスの強制終了に成功
/// * `Err` - プロセスの終了に失敗
#[cfg(windows)]
pub fn stop_process(pid: u32) -> Result<(), std::io::Error> {
    use std::process::Command;

    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // T005: LockInfo シリアライズ/デシリアライズテスト
    #[test]
    fn test_lock_info_serialize_deserialize() {
        let info = LockInfo {
            pid: 12345,
            started_at: Utc.with_ymd_and_hms(2026, 1, 30, 12, 0, 0).unwrap(),
            port: 8000,
        };

        // シリアライズ
        let json = serde_json::to_string(&info).expect("Failed to serialize");
        assert!(json.contains("12345"));
        assert!(json.contains("8000"));

        // デシリアライズ
        let deserialized: LockInfo = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_lock_info_json_format() {
        let info = LockInfo {
            pid: 12345,
            started_at: Utc.with_ymd_and_hms(2026, 1, 30, 12, 0, 0).unwrap(),
            port: 8000,
        };

        let json = serde_json::to_string_pretty(&info).expect("Failed to serialize");
        // JSON形式が仕様通りであることを確認
        assert!(json.contains("\"pid\": 12345"));
        assert!(json.contains("\"port\": 8000"));
        assert!(json.contains("\"started_at\""));
    }

    // T006: lock_dir() と lock_path() テスト
    #[test]
    fn test_lock_dir_is_in_temp() {
        let dir = lock_dir();
        // 一時ディレクトリ配下であることを確認
        assert!(dir.starts_with(std::env::temp_dir()));
        assert!(dir.ends_with("llmlb"));
    }

    #[test]
    fn test_lock_path_includes_port() {
        let path = lock_path(8000);
        assert!(path.to_string_lossy().contains("serve_8000.lock"));

        let path2 = lock_path(32768);
        assert!(path2.to_string_lossy().contains("serve_32768.lock"));
    }

    #[test]
    fn test_lock_path_is_under_lock_dir() {
        let path = lock_path(8000);
        assert!(path.starts_with(lock_dir()));
    }

    // T007: is_process_running() テスト
    #[test]
    fn test_is_process_running_self() {
        // 現在のプロセス（自分自身）は存在するはず
        let current_pid = std::process::id();
        assert!(is_process_running(current_pid));
    }

    #[test]
    fn test_is_process_running_nonexistent() {
        // 存在しないPID（非常に大きな値）
        // 注意: PID 0 はカーネルプロセスの可能性があるため避ける
        let nonexistent_pid = u32::MAX - 1;
        assert!(!is_process_running(nonexistent_pid));
    }

    // read_lock_info テスト
    #[test]
    fn test_read_lock_info_nonexistent() {
        // 存在しないポートのロックファイル
        let result = read_lock_info(59999);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_read_lock_info_valid_file() {
        // テスト用のロックファイルを作成
        let port = 58888;
        let path = lock_path(port);

        // ディレクトリを作成
        std::fs::create_dir_all(lock_dir()).ok();

        // 有効なJSONを書き込み
        let info = LockInfo {
            pid: 12345,
            started_at: Utc::now(),
            port,
        };
        std::fs::write(&path, serde_json::to_string(&info).unwrap()).unwrap();

        // 読み取りテスト
        let result = read_lock_info(port);
        assert!(result.is_ok());
        let read_info = result.unwrap().unwrap();
        assert_eq!(read_info.pid, 12345);
        assert_eq!(read_info.port, port);

        // クリーンアップ
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_read_lock_info_corrupted_file() {
        // テスト用の破損ロックファイルを作成
        let port = 58887;
        let path = lock_path(port);

        // ディレクトリを作成
        std::fs::create_dir_all(lock_dir()).ok();

        // 無効なJSONを書き込み
        std::fs::write(&path, "not valid json").unwrap();

        // 読み取りテスト
        let result = read_lock_info(port);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LockError::Corrupted(_)));

        // クリーンアップ
        std::fs::remove_file(&path).ok();
    }

    // T013: ServerLock::acquire() テスト
    #[test]
    fn test_server_lock_acquire_success() {
        let port = 57777;
        let path = lock_path(port);

        // 既存のロックファイルがあれば削除
        std::fs::remove_file(&path).ok();

        // ロック取得
        let lock = ServerLock::acquire(port);
        assert!(lock.is_ok());
        let lock = lock.unwrap();

        // ロック情報を確認
        assert_eq!(lock.info().port, port);
        assert_eq!(lock.info().pid, std::process::id());

        // ロックファイルが作成されていることを確認
        assert!(path.exists());

        // JSONが正しいことを確認（Windowsではファイルロック中は読み取れないためスキップ）
        #[cfg(not(windows))]
        {
            let content = std::fs::read_to_string(&path).unwrap();
            let info: LockInfo = serde_json::from_str(&content).unwrap();
            assert_eq!(info.port, port);
        }

        // ロック解除（Dropで自動解除）
        drop(lock);

        // ロックファイルが削除されていることを確認
        assert!(!path.exists());
    }

    // T014: 重複ロック取得テスト
    #[test]
    fn test_server_lock_acquire_already_running() {
        let port = 57776;
        let path = lock_path(port);

        // 既存のロックファイルがあれば削除
        std::fs::remove_file(&path).ok();

        // 1つ目のロックを取得
        let lock1 = ServerLock::acquire(port);
        assert!(lock1.is_ok());
        let _lock1 = lock1.unwrap();

        // 2つ目のロック取得を試行（失敗するはず）
        let lock2 = ServerLock::acquire(port);
        assert!(lock2.is_err());

        let err = lock2.unwrap_err();
        match err {
            LockError::AlreadyRunning {
                port: err_port,
                pid,
                ..
            } => {
                assert_eq!(err_port, port);
                assert_eq!(pid, std::process::id());
            }
            // Windowsではファイルロック自体がブロックするため、FileLockedエラーになる
            #[cfg(windows)]
            LockError::FileLocked { port: err_port } => {
                assert_eq!(err_port, port);
            }
            _ => panic!(
                "Expected AlreadyRunning or FileLocked error, got: {:?}",
                err
            ),
        }
    }

    // T017: ServerLock::release() テスト
    #[test]
    fn test_server_lock_release() {
        let port = 57775;
        let path = lock_path(port);

        // 既存のロックファイルがあれば削除
        std::fs::remove_file(&path).ok();

        // ロック取得
        let lock = ServerLock::acquire(port).unwrap();
        assert!(path.exists());

        // 明示的にリリース
        let result = lock.release();
        assert!(result.is_ok());

        // ロックファイルが削除されていることを確認
        assert!(!path.exists());
    }

    // T026: 残留ロック自動解除テスト
    #[test]
    fn test_server_lock_stale_lock_cleanup() {
        let port = 57774;
        let path = lock_path(port);

        // ディレクトリを作成
        std::fs::create_dir_all(lock_dir()).ok();

        // 存在しないPIDのロックファイルを作成
        let stale_info = LockInfo {
            pid: u32::MAX - 1, // 存在しないPID
            started_at: Utc::now(),
            port,
        };
        std::fs::write(&path, serde_json::to_string(&stale_info).unwrap()).unwrap();

        // ロック取得（残留ロックが自動削除されるはず）
        let lock = ServerLock::acquire(port);
        assert!(lock.is_ok());
        let lock = lock.unwrap();

        // 新しいPIDで取得されていることを確認
        assert_eq!(lock.info().pid, std::process::id());

        // クリーンアップ
        drop(lock);
    }

    // T028: Dropトレイトテスト
    #[test]
    fn test_server_lock_drop_releases_lock() {
        let port = 57773;
        let path = lock_path(port);

        // 既存のロックファイルがあれば削除
        std::fs::remove_file(&path).ok();

        {
            // スコープ内でロック取得
            let _lock = ServerLock::acquire(port).unwrap();
            assert!(path.exists());
        }
        // スコープを抜けるとDropが呼ばれる

        // ロックファイルが削除されていることを確認
        assert!(!path.exists());
    }

    // T023: list_all_locks() テスト
    #[test]
    fn test_list_all_locks_empty() {
        // ロックディレクトリが存在しない場合
        let locks = list_all_locks();
        // 他のテストが実行中でなければ空
        // 注意: 並列実行時は他のテストのロックが見える可能性がある
        assert!(locks.is_empty() || locks.iter().all(|l| is_process_running(l.pid)));
    }

    #[test]
    fn test_list_all_locks_with_active_lock() {
        let port = 57772;
        let path = lock_path(port);

        // 既存のロックファイルがあれば削除
        std::fs::remove_file(&path).ok();

        // ロックを取得
        let _lock = ServerLock::acquire(port).unwrap();

        // list_all_locks で取得できることを確認
        let locks = list_all_locks();
        let found = locks.iter().find(|l| l.port == port);
        assert!(found.is_some());

        // WindowsではFileLocked時にPID=0が設定される
        #[cfg(not(windows))]
        assert_eq!(found.unwrap().pid, std::process::id());
        #[cfg(windows)]
        {
            // WindowsではPIDが0（ファイルロック中で読み取れない）または実際のPID
            let pid = found.unwrap().pid;
            assert!(pid == 0 || pid == std::process::id());
        }
    }

    // =======================================================================
    // LockError Display / Debug tests
    // =======================================================================
    #[test]
    fn lock_error_already_running_display() {
        let err = LockError::AlreadyRunning {
            port: 8080,
            pid: 1234,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("8080"));
        assert!(msg.contains("1234"));
        assert!(msg.contains("llmlb stop --port 8080"));
        assert!(msg.contains("kill -TERM 1234"));
    }

    #[test]
    fn lock_error_acquire_failed_display() {
        let err = LockError::AcquireFailed(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("Failed to acquire lock"));
    }

    #[test]
    fn lock_error_release_failed_display() {
        let err = LockError::ReleaseFailed(std::io::Error::other("file busy"));
        let msg = format!("{}", err);
        assert!(msg.contains("Failed to release lock"));
    }

    #[test]
    fn lock_error_corrupted_display() {
        let err = LockError::Corrupted("invalid JSON".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Lock file corrupted"));
        assert!(msg.contains("invalid JSON"));
    }

    #[test]
    fn lock_error_directory_creation_failed_display() {
        let err = LockError::DirectoryCreationFailed(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "cannot create",
        ));
        let msg = format!("{}", err);
        assert!(msg.contains("Failed to create lock directory"));
    }

    #[test]
    fn lock_error_file_locked_display() {
        let err = LockError::FileLocked { port: 9090 };
        let msg = format!("{}", err);
        assert!(msg.contains("9090"));
        assert!(msg.contains("llmlb stop --port 9090"));
    }

    // =======================================================================
    // LockInfo equality
    // =======================================================================
    #[test]
    fn lock_info_equality() {
        let ts = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let a = LockInfo {
            pid: 100,
            started_at: ts,
            port: 8000,
        };
        let b = LockInfo {
            pid: 100,
            started_at: ts,
            port: 8000,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn lock_info_inequality_different_pid() {
        let ts = Utc::now();
        let a = LockInfo {
            pid: 100,
            started_at: ts,
            port: 8000,
        };
        let b = LockInfo {
            pid: 200,
            started_at: ts,
            port: 8000,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn lock_info_inequality_different_port() {
        let ts = Utc::now();
        let a = LockInfo {
            pid: 100,
            started_at: ts,
            port: 8000,
        };
        let b = LockInfo {
            pid: 100,
            started_at: ts,
            port: 9000,
        };
        assert_ne!(a, b);
    }

    // =======================================================================
    // LockInfo clone
    // =======================================================================
    #[test]
    fn lock_info_clone_is_equal() {
        let info = LockInfo {
            pid: 42,
            started_at: Utc::now(),
            port: 3000,
        };
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    // =======================================================================
    // lock_path: various ports
    // =======================================================================
    #[test]
    fn lock_path_port_zero() {
        let path = lock_path(0);
        assert!(path.to_string_lossy().contains("serve_0.lock"));
    }

    #[test]
    fn lock_path_port_max() {
        let path = lock_path(u16::MAX);
        assert!(path
            .to_string_lossy()
            .contains(&format!("serve_{}.lock", u16::MAX)));
    }

    #[test]
    fn lock_path_different_ports_are_different() {
        let p1 = lock_path(8000);
        let p2 = lock_path(8001);
        assert_ne!(p1, p2);
    }

    // =======================================================================
    // read_lock_info: empty file
    // =======================================================================
    #[test]
    fn test_read_lock_info_empty_file() {
        let port = 58886;
        let path = lock_path(port);
        std::fs::create_dir_all(lock_dir()).ok();
        std::fs::write(&path, "").unwrap();

        let result = read_lock_info(port);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LockError::Corrupted(_)));

        std::fs::remove_file(&path).ok();
    }

    // =======================================================================
    // read_lock_info: partial JSON
    // =======================================================================
    #[test]
    fn test_read_lock_info_partial_json() {
        let port = 58885;
        let path = lock_path(port);
        std::fs::create_dir_all(lock_dir()).ok();
        std::fs::write(&path, "{\"pid\": 123").unwrap();

        let result = read_lock_info(port);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LockError::Corrupted(_)));

        std::fs::remove_file(&path).ok();
    }

    // =======================================================================
    // read_lock_info: valid JSON with extra fields
    // =======================================================================
    #[test]
    fn test_read_lock_info_extra_fields_ignored() {
        let port = 58884;
        let path = lock_path(port);
        std::fs::create_dir_all(lock_dir()).ok();

        let json = serde_json::json!({
            "pid": 12345,
            "started_at": "2026-01-01T12:00:00Z",
            "port": port,
            "extra_field": "should be ignored"
        });
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let result = read_lock_info(port);
        assert!(result.is_ok());
        let info = result.unwrap().unwrap();
        assert_eq!(info.pid, 12345);
        assert_eq!(info.port, port);

        std::fs::remove_file(&path).ok();
    }

    // =======================================================================
    // ServerLock::info() accessor
    // =======================================================================
    #[test]
    fn server_lock_info_returns_correct_data() {
        let port = 57770;
        let path = lock_path(port);
        std::fs::remove_file(&path).ok();

        let lock = ServerLock::acquire(port).unwrap();
        let info = lock.info();
        assert_eq!(info.port, port);
        assert_eq!(info.pid, std::process::id());

        drop(lock);
    }

    // =======================================================================
    // ServerLock: Debug impl
    // =======================================================================
    #[test]
    fn server_lock_debug_format() {
        let port = 57769;
        let path = lock_path(port);
        std::fs::remove_file(&path).ok();

        let lock = ServerLock::acquire(port).unwrap();
        let debug = format!("{:?}", lock);
        assert!(debug.contains("ServerLock"));
        assert!(debug.contains("lock_path"));
        assert!(debug.contains("info"));

        drop(lock);
    }

    // =======================================================================
    // ServerLock: acquire + release + re-acquire
    // =======================================================================
    #[test]
    fn server_lock_reacquire_after_release() {
        let port = 57768;
        let path = lock_path(port);
        std::fs::remove_file(&path).ok();

        // First acquire
        let lock1 = ServerLock::acquire(port).unwrap();
        lock1.release().unwrap();

        // Re-acquire should succeed
        let lock2 = ServerLock::acquire(port).unwrap();
        assert_eq!(lock2.info().port, port);

        drop(lock2);
    }

    // =======================================================================
    // ServerLock: multiple different ports
    // =======================================================================
    #[test]
    fn server_lock_multiple_ports() {
        let port_a = 57767;
        let port_b = 57766;
        let path_a = lock_path(port_a);
        let path_b = lock_path(port_b);
        std::fs::remove_file(&path_a).ok();
        std::fs::remove_file(&path_b).ok();

        let lock_a = ServerLock::acquire(port_a).unwrap();
        let lock_b = ServerLock::acquire(port_b).unwrap();

        assert_eq!(lock_a.info().port, port_a);
        assert_eq!(lock_b.info().port, port_b);

        drop(lock_a);
        drop(lock_b);
    }

    // =======================================================================
    // LockInfo: JSON pretty-print consistency
    // =======================================================================
    #[test]
    fn lock_info_json_pretty_and_compact_roundtrip() {
        let info = LockInfo {
            pid: 99999,
            started_at: Utc.with_ymd_and_hms(2026, 6, 15, 8, 30, 0).unwrap(),
            port: 12345,
        };

        let pretty = serde_json::to_string_pretty(&info).unwrap();
        let compact = serde_json::to_string(&info).unwrap();

        let from_pretty: LockInfo = serde_json::from_str(&pretty).unwrap();
        let from_compact: LockInfo = serde_json::from_str(&compact).unwrap();

        assert_eq!(from_pretty, from_compact);
        assert_eq!(from_pretty, info);
    }

    // =======================================================================
    // lock_dir: consistent path
    // =======================================================================
    #[test]
    fn lock_dir_is_consistent() {
        let d1 = lock_dir();
        let d2 = lock_dir();
        assert_eq!(d1, d2);
    }

    // =======================================================================
    // is_process_running: PID 0 behavior (edge case)
    // =======================================================================
    #[test]
    fn is_process_running_pid_zero() {
        // PID 0 may or may not exist depending on OS
        // Just ensure it doesn't panic
        let _ = is_process_running(0);
    }

    #[test]
    fn test_list_all_locks_excludes_stale() {
        let port = 57771;
        let path = lock_path(port);

        // ディレクトリを作成
        std::fs::create_dir_all(lock_dir()).ok();

        // 存在しないPIDのロックファイルを作成
        let stale_info = LockInfo {
            pid: u32::MAX - 1,
            started_at: Utc::now(),
            port,
        };
        std::fs::write(&path, serde_json::to_string(&stale_info).unwrap()).unwrap();

        // list_all_locks で取得されないことを確認（PIDが存在しないため）
        let locks = list_all_locks();
        let found = locks.iter().find(|l| l.port == port);
        assert!(found.is_none());

        // クリーンアップ
        std::fs::remove_file(&path).ok();
    }
}
