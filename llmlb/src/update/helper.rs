//! `__internal` ヘルパープロセス実装（バイナリ差し替え/インストーラ実行/ロールバック）
//!
//! arch-review [H6]: update/mod.rs から、再起動引数の永続化・PID 待機・バックアップ
//! 復元・ポート検出・ヘルスチェック監視・自動ロールバック履歴を含むヘルパー
//! プロセスのロジックを分離。親（UpdateManager）と cli/internal から参照される。

#[cfg(target_os = "macos")]
use super::macos_installer::run_macos_pkg_installer_with_privileges;
use super::{history, InstallerKind, DEFAULT_LISTEN_PORT};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct RestartArgsFile {
    pub(super) args: Vec<String>,
    pub(super) cwd: String,
}

pub(super) fn write_restart_args_file(update_dir: &Path) -> Result<PathBuf> {
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

pub(super) fn spawn_internal_apply_update(
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

pub(super) fn spawn_internal_run_installer(
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
pub(super) fn detect_server_port(args_file: &Path) -> u16 {
    detect_server_port_from_args_file(args_file)
        .or_else(|| {
            // Fall back to env var if restart args do not include explicit port.
            std::env::var("LLMLB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(DEFAULT_LISTEN_PORT)
}

pub(super) fn detect_server_port_from_args_file(args_file: &Path) -> Option<u16> {
    let content = fs::read_to_string(args_file).ok()?;
    let parsed: RestartArgsFile = serde_json::from_str(&content).ok()?;
    parse_port_from_args(&parsed.args)
}

pub(super) fn parse_port_from_args(args: &[String]) -> Option<u16> {
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
pub(super) fn record_auto_rollback_history(args_file: &Path, reason: &str) {
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

pub(super) fn spawn_internal_rollback(
    current_exe: &Path,
    backup: &Path,
    args_file: &Path,
) -> Result<()> {
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
