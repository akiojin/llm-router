//! GitHub Releases API からの最新リリース取得とタグ→バージョン変換
//!
//! arch-review [H2]: update/mod.rs が純ドメイン・IO・OS プロセス・UI を 1 ファイルに
//! 混載していたため、GitHub Releases API の呼び出しとレスポンス型を submodule へ
//! 切り出した。UpdateManager 本体からは `pub(super)` 経由で従来どおり呼び出す。

use anyhow::{anyhow, Context, Result};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
struct GitHubReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct GitHubAsset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

#[derive(Debug, Clone)]
pub(super) struct GitHubRelease {
    pub(super) tag_name: String,
    pub(super) html_url: String,
    pub(super) assets: Vec<GitHubAsset>,
}

pub(super) async fn fetch_latest_release(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    timeout: Duration,
    api_base_url: Option<&str>,
) -> Result<GitHubRelease> {
    let base = api_base_url.unwrap_or("https://api.github.com");
    let url = format!("{base}/repos/{owner}/{repo}/releases/latest");
    let user_agent = format!("llmlb/{}", env!("CARGO_PKG_VERSION"));
    let res = client
        .get(url)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", user_agent)
        .timeout(timeout)
        .send()
        .await
        .context("Failed to call GitHub Releases API")?;
    if !res.status().is_success() {
        return Err(anyhow!("GitHub API returned {}", res.status().as_u16()));
    }
    let parsed: GitHubReleaseResponse = res
        .json()
        .await
        .context("Failed to parse GitHub release JSON")?;
    Ok(GitHubRelease {
        tag_name: parsed.tag_name,
        html_url: parsed.html_url,
        assets: parsed.assets,
    })
}

pub(super) fn parse_tag_to_version(tag: &str) -> Result<Version> {
    let normalized = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(normalized).map_err(|e| anyhow!("Invalid tag semver: {e}"))
}
