//! Ollama管理モジュール
//!
//! Ollamaの自動ダウンロード、起動、停止、状態監視

use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use ollama_coordinator_common::error::{AgentError, AgentResult};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration as StdDuration;
use sysinfo::System;
use tar::Archive;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use zip::ZipArchive;

/// Ollamaマネージャー
pub struct OllamaManager {
    ollama_path: PathBuf,
    process: Option<Child>,
    port: u16,
}

/// ダウンロード進捗情報
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// 現在ダウンロードしたバイト数
    pub current: u64,
    /// 合計バイト数
    pub total: u64,
}

impl DownloadProgress {
    /// パーセンテージを計算
    pub fn percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64) * 100.0
        }
    }
}

const DEFAULT_MODEL: &str = "gpt-oss:20b";
const DEFAULT_MODEL_CANDIDATES: &[&str] =
    &["gpt-oss:20b", "gpt-oss:7b", "gpt-oss:3b", "gpt-oss:1b"];
const MODEL_MEMORY_REQUIREMENTS: &[(&str, f64)] = &[
    ("gpt-oss:20b", 12.0),
    ("gpt-oss:7b", 6.0),
    ("gpt-oss:3b", 3.0),
    ("gpt-oss:1b", 1.0),
];

impl OllamaManager {
    /// 新しいOllamaマネージャーを作成
    pub fn new(port: u16) -> Self {
        let ollama_dir = get_ollama_directory();
        let ollama_path = ollama_dir.join(if cfg!(windows) {
            "ollama.exe"
        } else {
            "ollama"
        });

        Self {
            ollama_path,
            process: None,
            port,
        }
    }

    /// ollamaバイナリのパスを取得
    pub fn ollama_path(&self) -> &std::path::Path {
        &self.ollama_path
    }

    /// Ollamaが利用可能か確認し、必要に応じてダウンロード・起動
    pub async fn ensure_running(&mut self) -> AgentResult<()> {
        // Ollamaがインストールされているか確認
        if !self.is_installed() {
            info!("Ollama not found. Downloading...");
            self.download().await?;
        }

        // Ollamaが起動しているか確認
        if !self.is_running().await {
            info!("Starting Ollama...");
            self.start()?;

            // 起動を待つ
            self.wait_for_startup().await?;
        }

        // デフォルトモデルを確保
        self.ensure_default_model().await?;

        Ok(())
    }

    /// デフォルトモデル（環境変数で上書き可能）を確保
    async fn ensure_default_model(&self) -> AgentResult<()> {
        if let Some(override_model) = default_model_override() {
            info!("Using explicit default model override: {}", override_model);
            return self.ensure_model(&override_model).await;
        }

        let (selected_model, total_memory_gib) = select_default_model_for_memory();
        if selected_model != DEFAULT_MODEL {
            warn!(
                "System memory {:.1} GiB is below the recommended threshold for {}. Falling back to {}.",
                total_memory_gib,
                DEFAULT_MODEL,
                selected_model
            );
        }

        self.ensure_model(selected_model).await
    }

    /// 指定モデルが存在しなければプルする
    pub async fn ensure_model(&self, model: &str) -> AgentResult<()> {
        let name = model.trim();
        if name.is_empty() {
            return Ok(());
        }

        if self.model_exists(name).await? {
            info!("Model {} already available", name);
            return Ok(());
        }

        info!("Model {} not found. Pulling...", name);
        self.pull_model(name).await?;

        if self.model_exists(name).await? {
            info!("Model {} downloaded", name);
            Ok(())
        } else {
            Err(AgentError::OllamaConnection(format!(
                "Model {} did not become available after pull",
                name
            )))
        }
    }

    /// モデル存在チェック
    async fn model_exists(&self, model: &str) -> AgentResult<bool> {
        let name = model.trim();
        if name.is_empty() {
            return Ok(true);
        }

        let models = self.fetch_models(false).await?;
        Ok(models.iter().any(|entry| tag_matches_model(entry, name)))
    }

    /// モデル一覧を取得
    pub async fn list_models(&self) -> AgentResult<Vec<String>> {
        self.fetch_models(true).await
    }

