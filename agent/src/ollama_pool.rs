//! 複数Ollamaプロセスをモデル単位で管理するプール

use crate::ollama::OllamaManager;
use ollama_coordinator_common::error::{AgentError, AgentResult};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// モデル名 -> ポート のマッピングと対応する OllamaManager
#[derive(Clone)]
pub struct OllamaPool {
    base_port: u16,
    max_port: u16,
    /// モデル名 -> (ポート, Manager)
    managers: std::sync::Arc<Mutex<HashMap<String, (u16, OllamaManager)>>>,
}

impl OllamaPool {
    /// プールを初期化し、使用可能なポート範囲を指定する
    pub fn new(base_port: u16, max_port: u16) -> Self {
        Self {
            base_port,
            max_port,
            managers: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 指定モデルの Ollama を確保し、ポートを返す
    pub async fn ensure(&self, model: &str) -> AgentResult<u16> {
        // 既に存在する場合は即返す
        if let Some(port) = self.get_port_if_exists(model).await {
            return Ok(port);
        }

        // 新規起動
        let mut map = self.managers.lock().await;
        if let Some((port, _)) = map.get(model) {
            return Ok(*port);
        }

        let port = self.allocate_port(&map)?;
        let mut manager = OllamaManager::new(port);
        manager.ensure_running().await?;
        manager.ensure_model(model).await?;

        map.insert(model.to_string(), (port, manager));
        Ok(port)
    }

    /// 既にある場合のポート取得（ロック短時間）
    async fn get_port_if_exists(&self, model: &str) -> Option<u16> {
        let map = self.managers.lock().await;
        map.get(model).map(|(p, _)| *p)
    }

    fn allocate_port(&self, map: &HashMap<String, (u16, OllamaManager)>) -> AgentResult<u16> {
        for port in self.base_port..=self.max_port {
            let used = map.values().any(|(p, _)| *p == port);
            if !used {
                return Ok(port);
            }
        }
        Err(AgentError::Internal(
            "No available port for new Ollama instance".to_string(),
        ))
    }
}
