//! リクエスト/レスポンス履歴のストレージ層
//!
//! JSONファイルベースでリクエスト履歴を永続化

use chrono::{DateTime, Duration, Utc};
use llm_router_common::{
    error::{RouterError, RouterResult},
    protocol::{RecordStatus, RequestResponseRecord},
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use uuid::Uuid;

/// リクエスト履歴ストレージ
pub struct RequestHistoryStorage {
    file_path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl RequestHistoryStorage {
    /// 新しいストレージインスタンスを作成
    pub fn new() -> RouterResult<Self> {
        let file_path = get_history_file_path()?;
        Ok(Self {
            file_path,
            lock: Arc::new(Mutex::new(())),
        })
    }

    /// レコードを保存
    pub async fn save_record(&self, record: &RequestResponseRecord) -> RouterResult<()> {
        let _guard = self.lock.lock().await;

        // 既存のレコードを読み込み
        let mut records = self.load_records_unlocked().await?;

        // レコードを追加
        records.push(record.clone());

        // ファイルに保存
        self.save_records_unlocked(&records).await?;

        Ok(())
    }

    /// すべてのレコードを読み込み
    pub async fn load_records(&self) -> RouterResult<Vec<RequestResponseRecord>> {
        let _guard = self.lock.lock().await;
        self.load_records_unlocked().await
    }

    /// 7日より古いレコードを削除
    pub async fn cleanup_old_records(&self, max_age: Duration) -> RouterResult<()> {
        let _guard = self.lock.lock().await;

        let mut records = self.load_records_unlocked().await?;
        let cutoff = Utc::now() - max_age;

        // 古いレコードを除外
        records.retain(|r| r.timestamp > cutoff);

        self.save_records_unlocked(&records).await?;

        Ok(())
    }

    /// レコードをフィルタリング＆ページネーション
    pub async fn filter_and_paginate(
        &self,
        filter: &RecordFilter,
        page: usize,
        per_page: usize,
    ) -> RouterResult<FilteredRecords> {
        let records = self.load_records().await?;

        // フィルタリング
        let filtered: Vec<RequestResponseRecord> =
            records.into_iter().filter(|r| filter.matches(r)).collect();

        let total_count = filtered.len();

        // ページネーション
        let start = (page.saturating_sub(1)) * per_page;
        let paginated: Vec<RequestResponseRecord> =
            filtered.into_iter().skip(start).take(per_page).collect();

        Ok(FilteredRecords {
            records: paginated,
            total_count,
            page,
            per_page,
        })
    }

    /// ロックなしでレコードを読み込み（内部使用）
    async fn load_records_unlocked(&self) -> RouterResult<Vec<RequestResponseRecord>> {
        // ファイルが存在しない場合は空配列
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.file_path)
            .await
            .map_err(|e| RouterError::Database(format!("Failed to read history file: {}", e)))?;

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        serde_json::from_str(&content)
            .map_err(|e| RouterError::Database(format!("Failed to parse history file: {}", e)))
    }

    /// ロックなしでレコードを保存（内部使用）
    async fn save_records_unlocked(&self, records: &[RequestResponseRecord]) -> RouterResult<()> {
        // ディレクトリが存在しない場合は作成（冪等なので常に実行）
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| RouterError::Database(format!("Failed to create directory: {}", e)))?;
        }

        // JSONに変換
        let json = serde_json::to_string_pretty(records)
            .map_err(|e| RouterError::Database(format!("Failed to serialize records: {}", e)))?;

        // 一時ファイルに書き込んでから rename（破損防止）
        let temp_path = self.file_path.with_extension("tmp");
        fs::write(&temp_path, json)
            .await
            .map_err(|e| RouterError::Database(format!("Failed to write temp file: {}", e)))?;

        fs::rename(&temp_path, &self.file_path)
            .await
            .map_err(|e| RouterError::Database(format!("Failed to rename temp file: {}", e)))?;

        Ok(())
    }
}

impl Default for RequestHistoryStorage {
    fn default() -> Self {
        Self::new().expect("Failed to create RequestHistoryStorage")
    }
}

/// レコードフィルタ
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    /// モデル名フィルタ（部分一致）
    pub model: Option<String>,
    /// ノードIDフィルタ
    pub node_id: Option<Uuid>,
    /// ステータスフィルタ
    pub status: Option<FilterStatus>,
    /// 開始時刻フィルタ
    pub start_time: Option<DateTime<Utc>>,
    /// 終了時刻フィルタ
    pub end_time: Option<DateTime<Utc>>,
}

impl RecordFilter {
    /// レコードがフィルタ条件に一致するか
    pub fn matches(&self, record: &RequestResponseRecord) -> bool {
        if let Some(ref model) = self.model {
            if !record.model.contains(model) {
                return false;
            }
        }

        if let Some(node_id) = self.node_id {
            if record.node_id != node_id {
                return false;
            }
        }

        if let Some(ref status) = self.status {
            match (status, &record.status) {
                (FilterStatus::Success, RecordStatus::Success) => {}
                (FilterStatus::Error, RecordStatus::Error { .. }) => {}
                _ => return false,
            }
        }

        if let Some(start_time) = self.start_time {
            if record.timestamp < start_time {
                return false;
            }
        }

        if let Some(end_time) = self.end_time {
            if record.timestamp > end_time {
                return false;
            }
        }

        true
    }
}