    async fn fetch_models(&self, fallback: bool) -> AgentResult<Vec<String>> {
        let client = reqwest::Client::builder()
            .user_agent("ollama-coordinator-agent/0.1")
            .timeout(StdDuration::from_secs(10))
            .build()
            .map_err(|e| AgentError::Internal(format!("Failed to build HTTP client: {}", e)))?;

        let url = format!("{}/api/tags", self.api_base());
        let response = client.get(&url).send().await.map_err(|e| {
            AgentError::OllamaConnection(format!("Failed to query Ollama tags: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(AgentError::OllamaConnection(format!(
                "Failed to query Ollama tags: HTTP {}",
                response.status()
            )));
        }

        let payload = response
            .json::<TagsResponse>()
            .await
            .map_err(|e| AgentError::Internal(format!("Failed to parse tags response: {}", e)))?;

        let mut names = Vec::with_capacity(payload.models.len());
        for entry in payload.models {
            if let Some(candidate) = normalize_model_name(&entry.name) {
                if !names.iter().any(|existing| existing == &candidate) {
                    names.push(candidate);
                }
            }
        }

        if fallback && names.is_empty() {
            if let Some(default_model) = normalize_model_name(&default_model_name()) {
                names.push(default_model);
            }
        }

        Ok(names)
    }

