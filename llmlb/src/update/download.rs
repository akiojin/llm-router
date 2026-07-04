//! 更新アーティファクトのダウンロードと展開 IO
//!
//! arch-review [H2]: update/mod.rs の IO 層（HTTP ストリーミングダウンロード・
//! アーカイブ展開・展開後バイナリ探索）を submodule として切り出した。
//! UpdateManager からは pub(super) 経由で従来どおり利用する。

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use futures::StreamExt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn asset_name_from_url(url: &str) -> Option<String> {
    url.split('/').next_back().map(|s| s.to_string())
}

/// Progress callback for streaming downloads: `(downloaded_bytes, total_bytes)`.
pub(super) type ProgressCallback = Box<dyn Fn(u64, Option<u64>) + Send + Sync>;

pub(super) async fn download_to_path(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    on_progress: Option<ProgressCallback>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let res = client
        .get(url)
        .timeout(Duration::from_secs(300))
        .send()
        .await?;
    if !res.status().is_success() {
        return Err(anyhow!("download failed with status {}", res.status()));
    }
    let total_bytes = res.content_length();
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading download stream")?;
        io::Write::write_all(&mut file, &chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(ref cb) = on_progress {
            cb(downloaded, total_bytes);
        }
    }
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}

pub(super) fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    if name.ends_with(".tar.gz") {
        let file = fs::File::open(archive_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(dest_dir)?;
        return Ok(());
    }

    if name.ends_with(".zip") {
        let file = fs::File::open(archive_path)?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(dest_dir)?;
        return Ok(());
    }

    Err(anyhow!("unsupported archive format: {name}"))
}

pub(super) fn find_extracted_binary(
    extract_dir: &Path,
    binary_name: &str,
) -> Result<Option<PathBuf>> {
    // Expected layout: dist/llmlb-<artifact>/<binary>
    let mut candidates = Vec::<PathBuf>::new();
    for entry in fs::read_dir(extract_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            candidates.push(path.join(binary_name));
        } else if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
            candidates.push(path);
        }
    }

    for c in candidates {
        if c.exists() {
            return Ok(Some(c));
        }
    }

    // Fallback: deep search.
    let mut stack = vec![extract_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .unwrap_or_else(|_| fs::read_dir(extract_dir).unwrap())
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
                return Ok(Some(path));
            }
        }
    }

    Ok(None)
}
