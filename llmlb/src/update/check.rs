//! GitHub Releases チェックとキャッシュ整合。
//!
//! 最新リリースを照会して UpdateState と update-check.json を更新し、
//! キャッシュからの復元やチェック失敗の記録を担う。ダウンロードは行わない。

use super::cache::{load_cache, save_cache, UpdateCacheFile};
use super::github::{fetch_latest_release, parse_tag_to_version};
use super::platform::{select_assets, Platform};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use super::tray::{notify_tray_available, notify_tray_failed, notify_tray_up_to_date};
use super::{PayloadState, UpdateManager, UpdateState};
use anyhow::{Context, Result};
use chrono::Utc;
use semver::Version;
use std::time::Duration;

impl UpdateManager {
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

    pub(super) async fn check_and_maybe_download(&self, force: bool) -> Result<()> {
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

    pub(super) async fn apply_cache(&self, cache: UpdateCacheFile) -> Result<()> {
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
}