    /// モデルをプル（リトライ付き・進捗表示）
    async fn pull_model(&self, model: &str) -> AgentResult<()> {
        use futures::StreamExt;
        use indicatif::{ProgressBar, ProgressStyle};

        let mut client_builder =
            reqwest::Client::builder().user_agent("ollama-coordinator-agent/0.1");

        if let Some(timeout) = pull_timeout() {
            client_builder = client_builder.timeout(timeout);
        }

        let client = client_builder
            .build()
            .map_err(|e| AgentError::Internal(format!("Failed to build HTTP client: {}", e)))?;

        let url = format!("{}/api/pull", self.api_base());

        // リトライ設定を取得
        let (max_retries, max_backoff_secs) = get_retry_config();

        // リトライ付きでモデルプルを実行（ストリーミング有効）
        let response = retry_http_request(
            || {
                let client = client.clone();
                let url = url.clone();
                let model = model.to_string();
                async move {
                    client
                        .post(&url)
                        .json(&json!({ "name": model, "stream": true }))
                        .send()
                        .await
                }
            },
            max_retries,
            max_backoff_secs,
        )
        .await
        .map_err(|e| {
            AgentError::OllamaConnection(format!(
                "Failed to pull model {} after retries: {}",
                model, e
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::OllamaConnection(format!(
                "Failed to pull model {}: HTTP {} {}",
                model, status, body
            )));
        }

        // プログレスバーを作成（サイズ不明の場合は spinner として使用）
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message(format!("Pulling model {}", model));

        // ストリーミングレスポンスを1行ずつ処理
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                AgentError::Internal(format!("Failed to read pull stream for {}: {}", model, e))
            })?;

            buffer.extend_from_slice(&chunk);

            // 改行で区切られたJSONを処理
            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes = buffer.drain(..=newline_pos).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line_bytes);
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
                    // エラーチェック
                    if let Some(error) = map
                        .get("error")
                        .and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty())
                    {
                        pb.finish_and_clear();
                        return Err(AgentError::OllamaConnection(format!(
                            "Failed to pull model {}: {}",
                            model, error
                        )));
                    }

                    // ステータス表示
                    if let Some(status_msg) = map.get("status").and_then(|v| v.as_str()) {
                        // ダウンロード進捗がある場合
                        if let (Some(total), Some(completed)) = (
                            map.get("total").and_then(|v| v.as_u64()),
                            map.get("completed").and_then(|v| v.as_u64()),
                        ) {
                            if total > 0 {
                                // プログレスバーを進捗モードに切り替え
                                if pb.length().is_none() || pb.length() == Some(0) {
                                    pb.set_length(total);
                                    pb.set_style(
                                        ProgressStyle::default_bar()
                                            .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%)")
                                            .unwrap()
                                            .progress_chars("#>-"),
                                    );
                                }
                                pb.set_position(completed);
                                pb.set_message(format!("Pulling model {}: {}", model, status_msg));
                            } else {
                                pb.set_message(format!("Pulling model {}: {}", model, status_msg));
                            }
                        } else {
                            pb.set_message(format!("Pulling model {}: {}", model, status_msg));
                        }
                    }
                }
            }
        }

        pb.finish_with_message(format!("Model {} pulled successfully", model));
        Ok(())
    }

    fn api_base(&self) -> String {
        if let Ok(raw) = std::env::var("OLLAMA_API_BASE") {
            let trimmed = raw.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                return format!("http://127.0.0.1:{}", self.port);
            }

            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                trimmed
            } else {
                format!("http://{}", trimmed)
            }
        } else {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    /// Ollamaがインストールされているか確認
    pub fn is_installed(&self) -> bool {
        self.ollama_path.exists()
    }

    /// Ollamaをダウンロード
    async fn download(&self) -> AgentResult<()> {
        let download_url = get_ollama_download_url();

        info!("Downloading Ollama from {}", download_url);

        // ダウンロードディレクトリを作成
        if let Some(parent) = self.ollama_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AgentError::Internal(format!("Failed to create directory: {}", e)))?;
        }

        // Ollamaをダウンロード（リトライ付き、プロキシ対応）
        let client = build_http_client_with_proxy()?;

        let (max_retries, max_backoff_secs) = get_retry_config();

        // リトライ付きでHTTPリクエスト実行
        let response = retry_http_request(
            || {
                let client = client.clone();
                let url = download_url.clone();
                async move { client.get(&url).send().await }
            },
            max_retries,
            max_backoff_secs,
        )
        .await?;

        if !response.status().is_success() {
            return Err(AgentError::Internal(format!(
                "Failed to download Ollama: HTTP {}",
                response.status()
            )));
        }

        // プログレスバーを作成
        let total_size = response.content_length().unwrap_or(0);
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Downloading Ollama");

        // チャンク単位でダウンロード
        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| AgentError::Internal(format!("Failed to read chunk: {}", e)))?;
            buffer.extend_from_slice(&chunk);
            pb.inc(chunk.len() as u64);
        }

        pb.finish_with_message("Download complete");

        // チェックサム検証（環境変数で有効化）
        if std::env::var("OLLAMA_VERIFY_CHECKSUM").is_ok() {
            info!("Verifying checksum...");
            let expected_checksum = fetch_checksum_from_url(&client, &download_url).await?;
            verify_checksum(&buffer, &expected_checksum)?;
            info!("Checksum verification successful");
        }

        save_ollama_binary(&buffer, &download_url, &self.ollama_path)?;

        info!("Ollama downloaded successfully to {:?}", self.ollama_path);
        Ok(())
    }

    /// Ollamaを起動
    fn start(&mut self) -> AgentResult<()> {
        let mut command = Command::new(&self.ollama_path);
        command
            .arg("serve")
            .env("OLLAMA_HOST", format!("0.0.0.0:{}", self.port));

        let child = command
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| AgentError::OllamaConnection(format!("Failed to start Ollama: {}", e)))?;

        self.process = Some(child);
        info!("Ollama process started");
        Ok(())
    }

    /// Ollamaが起動しているか確認
    pub async fn is_running(&self) -> bool {
        let client = match reqwest::Client::builder()
            .timeout(StdDuration::from_secs(3))
            .build()
        {
            Ok(client) => client,
            Err(_) => return false,
        };

        let url = format!("{}/api/tags", self.api_base());
        match client.get(url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Ollama起動を待つ
    async fn wait_for_startup(&mut self) -> AgentResult<()> {
        let timeout_secs = std::env::var("OLLAMA_STARTUP_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120);
        let max_attempts = timeout_secs;

        for _ in 0..max_attempts {
            if let Some(process) = self.process.as_mut() {
                if let Ok(Some(status)) = process.try_wait() {
                    return Err(AgentError::OllamaConnection(format!(
                        "Ollama exited prematurely with status {}",
                        status
                    )));
                }
            }
            if self.is_running().await {
                info!("Ollama is ready");
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
        }

        Err(AgentError::OllamaConnection(format!(
            "Ollama failed to start within {} seconds",
            timeout_secs
        )))
    }

    /// Ollamaのバージョンを取得
    pub async fn get_version(&self) -> AgentResult<String> {
        let output = Command::new(&self.ollama_path)
            .arg("--version")
            .output()
            .map_err(|e| {
                AgentError::OllamaConnection(format!("Failed to get Ollama version: {}", e))
            })?;

        if !output.status.success() {
            return Err(AgentError::OllamaConnection(
                "Failed to get Ollama version".to_string(),
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(version)
    }

    /// Ollamaを停止
    pub fn stop(&mut self) -> AgentResult<()> {
        if let Some(mut process) = self.process.take() {
            process.kill().map_err(|e| {
                AgentError::Internal(format!("Failed to kill Ollama process: {}", e))
            })?;
            info!("Ollama process stopped");
        }
        Ok(())
    }
}

impl Drop for OllamaManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<ModelSummary>,
}

#[derive(Debug, Deserialize)]
struct ModelSummary {
    name: String,
}

fn default_model_name() -> String {
    default_model_override().unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn default_model_override() -> Option<String> {
    std::env::var("OLLAMA_DEFAULT_MODEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn select_default_model_for_memory() -> (&'static str, f64) {
    let mut system = System::new();
    system.refresh_memory();

    let total_kib = system.total_memory() as f64;
    let total_gib = if total_kib <= 0.0 {
        0.0
    } else {
        total_kib / 1024.0 / 1024.0
    };

    (pick_model_for_memory(total_gib), total_gib)
}

fn pick_model_for_memory(total_gib: f64) -> &'static str {
    for (model, required_gib) in MODEL_MEMORY_REQUIREMENTS {
        if total_gib >= *required_gib {
            return model;
        }
    }

    DEFAULT_MODEL_CANDIDATES
        .last()
        .copied()
        .unwrap_or(DEFAULT_MODEL)
}

fn pull_timeout() -> Option<StdDuration> {
    match std::env::var("OLLAMA_PULL_TIMEOUT_SECS") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return None;
            }
            match trimmed.parse::<u64>() {
                Ok(0) => None,
                Ok(secs) => Some(StdDuration::from_secs(secs)),
                Err(_) => None,
            }
        }
        Err(_) => None,
    }
}

fn tag_matches_model(tag: &str, model: &str) -> bool {
    if tag == model {
        return true;
    }
    tag.split(':').next() == Some(model)
}

fn normalize_model_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn save_ollama_binary(bytes: &[u8], download_url: &str, destination: &Path) -> AgentResult<()> {
    if download_url.ends_with(".tgz") || download_url.ends_with(".tar.gz") {
        extract_tar_gz(bytes, destination)
    } else if download_url.ends_with(".zip") {
        extract_zip(bytes, destination)
    } else {
        write_binary(bytes, destination)
    }
}

fn extract_tar_gz(bytes: &[u8], destination: &Path) -> AgentResult<()> {
    let cursor = Cursor::new(bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e| AgentError::Internal(format!("Failed to read archive: {}", e)))?
    {
        let mut entry =
            entry.map_err(|e| AgentError::Internal(format!("Failed to read entry: {}", e)))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry
            .path()
            .map_err(|e| AgentError::Internal(format!("Failed to read entry path: {}", e)))?;
        if path
            .file_name()
            .map(|name| name == "ollama")
            .unwrap_or(false)
        {
            let mut file = File::create(destination)
                .map_err(|e| AgentError::Internal(format!("Failed to create file: {}", e)))?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|e| AgentError::Internal(format!("Failed to extract file: {}", e)))?;
            set_unix_executable(destination)?;
            return Ok(());
        }
    }

    Err(AgentError::Internal(
        "Failed to locate ollama binary in archive".to_string(),
    ))
}

fn extract_zip(bytes: &[u8], destination: &Path) -> AgentResult<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AgentError::Internal(format!("Failed to read zip archive: {}", e)))?;

    let expected = if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    };

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AgentError::Internal(format!("Failed to access zip entry: {}", e)))?;
        if !file.is_file() {
            continue;
        }

        if file.name().ends_with(expected) {
            let mut output = File::create(destination)
                .map_err(|e| AgentError::Internal(format!("Failed to create file: {}", e)))?;
            std::io::copy(&mut file, &mut output)
                .map_err(|e| AgentError::Internal(format!("Failed to extract file: {}", e)))?;
            set_unix_executable(destination)?;
            return Ok(());
        }
    }

    Err(AgentError::Internal(
        "Failed to locate ollama binary in archive".to_string(),
    ))
}

