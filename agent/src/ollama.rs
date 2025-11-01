//! Ollama管理モジュール
//!
//! Ollamaの自動ダウンロード、起動、停止、状態監視

use flate2::read::GzDecoder;
use ollama_coordinator_common::error::{AgentError, AgentResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration as StdDuration;
use tar::Archive;
use tokio::time::{sleep, Duration};
use zip::ZipArchive;

/// Ollamaマネージャー
pub struct OllamaManager {
    ollama_path: PathBuf,
    process: Option<Child>,
    port: u16,
}

const DEFAULT_MODEL: &str = "gpt-oss:20b";

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
            println!("Ollama not found. Downloading...");
            self.download().await?;
        }

        // Ollamaが起動しているか確認
        if !self.is_running().await {
            println!("Starting Ollama...");
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
        let model = default_model_name();
        self.ensure_model(&model).await
    }

    /// 指定モデルが存在しなければプルする
    async fn ensure_model(&self, model: &str) -> AgentResult<()> {
        let name = model.trim();
        if name.is_empty() {
            return Ok(());
        }

        if self.model_exists(name).await? {
            println!("Model {} already available", name);
            return Ok(());
        }

        println!("Model {} not found. Pulling...", name);
        self.pull_model(name).await?;

        if self.model_exists(name).await? {
            println!("Model {} downloaded", name);
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

    /// モデルをプル
    async fn pull_model(&self, model: &str) -> AgentResult<()> {
        let mut client_builder =
            reqwest::Client::builder().user_agent("ollama-coordinator-agent/0.1");

        if let Some(timeout) = pull_timeout() {
            client_builder = client_builder.timeout(timeout);
        }

        let client = client_builder
            .build()
            .map_err(|e| AgentError::Internal(format!("Failed to build HTTP client: {}", e)))?;

        let url = format!("{}/api/pull", self.api_base());
        let response = client
            .post(&url)
            .json(&json!({ "name": model, "stream": false }))
            .send()
            .await
            .map_err(|e| {
                AgentError::OllamaConnection(format!("Failed to pull model {}: {}", model, e))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            AgentError::Internal(format!("Failed to read pull response for {}: {}", model, e))
        })?;

        if !status.is_success() {
            return Err(AgentError::OllamaConnection(format!(
                "Failed to pull model {}: HTTP {} {}",
                model, status, body
            )));
        }

        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
                if let Some(error) = map
                    .get("error")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                {
                    return Err(AgentError::OllamaConnection(format!(
                        "Failed to pull model {}: {}",
                        model, error
                    )));
                }
            }
        }

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

        println!("Downloading Ollama from {}", download_url);

        // ダウンロードディレクトリを作成
        if let Some(parent) = self.ollama_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AgentError::Internal(format!("Failed to create directory: {}", e)))?;
        }

        // Ollamaをダウンロード
        let client = reqwest::Client::builder()
            .user_agent("ollama-coordinator-agent/0.1")
            .build()
            .map_err(|e| AgentError::Internal(format!("Failed to build HTTP client: {}", e)))?;

        let response = client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| AgentError::Internal(format!("Failed to download Ollama: {}", e)))?;

        if !response.status().is_success() {
            return Err(AgentError::Internal(format!(
                "Failed to download Ollama: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AgentError::Internal(format!("Failed to read Ollama download: {}", e)))?;

        save_ollama_binary(&bytes, &download_url, &self.ollama_path)?;

        println!("Ollama downloaded successfully to {:?}", self.ollama_path);
        Ok(())
    }

    /// Ollamaを起動
    fn start(&mut self) -> AgentResult<()> {
        let mut command = Command::new(&self.ollama_path);
        command
            .arg("serve")
            .env("OLLAMA_HOST", format!("0.0.0.0:{}", self.port));

        if std::env::var("OLLAMA_NO_GPU").is_err()
            && std::env::var("OLLAMA_GPU").is_err()
            && std::env::var("OLLAMA_USE_GPU").is_err()
        {
            command.env("OLLAMA_NO_GPU", "1");
        }

        let child = command
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| AgentError::OllamaConnection(format!("Failed to start Ollama: {}", e)))?;

        self.process = Some(child);
        println!("Ollama process started");
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
                println!("Ollama is ready");
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
            println!("Ollama process stopped");
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
    std::env::var("OLLAMA_DEFAULT_MODEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
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
