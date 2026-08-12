//! SHA-256バッチハッシュチェーン (SPEC-8301d106)
//!
//! 監査ログの改ざん検知のためのハッシュチェーン実装。
//! バッチ単位でSHA-256ハッシュを計算し、前バッチのハッシュを含むチェーンを構成する。

use crate::audit::types::AuditLogEntry;
use crate::common::error::RouterResult;
use crate::db::audit_log::AuditLogStorage;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tracing::warn;

/// ジェネシスバッチのprevious_hash（ゼロハッシュ）
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// チェーン検証結果
#[derive(Debug, Clone, Serialize)]
pub struct ChainVerificationResult {
    /// 検証が成功したか
    pub valid: bool,
    /// 検証したバッチ数
    pub batches_checked: i64,
    /// 改ざんが検出されたバッチ連番（該当する場合）
    pub tampered_batch: Option<i64>,
    /// エラーメッセージ（該当する場合）
    pub message: Option<String>,
}

/// 個別エントリのハッシュを計算
///
/// `SHA-256(timestamp || method || path || status || actor_type || actor_id || client_ip)`
pub fn compute_record_hash(entry: &AuditLogEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry.timestamp.to_rfc3339().as_bytes());
    hasher.update(entry.http_method.as_bytes());
    hasher.update(entry.request_path.as_bytes());
    hasher.update(entry.status_code.to_string().as_bytes());
    hasher.update(entry.actor_type.as_str().as_bytes());
    if let Some(ref actor_id) = entry.actor_id {
        hasher.update(actor_id.as_bytes());
    }
    // client_ip を含める（None の場合は空文字列）
    if let Some(ref client_ip) = entry.client_ip {
        hasher.update(client_ip.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `client_ip` がハッシュ対象になる前の個別エントリハッシュを計算する。
fn compute_legacy_record_hash(entry: &AuditLogEntry) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry.timestamp.to_rfc3339().as_bytes());
    hasher.update(entry.http_method.as_bytes());
    hasher.update(entry.request_path.as_bytes());
    hasher.update(entry.status_code.to_string().as_bytes());
    hasher.update(entry.actor_type.as_str().as_bytes());
    if let Some(ref actor_id) = entry.actor_id {
        hasher.update(actor_id.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// バッチハッシュを計算
///
/// `SHA-256(previous_hash || seq || start || end || count || records_hash)`
pub fn compute_batch_hash(
    previous_hash: &str,
    seq: i64,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    count: i64,
    entries: &[AuditLogEntry],
) -> String {
    // 全エントリのハッシュを連結して中間ハッシュを計算
    let mut records_hasher = Sha256::new();
    for entry in entries {
        records_hasher.update(compute_record_hash(entry).as_bytes());
    }
    let records_hash: String = records_hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(seq.to_string().as_bytes());
    hasher.update(start.to_rfc3339().as_bytes());
    hasher.update(end.to_rfc3339().as_bytes());
    hasher.update(count.to_string().as_bytes());
    hasher.update(records_hash.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `client_ip` 導入前のレコードハッシュを使ってバッチハッシュを計算する。
///
/// Issue #694 の一回限りの移行で、保存済みバッチが旧方式として正当かを
/// 検証するためだけに使用する。
pub(crate) fn compute_legacy_batch_hash(
    previous_hash: &str,
    seq: i64,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    count: i64,
    entries: &[AuditLogEntry],
) -> String {
    let mut records_hasher = Sha256::new();
    for entry in entries {
        records_hasher.update(compute_legacy_record_hash(entry).as_bytes());
    }
    let records_hash: String = records_hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(previous_hash.as_bytes());
    hasher.update(seq.to_string().as_bytes());
    hasher.update(start.to_rfc3339().as_bytes());
    hasher.update(end.to_rfc3339().as_bytes());
    hasher.update(count.to_string().as_bytes());
    hasher.update(records_hash.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 全バッチのハッシュチェーンを検証
pub async fn verify_chain(storage: &AuditLogStorage) -> RouterResult<ChainVerificationResult> {
    let batches = storage.get_all_batch_hashes().await?;

    if batches.is_empty() {
        return Ok(ChainVerificationResult {
            valid: true,
            batches_checked: 0,
            tampered_batch: None,
            message: Some("No batches to verify".to_string()),
        });
    }

    let mut expected_previous_hash = GENESIS_HASH.to_string();

    for batch in &batches {
        // 前バッチハッシュの整合性チェック
        if batch.previous_hash != expected_previous_hash {
            warn!(
                batch_seq = batch.sequence_number,
                expected = %expected_previous_hash,
                actual = %batch.previous_hash,
                "Hash chain broken: previous_hash mismatch"
            );
            return Ok(ChainVerificationResult {
                valid: false,
                batches_checked: batch.sequence_number,
                tampered_batch: Some(batch.sequence_number),
                message: Some(format!(
                    "Previous hash mismatch at batch {}",
                    batch.sequence_number
                )),
            });
        }

        // バッチ内エントリを取得してハッシュを再計算
        let batch_id = batch.id.unwrap_or(0);
        let entries = storage.get_entries_for_batch(batch_id).await?;

        let recomputed = compute_batch_hash(
            &batch.previous_hash,
            batch.sequence_number,
            &batch.batch_start,
            &batch.batch_end,
            batch.record_count,
            &entries,
        );

        if recomputed != batch.hash {
            warn!(
                batch_seq = batch.sequence_number,
                expected = %batch.hash,
                recomputed = %recomputed,
                "Hash chain broken: batch hash mismatch"
            );
            return Ok(ChainVerificationResult {
                valid: false,
                batches_checked: batch.sequence_number,
                tampered_batch: Some(batch.sequence_number),
                message: Some(format!(
                    "Batch hash mismatch at batch {}",
                    batch.sequence_number
                )),
            });
        }

        expected_previous_hash = batch.hash.clone();
    }

    Ok(ChainVerificationResult {
        valid: true,
        batches_checked: batches.len() as i64,
        tampered_batch: None,
        message: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::types::{ActorType, AuditBatchHash, AuditLogEntry, AuditLogFilter};
    use chrono::Utc;

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

    #[test]
    fn test_compute_record_hash_deterministic() {
        let entry = create_test_entry("/api/test");
        let hash1 = compute_record_hash(&entry);
        let hash2 = compute_record_hash(&entry);
        assert_eq!(hash1, hash2, "Same entry should produce same hash");
        assert_eq!(hash1.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[test]
    fn test_compute_record_hash_different_entries() {
        let entry1 = create_test_entry("/api/users");
        let entry2 = create_test_entry("/api/endpoints");
        let hash1 = compute_record_hash(&entry1);
        let hash2 = compute_record_hash(&entry2);
        assert_ne!(
            hash1, hash2,
            "Different entries should produce different hashes"
        );
    }

    #[test]
    fn test_compute_record_hash_includes_client_ip() {
        let mut entry1 = create_test_entry("/api/test");
        let mut entry2 = create_test_entry("/api/test");

        entry1.client_ip = Some("192.168.1.1".to_string());
        entry2.client_ip = Some("192.168.1.2".to_string());

        let hash1 = compute_record_hash(&entry1);
        let hash2 = compute_record_hash(&entry2);

        assert_ne!(
            hash1, hash2,
            "Different client_ip should produce different hashes"
        );
    }

    #[test]
    fn test_compute_record_hash_client_ip_null() {
        let mut entry = create_test_entry("/api/test");
        entry.client_ip = None;

        let hash1 = compute_record_hash(&entry);
        let hash2 = compute_record_hash(&entry);

        assert_eq!(
            hash1, hash2,
            "NULL client_ip should produce consistent hashes"
        );
        assert_eq!(hash1.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[test]
    fn test_compute_batch_hash_deterministic() {
        let now = Utc::now();
        let entries = vec![create_test_entry("/api/test")];
        let hash1 = compute_batch_hash(GENESIS_HASH, 1, &now, &now, 1, &entries);
        let hash2 = compute_batch_hash(GENESIS_HASH, 1, &now, &now, 1, &entries);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_genesis_hash_is_zero() {
        assert_eq!(GENESIS_HASH.len(), 64);
        assert!(GENESIS_HASH.chars().all(|c| c == '0'));
    }

    #[test]
    fn test_compute_batch_hash_changes_with_different_previous() {
        let now = Utc::now();
        let entries = vec![create_test_entry("/api/test")];
        let hash1 = compute_batch_hash(GENESIS_HASH, 1, &now, &now, 1, &entries);
        let hash2 = compute_batch_hash(&"a".repeat(64), 1, &now, &now, 1, &entries);
        assert_ne!(
            hash1, hash2,
            "Different previous_hash should change batch hash"
        );
    }

    async fn create_test_pool() -> sqlx::SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_verify_chain_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);
        let result = verify_chain(&storage).await.unwrap();
        assert!(result.valid);
        assert_eq!(result.batches_checked, 0);
    }

    #[tokio::test]
    async fn test_verify_chain_valid_single_batch() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        // テストエントリを挿入
        let entries = vec![create_test_entry("/api/test")];
        storage.insert_batch(&entries).await.unwrap();

        // 正しいバッチハッシュを計算して挿入
        let now = Utc::now();
        let inserted_entries = storage.query(&AuditLogFilter::default()).await.unwrap();
        let hash = compute_batch_hash(GENESIS_HASH, 1, &now, &now, 1, &inserted_entries);

        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: now,
            batch_end: now,
            record_count: 1,
            hash: hash.clone(),
            previous_hash: GENESIS_HASH.to_string(),
        };
        let batch_id = storage.insert_batch_hash(&batch).await.unwrap();

        // エントリのbatch_idを更新
        let entry_ids: Vec<i64> = inserted_entries.iter().filter_map(|e| e.id).collect();
        storage
            .update_entries_batch_id(&entry_ids, batch_id)
            .await
            .unwrap();

        let result = verify_chain(&storage).await.unwrap();
        assert!(result.valid);
        assert_eq!(result.batches_checked, 1);
        assert!(result.tampered_batch.is_none());
    }

    #[tokio::test]
    async fn test_verify_chain_detects_tampered_hash() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        // テストエントリを挿入
        let entries = vec![create_test_entry("/api/test")];
        storage.insert_batch(&entries).await.unwrap();

        let now = Utc::now();
        // 意図的に間違ったハッシュでバッチを作成
        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: now,
            batch_end: now,
            record_count: 1,
            hash: "tampered_hash_value_that_is_definitely_wrong_and_invalid".to_string(),
            previous_hash: GENESIS_HASH.to_string(),
        };
        let batch_id = storage.insert_batch_hash(&batch).await.unwrap();

        let inserted_entries = storage.query(&AuditLogFilter::default()).await.unwrap();
        let entry_ids: Vec<i64> = inserted_entries.iter().filter_map(|e| e.id).collect();
        storage
            .update_entries_batch_id(&entry_ids, batch_id)
            .await
            .unwrap();

        let result = verify_chain(&storage).await.unwrap();
        assert!(!result.valid);
        assert_eq!(result.tampered_batch, Some(1));
    }
}