fn write_binary(bytes: &[u8], destination: &Path) -> AgentResult<()> {
    std::fs::write(destination, bytes)
        .map_err(|e| AgentError::Internal(format!("Failed to write Ollama binary: {}", e)))?;
    set_unix_executable(destination)?;
    Ok(())
}

fn set_unix_executable(_path: &Path) -> AgentResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(_path)
            .map_err(|e| AgentError::Internal(format!("Failed to get file metadata: {}", e)))?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(_path, perms).map_err(|e| {
            AgentError::Internal(format!("Failed to set execute permission: {}", e))
        })?;
    }
    Ok(())
}

/// Ollamaディレクトリを取得
fn get_ollama_directory() -> PathBuf {
    if cfg!(windows) {
        // Windows: %LOCALAPPDATA%\OllamaCoordinator\ollama
        let local_app_data = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| String::from("C:\\Users\\Default\\AppData\\Local"));
        PathBuf::from(local_app_data)
            .join("OllamaCoordinator")
            .join("ollama")
    } else if cfg!(target_os = "macos") {
        // macOS: ~/Library/Application Support/OllamaCoordinator/ollama
        let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("OllamaCoordinator")
            .join("ollama")
    } else {
        // Linux: ~/.local/share/ollama-coordinator/ollama
        let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("ollama-coordinator")
            .join("ollama")
    }
}

