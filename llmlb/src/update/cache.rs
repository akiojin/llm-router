//! 更新チェック結果のローカルキャッシュ。
//!
//! 最新バージョン情報を `update-check.json` として永続化し、
//! 起動時のオンラインチェックを一定間隔に抑制するために使う。

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 更新チェックのキャッシュファイル表現。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct UpdateCacheFile {
    pub(super) last_checked_at: DateTime<Utc>,
    pub(super) latest_version: Option<String>,
    pub(super) release_url: Option<String>,
    pub(super) portable_asset_url: Option<String>,
    pub(super) installer_asset_url: Option<String>,
}

/// キャッシュファイルを読み込む。存在しなければ `None`。
pub(super) fn load_cache(path: &Path) -> Result<Option<UpdateCacheFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)?;
    let cache: UpdateCacheFile = serde_json::from_str(&content)?;
    Ok(Some(cache))
}

/// キャッシュファイルをアトミックに書き込む（tmp へ書いて rename）。
pub(super) fn save_cache(path: &Path, cache: UpdateCacheFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&cache)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // load_cache / save_cache roundtrip
    // =======================================================================
    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");

        // No cache initially
        assert!(load_cache(&cache_path).unwrap().is_none());

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("5.0.0".to_string()),
            release_url: Some("https://example.com/release".to_string()),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
        };
        save_cache(&cache_path, cache.clone()).unwrap();

        let loaded = load_cache(&cache_path).unwrap().unwrap();
        assert_eq!(loaded.latest_version, cache.latest_version);
        assert_eq!(loaded.release_url, cache.release_url);
        assert_eq!(loaded.portable_asset_url, cache.portable_asset_url);
        assert_eq!(loaded.installer_asset_url, cache.installer_asset_url);
    }

    #[test]
    fn load_cache_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.json");
        assert!(load_cache(&cache_path).unwrap().is_none());
    }

    #[test]
    fn save_cache_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("subdir").join("nested").join("cache.json");

        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: None,
            release_url: None,
            portable_asset_url: None,
            installer_asset_url: None,
        };
        save_cache(&cache_path, cache).unwrap();
        assert!(cache_path.exists());
    }

    // =======================================================================
    // UpdateCacheFile serialization
    // =======================================================================
    #[test]
    fn update_cache_file_serialization() {
        let cache = UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("5.0.0".to_string()),
            release_url: Some("https://example.com/release".to_string()),
            portable_asset_url: None,
            installer_asset_url: None,
        };
        let json = serde_json::to_string(&cache).unwrap();
        let deserialized: UpdateCacheFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.latest_version, cache.latest_version);
        assert_eq!(deserialized.release_url, cache.release_url);
    }
}
