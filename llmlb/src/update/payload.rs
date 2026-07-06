//! 更新ペイロードの準備フェーズ。
//!
//! ダウンロード済みアーティファクトの検証・展開・適用プラン選定を行い、
//! 適用可能な PayloadKind を確定する。準備失敗は状態へ記録する。

use super::download::{
    asset_name_from_url, download_to_path, extract_archive, find_extracted_binary, ProgressCallback,
};
use super::platform::{choose_apply_plan, is_dir_writable, ApplyPlan, Platform};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use super::tray::notify_tray_ready;
use super::{PayloadKind, PayloadState, UpdateManager, UpdateState};
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

impl UpdateManager {
    pub(super) async fn ensure_payload_ready(&self) -> Result<PayloadKind> {
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

    pub(super) async fn set_payload_error(&self, msg: String) {
        let mut st = self.inner.state.write().await;
        if let UpdateState::Available { payload, .. } = &mut *st {
            *payload = PayloadState::Error { message: msg };
        }
    }
}
