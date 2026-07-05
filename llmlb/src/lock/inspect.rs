//! ロックファイル経由で他プロセスを観測・制御するユーティリティ。
//!
//! 稼働中サーバーの PID 生存確認・ロック情報の読み取り・全ロック列挙・
//! プロセス停止を担う。自プロセスの RAII ロックは `guard` 側の責務。

use super::{lock_dir, lock_path, LockError, LockInfo};

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
