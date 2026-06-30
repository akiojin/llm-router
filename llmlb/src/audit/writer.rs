//! 監査ログの非同期バッファライター (SPEC-8301d106)
//!
//! mpscチャネルでエントリを受信し、定期的にDBへ一括書き込みする。
//! バッチ間隔ごとにSHA-256ハッシュチェーンのバッチを生成する。

use crate::audit::hash_chain;
use crate::audit::types::{AuditBatchHash, AuditLogEntry};
use crate::db::audit_log::AuditLogStorage;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// 監査ログライター設定
pub struct AuditLogWriterConfig {
    /// フラッシュ間隔（秒）。デフォルト: 30
    pub flush_interval_secs: u64,
    /// バッファ上限エントリ数。デフォルト: 10000
    pub buffer_capacity: usize,
    /// バッチハッシュ生成間隔（秒）。デフォルト: 300（5分）
    pub batch_interval_secs: u64,
}

impl Default for AuditLogWriterConfig {
    fn default() -> Self {
        Self {
            flush_interval_secs: std::env::var("LLMLB_AUDIT_FLUSH_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            buffer_capacity: std::env::var("LLMLB_AUDIT_BUFFER_CAPACITY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000),
            batch_interval_secs: std::env::var("LLMLB_AUDIT_BATCH_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        }
    }
}

/// 監査ログの非同期ライター
///
/// mpscチャネル経由でAuditLogEntryを受信し、
/// 定期的にDBへ一括書き込みする。
/// Clone可能（senderのクローン）。
#[derive(Clone)]
pub struct AuditLogWriter {
    sender: mpsc::Sender<AuditLogEntry>,
}

impl AuditLogWriter {
    /// 新しいAuditLogWriterを作成し、バックグラウンドタスクを起動
    pub fn new(storage: AuditLogStorage, config: AuditLogWriterConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.buffer_capacity);

        tokio::spawn(Self::background_task(rx, storage, config));

        Self { sender: tx }
    }

    /// エントリをバッファに送信（非同期、ブロックしない）
    pub fn send(&self, entry: AuditLogEntry) {
        if let Err(e) = self.sender.try_send(entry) {
            warn!("Failed to send audit log entry: {}", e);
        }
    }

    /// バックグラウンドフラッシュタスク
    async fn background_task(
        mut rx: mpsc::Receiver<AuditLogEntry>,
        storage: AuditLogStorage,
        config: AuditLogWriterConfig,
    ) {
        let mut buffer = VecDeque::with_capacity(config.buffer_capacity);
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(config.flush_interval_secs));
        // 最初のtickはすぐに発火するのでスキップ
        interval.tick().await;

        let batch_interval = std::time::Duration::from_secs(config.batch_interval_secs);
        let mut last_batch_time = tokio::time::Instant::now();

        loop {
            tokio::select! {
                // フラッシュ間隔
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        let should_create_batch = last_batch_time.elapsed() >= batch_interval;
                        Self::flush_buffer(&mut buffer, &storage, should_create_batch).await;
                        if should_create_batch {
                            last_batch_time = tokio::time::Instant::now();
                        }
                    }
                }
                // チャネルからエントリ受信
                entry = rx.recv() => {
                    match entry {
                        Some(entry) => {
                            // バッファ上限チェック
                            if buffer.len() >= config.buffer_capacity {
                                let discarded = buffer.pop_front();
                                warn!(
                                    "Audit log buffer overflow (capacity: {}), discarding oldest entry: {:?}",
                                    config.buffer_capacity,
                                    discarded.map(|e| e.request_path)
                                );
                            }
                            buffer.push_back(entry);
                        }
                        None => {
                            // チャネルが閉じられた → 残りをフラッシュして終了
                            if !buffer.is_empty() {
                                info!("Audit log writer shutting down, flushing {} remaining entries", buffer.len());
                                Self::flush_buffer(&mut buffer, &storage, true).await;
                            }
                            info!("Audit log writer background task stopped");
                            return;
                        }
                    }
                }
            }
        }
    }

    /// バッファ内エントリをDBに一括書き込み
    ///
    /// `create_batch`がtrueの場合、バッチハッシュを生成してハッシュチェーンに組み込む
    async fn flush_buffer(
        buffer: &mut VecDeque<AuditLogEntry>,
        storage: &AuditLogStorage,
        create_batch: bool,
    ) {
        let entries: Vec<AuditLogEntry> = buffer.drain(..).collect();
        let count = entries.len();

        // まずエントリをDB挿入（batch_id=NULL）
        if let Err(e) = storage.insert_batch(&entries).await {
            warn!(
                "Failed to flush audit log entries: {}. {} entries lost.",
                e, count
            );
            return;
        }

        info!("Flushed {} audit log entries to database", count);

        // バッチ作成が必要な場合、未割当エントリをまとめてバッチ化
        if create_batch {
            if let Err(e) = Self::create_batch_hash(storage).await {
                warn!("Failed to create batch hash: {}", e);
            }
        }
    }

    /// 未割当エントリをまとめてバッチハッシュを生成
    async fn create_batch_hash(
        storage: &AuditLogStorage,
    ) -> Result<(), crate::common::error::LbError> {
        let unbatched = storage.get_unbatched_entries().await?;
        if unbatched.is_empty() {
            return Ok(());
        }

        // 前バッチ情報を取得
        let latest = storage.get_latest_batch_hash().await?;
        let previous_hash = latest
            .as_ref()
            .map(|b| b.hash.clone())
            .unwrap_or_else(|| hash_chain::GENESIS_HASH.to_string());
        let sequence_number = latest.as_ref().map(|b| b.sequence_number + 1).unwrap_or(1);

        // バッチのタイムスタンプ範囲
        let batch_start = unbatched
            .iter()
            .map(|e| e.timestamp)
            .min()
            .expect("unbatched is non-empty");
        let batch_end = unbatched
            .iter()
            .map(|e| e.timestamp)
            .max()
            .expect("unbatched is non-empty");

        // バッチハッシュを計算
        let batch_hash = hash_chain::compute_batch_hash(
            &previous_hash,
            sequence_number,
            &batch_start,
            &batch_end,
            unbatched.len() as i64,
            &unbatched,
        );

        // バッチハッシュをDBに保存
        let batch = AuditBatchHash {
            id: None,
            sequence_number,
            batch_start,
            batch_end,
            record_count: unbatched.len() as i64,
            hash: batch_hash,
            previous_hash,
        };
        // バッチ行の挿入とエントリの batch_id 更新を単一トランザクションで実行する。
        // 途中失敗でバッチ行だけが残ると、未割当エントリが次回再取得されて別バッチへ
        // 二重組み込みされ、ハッシュチェーン検証が恒久的に不整合となるため原子的に行う。
        let entry_ids: Vec<i64> = unbatched.iter().filter_map(|e| e.id).collect();
        storage
            .insert_batch_hash_with_entries(&batch, &entry_ids)
            .await?;

        info!(
            "Created batch hash #{} with {} entries",
            sequence_number,
            unbatched.len()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::types::{ActorType, AuditLogEntry, AuditLogFilter};
    use crate::db::audit_log::AuditLogStorage;
    use chrono::Utc;

    async fn create_test_pool() -> sqlx::SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    fn create_test_entry(path: &str) -> AuditLogEntry {
        AuditLogEntry {
            id: None,
            timestamp: Utc::now(),
            http_method: "GET".to_string(),
            request_path: path.to_string(),
            status_code: 200,
            actor_type: ActorType::User,
            actor_id: Some("user-1".to_string()),
            actor_username: Some("admin".to_string()),
            api_key_owner_id: None,
            client_ip: Some("127.0.0.1".to_string()),
            duration_ms: Some(10),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            model_name: None,
            endpoint_id: None,
            detail: None,
            batch_id: None,
            is_migrated: false,
        }
    }

    #[tokio::test]
    async fn test_send_entry() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let writer = AuditLogWriter::new(
            storage,
            AuditLogWriterConfig {
                flush_interval_secs: 1,
                buffer_capacity: 100,
                batch_interval_secs: 300,
            },
        );

        writer.send(create_test_entry("/api/test"));

        // フラッシュを待つ
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM audit_log_entries WHERE is_migrated = 0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(count.0 >= 1, "Expected at least 1 entry, got {}", count.0);
    }

    #[tokio::test]
    async fn test_shutdown_flushes_remaining() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let writer = AuditLogWriter::new(
            storage,
            AuditLogWriterConfig {
                flush_interval_secs: 300,
                buffer_capacity: 100,
                batch_interval_secs: 300,
            },
        );

        writer.send(create_test_entry("/api/shutdown-test"));

        drop(writer);

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path = '/api/shutdown-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 1, "Entry should be flushed on shutdown");
    }

    #[tokio::test]
    async fn test_shutdown_creates_batch_hash() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let writer = AuditLogWriter::new(
            storage.clone(),
            AuditLogWriterConfig {
                flush_interval_secs: 300,
                buffer_capacity: 100,
                batch_interval_secs: 300,
            },
        );

        writer.send(create_test_entry("/api/batch-test-1"));
        writer.send(create_test_entry("/api/batch-test-2"));

        drop(writer);

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // バッチハッシュが作成されたことを確認
        let batches = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(batches.len(), 1, "Shutdown should create a batch hash");
        assert_eq!(batches[0].sequence_number, 1);
        assert_eq!(batches[0].record_count, 2);
        assert_eq!(
            batches[0].previous_hash,
            hash_chain::GENESIS_HASH,
            "First batch should reference genesis hash"
        );

        // エントリにbatch_idが設定されたことを確認
        let entries = storage.query(&AuditLogFilter::default()).await.unwrap();
        for entry in &entries {
            assert!(
                entry.batch_id.is_some(),
                "Entry should have batch_id after shutdown"
            );
        }
    }

    #[tokio::test]
    async fn test_flush_without_batch_when_interval_not_elapsed() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let writer = AuditLogWriter::new(
            storage.clone(),
            AuditLogWriterConfig {
                flush_interval_secs: 1,
                buffer_capacity: 100,
                batch_interval_secs: 3600, // 1時間（テスト中は経過しない）
            },
        );

        writer.send(create_test_entry("/api/no-batch"));

        // フラッシュを待つ（バッチ間隔は未経過）
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // エントリは保存されるがバッチハッシュはまだない
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM audit_log_entries WHERE is_migrated = 0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(count.0 >= 1);

        let batches = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(
            batches.len(),
            0,
            "No batch hash should be created before interval"
        );

        // エントリのbatch_idはNULL
        let entries = storage.query(&AuditLogFilter::default()).await.unwrap();
        for entry in &entries {
            assert!(
                entry.batch_id.is_none(),
                "Entry should not have batch_id before batch interval"
            );
        }
    }

    #[tokio::test]
    async fn test_create_batch_hash_directly() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        // エントリを直接挿入（batch_id=NULL）
        let entries = vec![
            create_test_entry("/api/direct-1"),
            create_test_entry("/api/direct-2"),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // バッチハッシュを手動で作成
        AuditLogWriter::create_batch_hash(&storage).await.unwrap();

        // 検証
        let batches = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].sequence_number, 1);
        assert_eq!(batches[0].record_count, 2);
        assert_eq!(batches[0].hash.len(), 64, "SHA-256 hex should be 64 chars");

        // エントリにbatch_idが設定された
        let all_entries = storage.query(&AuditLogFilter::default()).await.unwrap();
        for entry in &all_entries {
            assert!(entry.batch_id.is_some());
        }

        // ハッシュチェーン検証
        let result = hash_chain::verify_chain(&storage).await.unwrap();
        assert!(result.valid, "Hash chain should be valid");
        assert_eq!(result.batches_checked, 1);
    }
}