/// フィルタ用のステータス
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStatus {
    /// 成功したリクエスト
    Success,
    /// 失敗したリクエスト
    Error,
}

/// フィルタ済みレコード
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilteredRecords {
    /// フィルタ・ページネーション適用後のレコード
    pub records: Vec<RequestResponseRecord>,
    /// フィルタ適用後の総件数
    pub total_count: usize,
    /// 現在のページ番号
    pub page: usize,
    /// 1ページあたりの件数
    pub per_page: usize,
}

/// 履歴ファイルのパスを取得
fn get_history_file_path() -> RouterResult<PathBuf> {
    let data_dir = if let Ok(test_dir) = std::env::var("LLM_ROUTER_DATA_DIR") {
        PathBuf::from(test_dir)
    } else {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| RouterError::Database("Failed to get home directory".to_string()))?;

        PathBuf::from(home).join(".llm-router")
    };

    Ok(data_dir.join("request_history.json"))
}

/// 定期クリーンアップタスクを開始
pub fn start_cleanup_task(storage: Arc<RequestHistoryStorage>) {
    tokio::spawn(async move {
        // 起動時に1回実行
        if let Err(e) = storage.cleanup_old_records(Duration::days(7)).await {
            tracing::error!("Initial cleanup failed: {}", e);
        }

        // 1時間ごとに実行
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;

            if let Err(e) = storage.cleanup_old_records(Duration::days(7)).await {
                tracing::error!("Periodic cleanup failed: {}", e);
            } else {
                tracing::info!("Periodic cleanup completed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::TEST_LOCK;
    use llm_router_common::protocol::RequestType;
    use std::net::IpAddr;
    use tempfile::tempdir;

    fn create_test_record(timestamp: DateTime<Utc>) -> RequestResponseRecord {
        RequestResponseRecord {
            id: Uuid::new_v4(),
            timestamp,
            request_type: RequestType::Chat,
            model: "test-model".to_string(),
            node_id: Uuid::new_v4(),
            agent_machine_name: "test-agent".to_string(),
            agent_ip: "192.168.1.100".parse::<IpAddr>().unwrap(),
            client_ip: Some("10.0.0.10".parse::<IpAddr>().unwrap()),
            request_body: serde_json::json!({"test": "request"}),
            response_body: Some(serde_json::json!({"test": "response"})),
            duration_ms: 1000,
            status: RecordStatus::Success,
            completed_at: timestamp + Duration::seconds(1),
        }
    }

    #[tokio::test]
    async fn test_save_and_load_record() {
        let _lock = TEST_LOCK.lock().await;

        let temp_dir = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", temp_dir.path());

        let storage = RequestHistoryStorage::new().unwrap();
        let record = create_test_record(Utc::now());

        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, record.id);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_cleanup_old_records() {
        let _lock = TEST_LOCK.lock().await;

        let temp_dir = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", temp_dir.path());

        let storage = RequestHistoryStorage::new().unwrap();

        // 8日前のレコード（削除される）
        let old_record = create_test_record(Utc::now() - Duration::days(8));
        storage.save_record(&old_record).await.unwrap();

        // 6日前のレコード（残る）
        let new_record = create_test_record(Utc::now() - Duration::days(6));
        storage.save_record(&new_record).await.unwrap();

        storage
            .cleanup_old_records(Duration::days(7))
            .await
            .unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, new_record.id);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_filter_by_model() {
        let _lock = TEST_LOCK.lock().await;

        let temp_dir = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", temp_dir.path());

        let storage = RequestHistoryStorage::new().unwrap();

        let mut record1 = create_test_record(Utc::now());
        record1.model = "llama2".to_string();
        storage.save_record(&record1).await.unwrap();

        let mut record2 = create_test_record(Utc::now());
        record2.model = "codellama".to_string();
        storage.save_record(&record2).await.unwrap();

        let filter = RecordFilter {
            model: Some("llama2".to_string()),
            ..Default::default()
        };

        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].model, "llama2");

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }

    #[tokio::test]
    async fn test_pagination() {
        let _lock = TEST_LOCK.lock().await;

        let temp_dir = tempdir().unwrap();
        std::env::set_var("LLM_ROUTER_DATA_DIR", temp_dir.path());

        let storage = RequestHistoryStorage::new().unwrap();

        // 5件のレコードを作成
        for _ in 0..5 {
            let record = create_test_record(Utc::now());
            storage.save_record(&record).await.unwrap();
        }

        // 1ページ目（2件）
        let filter = RecordFilter::default();
        let result = storage.filter_and_paginate(&filter, 1, 2).await.unwrap();
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.total_count, 5);
        assert_eq!(result.page, 1);

        // 2ページ目（2件）
        let result = storage.filter_and_paginate(&filter, 2, 2).await.unwrap();
        assert_eq!(result.records.len(), 2);

        // 3ページ目（1件）
        let result = storage.filter_and_paginate(&filter, 3, 2).await.unwrap();
        assert_eq!(result.records.len(), 1);

        std::env::remove_var("LLM_ROUTER_DATA_DIR");
    }
}