/// OllamaダウンロードURLを取得
fn get_ollama_download_url() -> String {
    if let Ok(url) = std::env::var("OLLAMA_DOWNLOAD_URL") {
        return url;
    }

    let arch = detect_arch();

    if cfg!(target_os = "windows") {
        match arch.as_str() {
            "x86_64" | "amd64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-windows-amd64.zip"
                    .to_string()
            }
            "aarch64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-windows-arm64.zip"
                    .to_string()
            }
            _ => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-windows-amd64.zip"
                    .to_string()
            }
        }
    } else if cfg!(target_os = "macos") {
        match arch.as_str() {
            "aarch64" | "arm64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-darwin.tgz"
                    .to_string()
            }
            "x86_64" | "amd64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-darwin.tgz"
                    .to_string()
            }
            _ => "https://github.com/ollama/ollama/releases/latest/download/ollama-darwin.tgz"
                .to_string(),
        }
    } else {
        match arch.as_str() {
            "x86_64" | "amd64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-linux-amd64.tgz"
                    .to_string()
            }
            "aarch64" | "arm64" => {
                "https://github.com/ollama/ollama/releases/latest/download/ollama-linux-arm64.tgz"
                    .to_string()
            }
            _ => "https://github.com/ollama/ollama/releases/latest/download/ollama-linux-amd64.tgz"
                .to_string(),
        }
    }
}

fn detect_arch() -> String {
    if let Ok(platform) = std::env::var("OLLAMA_PLATFORM") {
        return platform;
    }

    if let Ok(output) = Command::new("uname").arg("-m").output() {
        if output.status.success() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                let arch = text.trim();
                if !arch.is_empty() {
                    return arch.to_string();
                }
            }
        }
    }

    if let Ok(hosttype) = std::env::var("HOSTTYPE") {
        if !hosttype.is_empty() {
            return hosttype;
        }
    }

    std::env::consts::ARCH.to_string()
}

