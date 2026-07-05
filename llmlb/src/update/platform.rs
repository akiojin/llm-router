//! プラットフォーム/ターゲット検出とリリースアセット・適用プラン選択
//!
//! arch-review [H6]: update/mod.rs から、どのポータブル/インストーラアセットを
//! 取得しどう適用するかの選択ロジックを分離。親は use platform::{...} で参照する。

use super::github::{GitHubAsset, GitHubRelease};
use super::InstallerKind;
use anyhow::Result;
use std::{fs, io, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Platform {
    pub(crate) os: String,
    pub(crate) arch: String,
}

impl Platform {
    pub(crate) fn detect() -> Result<Self> {
        Ok(Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        })
    }

    pub(crate) fn artifact(&self) -> Option<&'static str> {
        match (self.os.as_str(), self.arch.as_str()) {
            ("linux", "x86_64") => Some("linux-x86_64"),
            ("linux", "aarch64") => Some("linux-arm64"),
            ("macos", "x86_64") => Some("macos-x86_64"),
            ("macos", "aarch64") => Some("macos-arm64"),
            ("windows", "x86_64") => Some("windows-x86_64"),
            _ => None,
        }
    }

    pub(crate) fn binary_name(&self) -> String {
        if self.os == "windows" {
            "llmlb.exe".to_string()
        } else {
            "llmlb".to_string()
        }
    }

    pub(crate) fn portable_asset_name(&self) -> Option<String> {
        self.artifact().map(|a| {
            if self.os == "windows" {
                format!("llmlb-{a}.zip")
            } else {
                format!("llmlb-{a}.tar.gz")
            }
        })
    }

    pub(crate) fn installer_asset_name(&self) -> Option<(String, InstallerKind)> {
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

pub(crate) fn select_assets(
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
pub(crate) enum ApplyPlan {
    Portable { url: String },
    Installer { url: String, kind: InstallerKind },
}

pub(crate) fn choose_apply_plan(
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

pub(crate) fn is_dir_writable(dir: &Path) -> Result<bool> {
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
