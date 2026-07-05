//! 監査ログのバッチハッシュ（改ざん検知チェーン）に関する DB アクセス。
//!
//! `audit_batch_hashes` テーブルへの挿入・全件取得・最新取得と、
//! 行マッピング用の `AuditBatchHashRow` およびその変換を集約する。

use super::*;

/// sqlx::FromRow用の行構造体（バッチハッシュ）
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct AuditBatchHashRow {
    pub(super) id: i64,
    pub(super) sequence_number: i64,
    pub(super) batch_start: String,
    pub(super) batch_end: String,
    pub(super) record_count: i64,
    pub(super) hash: String,
    pub(super) previous_hash: String,
}

impl TryFrom<AuditBatchHashRow> for AuditBatchHash {
    type Error = LbError;

    fn try_from(row: AuditBatchHashRow) -> Result<Self, Self::Error> {
        let batch_start = chrono::DateTime::parse_from_rfc3339(&row.batch_start)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| LbError::Database(format!("Failed to parse batch_start: {}", e)))?;
        let batch_end = chrono::DateTime::parse_from_rfc3339(&row.batch_end)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| LbError::Database(format!("Failed to parse batch_end: {}", e)))?;

        Ok(AuditBatchHash {
            id: Some(row.id),
            sequence_number: row.sequence_number,
            batch_start,
            batch_end,
            record_count: row.record_count,
            hash: row.hash,
            previous_hash: row.previous_hash,
        })
    }
}

impl AuditLogStorage {
    /// バッチハッシュを挿入してIDを返す
    pub async fn insert_batch_hash(&self, batch: &AuditBatchHash) -> RouterResult<i64> {
        let batch_start_str = batch.batch_start.to_rfc3339();
        let batch_end_str = batch.batch_end.to_rfc3339();

        let result = sqlx::query(
            r#"INSERT INTO audit_batch_hashes (
                sequence_number, batch_start, batch_end, record_count, hash, previous_hash
            ) VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(batch.sequence_number)
        .bind(&batch_start_str)
        .bind(&batch_end_str)
        .bind(batch.record_count)
        .bind(&batch.hash)
        .bind(&batch.previous_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to insert batch hash: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// 全バッチハッシュを連番順で取得
    pub async fn get_all_batch_hashes(&self) -> RouterResult<Vec<AuditBatchHash>> {
        let rows = sqlx::query_as::<_, AuditBatchHashRow>(
            "SELECT id, sequence_number, batch_start, batch_end, \
             record_count, hash, previous_hash \
             FROM audit_batch_hashes ORDER BY sequence_number ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get batch hashes: {}", e)))?;

        rows.into_iter()
            .map(AuditBatchHash::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// 最新バッチハッシュを取得
    pub async fn get_latest_batch_hash(&self) -> RouterResult<Option<AuditBatchHash>> {
        let row = sqlx::query_as::<_, AuditBatchHashRow>(
            "SELECT id, sequence_number, batch_start, batch_end, \
             record_count, hash, previous_hash \
             FROM audit_batch_hashes ORDER BY sequence_number DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get latest batch hash: {}", e)))?;

        match row {
            Some(r) => Ok(Some(AuditBatchHash::try_from(r)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_batch_hash_try_from_row() {
        let row = AuditBatchHashRow {
            id: 1,
            sequence_number: 42,
            batch_start: "2024-01-01T00:00:00+00:00".to_string(),
            batch_end: "2024-01-01T01:00:00+00:00".to_string(),
            record_count: 100,
            hash: "abcdef".to_string(),
            previous_hash: "000000".to_string(),
        };

        let batch = AuditBatchHash::try_from(row).unwrap();
        assert_eq!(batch.id, Some(1));
        assert_eq!(batch.sequence_number, 42);
        assert_eq!(batch.record_count, 100);
        assert_eq!(batch.hash, "abcdef");
        assert_eq!(batch.previous_hash, "000000");
    }

    #[test]
    fn test_audit_batch_hash_try_from_invalid_date() {
        let row = AuditBatchHashRow {
            id: 1,
            sequence_number: 1,
            batch_start: "not-a-date".to_string(),
            batch_end: "2024-01-01T00:00:00+00:00".to_string(),
            record_count: 0,
            hash: "x".to_string(),
            previous_hash: "y".to_string(),
        };

        let result = AuditBatchHash::try_from(row);
        assert!(result.is_err());
    }
}