/// HTTPリクエストをリトライ付きで実行
///
/// ネットワークエラー時に指数バックオフでリトライする
async fn retry_http_request<F, Fut, T>(
    operation: F,
    max_retries: u32,
    max_backoff_secs: u64,
) -> AgentResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, reqwest::Error>>,
{
    let mut attempt = 0;
    let mut backoff_secs = 1;

    loop {
        attempt += 1;

        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                // リトライすべきエラーかチェック
                let should_retry = e.is_timeout() || e.is_connect() || is_5xx_error(&e);

                if !should_retry || attempt >= max_retries {
                    return Err(AgentError::Internal(format!(
                        "HTTP request failed after {} attempts: {}",
                        attempt, e
                    )));
                }

                warn!(
                    "HTTP request failed (attempt {}/{}): {}. Retrying in {} seconds...",
                    attempt, max_retries, e, backoff_secs
                );

                // 指数バックオフで待機
                sleep(Duration::from_secs(backoff_secs)).await;

                // 次回のバックオフ時間を計算（指数的に増加、最大値まで）
                backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
            }
        }
    }
}

/// HTTP 5xxエラーかチェック
fn is_5xx_error(error: &reqwest::Error) -> bool {
    if let Some(status) = error.status() {
        status.is_server_error()
    } else {
        false
    }
}

/// 環境変数からリトライ設定を取得
fn get_retry_config() -> (u32, u64) {
    let max_retries = std::env::var("OLLAMA_DOWNLOAD_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let max_backoff_secs = std::env::var("OLLAMA_DOWNLOAD_MAX_BACKOFF_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    (max_retries, max_backoff_secs)
}

/// プロキシ設定付きHTTPクライアントを構築
fn build_http_client_with_proxy() -> AgentResult<reqwest::Client> {
    let client_builder = reqwest::Client::builder()
        .user_agent("ollama-coordinator-agent/0.1")
        .timeout(StdDuration::from_secs(300)); // 5分タイムアウト

    // 環境変数からプロキシ設定を取得（reqwestは自動的にHTTP_PROXY, HTTPS_PROXYを読み込む）
    // ただし、明示的にNO_PROXYを処理する場合は手動設定が必要

    // reqwestはデフォルトでシステムプロキシ設定を使用するため、
    // 特別な設定は不要（HTTP_PROXY, HTTPS_PROXY, NO_PROXYを自動認識）

    client_builder
        .build()
        .map_err(|e| AgentError::Internal(format!("Failed to build HTTP client: {}", e)))
}

/// SHA256チェックサムを検証
///
/// バイナリデータのSHA256ハッシュを計算し、期待されるチェックサムと比較する
fn verify_checksum(data: &[u8], expected_checksum: &str) -> AgentResult<()> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual_checksum = format!("{:x}", hasher.finalize());

    if actual_checksum.to_lowercase() != expected_checksum.to_lowercase() {
        return Err(AgentError::Internal(format!(
            "Checksum mismatch: expected {}, got {}",
            expected_checksum, actual_checksum
        )));
    }

    Ok(())
}

/// GitHubからチェックサムファイルを取得
///
/// ダウンロードURLに基づいてチェックサムファイル（.sha256）をダウンロードする
async fn fetch_checksum_from_url(
    client: &reqwest::Client,
    download_url: &str,
) -> AgentResult<String> {
    // チェックサムURLを生成（.sha256拡張子を追加）
    let checksum_url = format!("{}.sha256", download_url);

    info!("Fetching checksum from {}", checksum_url);

    let (max_retries, max_backoff_secs) = get_retry_config();

    // リトライ付きでチェックサムをダウンロード
    let response = retry_http_request(
        || {
            let client = client.clone();
            let url = checksum_url.clone();
            async move { client.get(&url).send().await }
        },
        max_retries,
        max_backoff_secs,
    )
    .await?;

    if !response.status().is_success() {
        return Err(AgentError::Internal(format!(
            "Failed to fetch checksum: HTTP {}",
            response.status()
        )));
    }

    let checksum = response
        .text()
        .await
        .map_err(|e| AgentError::Internal(format!("Failed to read checksum: {}", e)))?
        .trim()
        .to_string();

    Ok(checksum)
}

