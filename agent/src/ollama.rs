//! Ollama管理モジュール
//!
//! Ollamaの自動ダウンロード、起動、停止、状態監視

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tokio::time::{sleep, Duration};
use ollama_coordinator_common::error::{AgentError, AgentResult};

/// Ollamaマネージャー
pub struct OllamaManager {
    ollama_path: PathBuf,
    process: Option<Child>,
    port: u16,
}

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

        Ok(())
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
        let response = reqwest::get(&download_url)
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

        // ファイルに保存
        std::fs::write(&self.ollama_path, bytes)
            .map_err(|e| AgentError::Internal(format!("Failed to write Ollama binary: {}", e)))?;

        // Unix系OSでは実行権限を付与
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.ollama_path)
                .map_err(|e| AgentError::Internal(format!("Failed to get file metadata: {}", e)))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&self.ollama_path, perms)
                .map_err(|e| AgentError::Internal(format!("Failed to set execute permission: {}", e)))?;
        }

        println!("Ollama downloaded successfully to {:?}", self.ollama_path);
        Ok(())
    }

    /// Ollamaを起動
    fn start(&mut self) -> AgentResult<()> {
        let child = Command::new(&self.ollama_path)
            .arg("serve")
            .env("OLLAMA_HOST", format!("0.0.0.0:{}", self.port))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AgentError::OllamaConnection(format!("Failed to start Ollama: {}", e)))?;

        self.process = Some(child);
        println!("Ollama process started");
        Ok(())
    }

    /// Ollamaが起動しているか確認
    pub async fn is_running(&self) -> bool {
        let url = format!("http://localhost:{}/api/tags", self.port);
        reqwest::get(&url).await.is_ok()
    }

    /// Ollama起動を待つ
    async fn wait_for_startup(&self) -> AgentResult<()> {
        let max_attempts = 30; // 30秒待つ

        for _ in 0..max_attempts {
            if self.is_running().await {
                println!("Ollama is ready");
                return Ok(());
            }
            sleep(Duration::from_secs(1)).await;
        }

        Err(AgentError::OllamaConnection(
            "Ollama failed to start within 30 seconds".to_string(),
        ))
    }

    /// Ollamaのバージョンを取得
    pub async fn get_version(&self) -> AgentResult<String> {
        let output = Command::new(&self.ollama_path)
            .arg("--version")
            .output()
            .map_err(|e| AgentError::OllamaConnection(format!("Failed to get Ollama version: {}", e)))?;

        if !output.status.success() {
            return Err(AgentError::OllamaConnection(
                "Failed to get Ollama version".to_string(),
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        Ok(version)
    }

    /// Ollamaを停止
    pub fn stop(&mut self) -> AgentResult<()> {
        if let Some(mut process) = self.process.take() {
            process
                .kill()
                .map_err(|e| AgentError::Internal(format!("Failed to kill Ollama process: {}", e)))?;
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

/// Ollamaディレクトリを取得
fn get_ollama_directory() -> PathBuf {
    if cfg!(windows) {
        // Windows: %LOCALAPPDATA%\OllamaCoordinator\ollama
        let local_app_data = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| String::from("C:\\Users\\Default\\AppData\\Local"));
        PathBuf::from(local_app_data).join("OllamaCoordinator").join("ollama")
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
    if cfg!(windows) {
        "https://ollama.com/download/ollama-windows-amd64.exe".to_string()
    } else if cfg!(target_os = "macos") {
        // macOSの場合、アーキテクチャに応じて分岐
        if cfg!(target_arch = "aarch64") {
            "https://ollama.com/download/ollama-darwin-arm64".to_string()
        } else {
            "https://ollama.com/download/ollama-darwin-amd64".to_string()
        }
    } else {
        // Linux
        "https://ollama.com/download/ollama-linux-amd64".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_ollama_directory() {
        let dir = get_ollama_directory();
        assert!(dir.to_string_lossy().contains("OllamaCoordinator") ||
                dir.to_string_lossy().contains("ollama-coordinator"));
    }

    #[test]
    fn test_get_ollama_download_url() {
        let url = get_ollama_download_url();
        assert!(url.starts_with("https://ollama.com/download/"));
    }

    #[tokio::test]
    async fn test_ollama_manager_creation() {
        let manager = OllamaManager::new(11434);
        assert_eq!(manager.port, 11434);
        assert!(manager.ollama_path.to_string_lossy().contains("ollama"));
    }
}
