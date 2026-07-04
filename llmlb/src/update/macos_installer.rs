//! macOS 向けインストーラ実行（osascript 経由の権限昇格）
//!
//! arch-review [H2]: update の OS プロセス層のうち、macOS の pkg インストーラを
//! 管理者権限で起動する処理と AppleScript/シェルのエスケープを submodule へ分離。
//! モジュール全体が macOS 限定のため `mod macos_installer;` ごと cfg でゲートする。

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;

pub(super) fn run_macos_pkg_installer_with_privileges(installer: &Path) -> Result<()> {
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

fn sh_single_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn escape_applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