/// ollama psコマンドの実行結果
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[derive(Clone, Default)]
    struct TagSequenceResponder {
        hits: Arc<AtomicUsize>,
    }

    impl Respond for TagSequenceResponder {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let index = self.hits.fetch_add(1, Ordering::SeqCst);
            if index == 0 {
                ResponseTemplate::new(200).set_body_raw(r#"{"models":[]}"#, "application/json")
            } else {
                ResponseTemplate::new(200).set_body_raw(
                    r#"{"models":[{"name":"test-model"},{"name":"other:latest"}]}"#,
                    "application/json",
                )
            }
        }
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &'static str, value: Option<String>) -> Self {
            let original = std::env::var(key).ok();
            if let Some(ref val) = value {
                std::env::set_var(key, val);
            } else {
                std::env::remove_var(key);
            }

            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref val) = self.original {
                std::env::set_var(self.key, val);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn test_get_ollama_directory() {
        let dir = get_ollama_directory();
        assert!(
            dir.to_string_lossy().contains("OllamaCoordinator")
                || dir.to_string_lossy().contains("ollama-coordinator")
        );
    }

    #[test]
    fn test_get_ollama_download_url() {
        let url = get_ollama_download_url();
        assert!(url.contains("ollama"));
    }

    #[tokio::test]
    async fn test_ollama_manager_creation() {
        let manager = OllamaManager::new(11434);
        assert_eq!(manager.port, 11434);
        assert!(manager.ollama_path.to_string_lossy().contains("ollama"));
    }

    #[test]
    fn test_default_model_name_overrides() {
        let _lock = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();

        {
            let _guard = EnvGuard::new(
                "OLLAMA_DEFAULT_MODEL",
                Some("custom-model:latest".to_string()),
            );
            assert_eq!(default_model_name(), "custom-model:latest");
        }

        {
            let _guard = EnvGuard::new("OLLAMA_DEFAULT_MODEL", None);
            assert_eq!(default_model_name(), DEFAULT_MODEL);
        }
    }

    #[test]
    fn test_pick_model_for_memory_thresholds() {
        assert_eq!(pick_model_for_memory(16.0), "gpt-oss:20b");
        assert_eq!(pick_model_for_memory(8.0), "gpt-oss:7b");
        assert_eq!(pick_model_for_memory(4.5), "gpt-oss:3b");
        assert_eq!(pick_model_for_memory(0.5), "gpt-oss:1b");
    }

    #[test]
    fn test_api_base_override_without_scheme() {
        let _lock = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();

        let manager = OllamaManager::new(11434);
        {
            let _guard = EnvGuard::new("OLLAMA_API_BASE", Some("127.0.0.1:9999".to_string()));
            assert_eq!(manager.api_base(), "http://127.0.0.1:9999");
        }
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_ensure_model_pulls_when_missing() {
        let server = MockServer::start().await;

        let tags_responder = TagSequenceResponder::default();
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(tags_responder)
            .expect(2)
            .mount(&server)
            .await;

        let pull_response =
            ResponseTemplate::new(200).set_body_raw(r#"{"status":"success"}"#, "application/json");
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .respond_with(pull_response)
            .expect(1)
            .mount(&server)
            .await;

        let _lock = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _api_guard = EnvGuard::new("OLLAMA_API_BASE", Some(server.uri()));
        let _model_guard = EnvGuard::new("OLLAMA_DEFAULT_MODEL", Some("test-model".to_string()));

        let manager = OllamaManager::new(server.address().port());
        manager.ensure_default_model().await.unwrap();
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn test_list_models_returns_unique_trimmed() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    r#"{"models":[{"name":" gpt-oss:20b "},{"name":"gpt-oss:latest"},{"name":""},{"name":"phi-3"}]}"#,
                    "application/json",
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let _lock = ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _api_guard = EnvGuard::new("OLLAMA_API_BASE", Some(server.uri()));
        let manager = OllamaManager::new(server.address().port());

        let models = manager.list_models().await.unwrap();
        assert_eq!(models, vec!["gpt-oss:20b", "gpt-oss:latest", "phi-3"]);
    }
}
