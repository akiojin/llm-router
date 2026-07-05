//! HuggingFace 動的情報（downloads/likes）の TTL 付きキャッシュとフェッチ
//!
//! arch-review [H6] round2: api/models.rs から HF info キャッシュを分離。

use super::HfInfo;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

// ===== HuggingFace Info Cache (SPEC-6cd7f960) =====

/// HF情報キャッシュエントリ
#[derive(Clone)]
struct HfInfoCacheEntry {
    fetched_at: Instant,
    info: HfInfo,
}

/// HF情報キャッシュ（TTL: 10分）
static HF_INFO_CACHE: Lazy<RwLock<HashMap<String, HfInfoCacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const HF_INFO_CACHE_TTL: Duration = Duration::from_secs(600); // 10分

/// HF情報キャッシュを明示的にクリアする（テスト分離・強制無効化フック）。
///
/// arch-review [L1]: モジュールグローバル static のためテスト間で状態が漏れ、
/// キャッシュ分岐の検証が困難だった。クリアフックを提供して分離可能にする。
#[cfg(test)]
pub(crate) fn invalidate_hf_info_cache() {
    if let Ok(mut cache) = HF_INFO_CACHE.write() {
        cache.clear();
    }
}

/// HuggingFace APIからモデル情報を取得（キャッシュ付き）
pub(crate) async fn fetch_hf_info(http_client: &reqwest::Client, repo: &str) -> Option<HfInfo> {
    // キャッシュチェック（ロックポイズニング時はスキップ）
    if let Ok(cache) = HF_INFO_CACHE.read() {
        if let Some(entry) = cache.get(repo) {
            if entry.fetched_at.elapsed() < HF_INFO_CACHE_TTL {
                return Some(entry.info.clone());
            }
        }
    }

    // HF APIからフェッチ
    let base_url = std::env::var("HF_BASE_URL")
        .unwrap_or_else(|_| "https://huggingface.co".to_string())
        .trim_end_matches('/')
        .to_string();
    let url = format!("{}/api/models/{}", base_url, repo);

    let mut req = http_client.get(&url);
    if let Ok(token) = std::env::var("HF_TOKEN") {
        req = req.bearer_auth(token);
    }

    let resp = match req.timeout(Duration::from_secs(5)).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(repo = %repo, error = %e, "Failed to fetch HF info");
            return None;
        }
    };

    if !resp.status().is_success() {
        tracing::debug!(repo = %repo, status = ?resp.status(), "HF API returned non-success status");
        return None;
    }

    #[derive(Deserialize)]
    struct HfModelInfo {
        downloads: Option<u64>,
        likes: Option<u64>,
    }

    let model_info: HfModelInfo = match resp.json().await {
        Ok(info) => info,
        Err(e) => {
            tracing::debug!(repo = %repo, error = %e, "Failed to parse HF info");
            return None;
        }
    };

    let info = HfInfo {
        downloads: model_info.downloads,
        likes: model_info.likes,
    };

    // キャッシュに保存（ロックポイズニング時はスキップ）
    if let Ok(mut cache) = HF_INFO_CACHE.write() {
        cache.insert(
            repo.to_string(),
            HfInfoCacheEntry {
                fetched_at: Instant::now(),
                info: info.clone(),
            },
        );
    }

    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn hf_info_cache_invalidation_clears_entries() {
        // arch-review [L1]: クリアフックがキャッシュを確実に空にすることを検証。
        HF_INFO_CACHE.write().unwrap().insert(
            "owner/repo".to_string(),
            HfInfoCacheEntry {
                fetched_at: Instant::now(),
                info: HfInfo::default(),
            },
        );
        assert!(HF_INFO_CACHE.read().unwrap().contains_key("owner/repo"));
        invalidate_hf_info_cache();
        assert!(HF_INFO_CACHE.read().unwrap().is_empty());
    }
}
