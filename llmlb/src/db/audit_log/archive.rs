//! 監査ログのアーカイブ処理
//!
//! arch-review [C3]: AuditLogStorage の肥大化を抑えるため、別DBへの移送
//! （archive_old_entries）とアーカイブDBのクエリ/検索、および移送に伴う
//! メインチェーンメタデータ再構築を submodule へ切り出した。メソッドは
//! `RequestHistoryStorage` 同様 `impl AuditLogStorage` として維持する。

use super::{
    build_extra_where, build_where_clause, sanitize_fts_query, AuditBatchHashRow, AuditLogRow,
    AuditLogStorage,
};
use crate::audit::hash_chain::{self, GENESIS_HASH};
use crate::audit::types::{AuditBatchHash, AuditLogEntry, AuditLogFilter};
use crate::common::error::{LbError, RouterResult};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

impl AuditLogStorage {
    /// 古いエントリをアーカイブDBに移動
    ///
    /// `retention_days`日より古いエントリをアーカイブDBにINSERTし、
    /// メインDBからDELETEする。関連するバッチハッシュもコピーする。
    pub async fn archive_old_entries(
        &self,
        retention_days: i64,
        archive_pool: &SqlitePool,
    ) -> RouterResult<i64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
        let cutoff_str = cutoff.to_rfc3339();

        // 部分バッチの移動でチェーンが壊れないよう、batch_end が cutoff より古いバッチのみ対象にする
        let archivable_batches = sqlx::query_as::<_, AuditBatchHashRow>(
            "SELECT id, sequence_number, batch_start, batch_end, \
             record_count, hash, previous_hash \
             FROM audit_batch_hashes WHERE batch_end < ? ORDER BY sequence_number ASC",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to fetch archivable batches: {}", e)))?;

        let mut rows = Vec::new();

        // バッチ未割当（主に移行データ）も保持期限対象ならアーカイブする
        let unbatched_sql = format!(
            "SELECT {} FROM audit_log_entries \
             WHERE timestamp < ? AND batch_id IS NULL ORDER BY timestamp ASC",
            super::AUDIT_LOG_SELECT_COLUMNS
        );
        let mut unbatched_rows = sqlx::query_as::<_, AuditLogRow>(&unbatched_sql)
            .bind(&cutoff_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                LbError::Database(format!("Failed to fetch old unbatched entries: {}", e))
            })?;
        rows.append(&mut unbatched_rows);

        if !archivable_batches.is_empty() {
            let mut batch_rows = sqlx::query_as::<_, AuditLogRow>(
                "SELECT e.id, e.timestamp, e.http_method, e.request_path, e.status_code, \
                 e.actor_type, e.actor_id, e.actor_username, e.api_key_owner_id, e.client_ip, \
                 e.duration_ms, e.input_tokens, e.output_tokens, e.total_tokens, \
                 e.model_name, e.endpoint_id, e.detail, e.batch_id, e.is_migrated \
                 FROM audit_log_entries e \
                 JOIN audit_batch_hashes b ON e.batch_id = b.id \
                 WHERE b.batch_end < ? \
                 ORDER BY e.timestamp ASC",
            )
            .bind(&cutoff_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                LbError::Database(format!("Failed to fetch old batched entries: {}", e))
            })?;
            rows.append(&mut batch_rows);
        }

        if rows.is_empty() {
            return Ok(0);
        }

        let count = rows.len() as i64;

        // バッチハッシュをアーカイブDBにコピー
        for bh in &archivable_batches {
            sqlx::query(
                "INSERT OR IGNORE INTO audit_batch_hashes \
                 (id, sequence_number, batch_start, batch_end, record_count, hash, previous_hash) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(bh.id)
            .bind(bh.sequence_number)
            .bind(&bh.batch_start)
            .bind(&bh.batch_end)
            .bind(bh.record_count)
            .bind(&bh.hash)
            .bind(&bh.previous_hash)
            .execute(archive_pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to archive batch hash: {}", e)))?;
        }

        // エントリをアーカイブDBにINSERT
        for row in &rows {
            sqlx::query(
                "INSERT OR IGNORE INTO audit_log_entries \
                 (id, timestamp, http_method, request_path, status_code, \
                  actor_type, actor_id, actor_username, api_key_owner_id, client_ip, \
                  duration_ms, input_tokens, output_tokens, total_tokens, \
                  model_name, endpoint_id, detail, batch_id, is_migrated) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.id)
            .bind(&row.timestamp)
            .bind(&row.http_method)
            .bind(&row.request_path)
            .bind(row.status_code)
            .bind(&row.actor_type)
            .bind(&row.actor_id)
            .bind(&row.actor_username)
            .bind(&row.api_key_owner_id)
            .bind(&row.client_ip)
            .bind(row.duration_ms)
            .bind(row.input_tokens)
            .bind(row.output_tokens)
            .bind(row.total_tokens)
            .bind(&row.model_name)
            .bind(&row.endpoint_id)
            .bind(&row.detail)
            .bind(row.batch_id)
            .bind(row.is_migrated)
            .execute(archive_pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to archive entry: {}", e)))?;
        }

        // メインDBから移送済みエントリを削除
        sqlx::query("DELETE FROM audit_log_entries WHERE timestamp < ? AND batch_id IS NULL")
            .bind(&cutoff_str)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                LbError::Database(format!(
                    "Failed to delete archived unbatched entries: {}",
                    e
                ))
            })?;

        // アーカイブ済みのバッチハッシュを主DBから削除
        if !archivable_batches.is_empty() {
            sqlx::query(
                "DELETE FROM audit_log_entries \
                 WHERE batch_id IN (SELECT id FROM audit_batch_hashes WHERE batch_end < ?)",
            )
            .bind(&cutoff_str)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                LbError::Database(format!("Failed to delete archived batched entries: {}", e))
            })?;

            sqlx::query("DELETE FROM audit_batch_hashes WHERE batch_end < ?")
                .bind(&cutoff_str)
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    LbError::Database(format!("Failed to delete archived batches: {}", e))
                })?;

            self.rebuild_main_chain_metadata().await?;
        }

        Ok(count)
    }

    async fn rebuild_main_chain_metadata(&self) -> RouterResult<()> {
        let rows = sqlx::query_as::<_, AuditBatchHashRow>(
            "SELECT id, sequence_number, batch_start, batch_end, \
             record_count, hash, previous_hash \
             FROM audit_batch_hashes ORDER BY sequence_number ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to load remaining batches: {}", e)))?;

        let batches = rows
            .into_iter()
            .map(AuditBatchHash::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let mut updates = Vec::with_capacity(batches.len());
        let mut previous_hash = GENESIS_HASH.to_string();
        for batch in &batches {
            let batch_id = batch.id.ok_or_else(|| {
                LbError::Database("Batch id is missing while rebuilding chain".to_string())
            })?;
            let entries = self.get_entries_for_batch(batch_id).await?;
            let sequence_number = batch.sequence_number;
            let record_count = entries.len() as i64;
            let hash = hash_chain::compute_batch_hash(
                &previous_hash,
                sequence_number,
                &batch.batch_start,
                &batch.batch_end,
                record_count,
                &entries,
            );
            updates.push((
                batch_id,
                sequence_number,
                record_count,
                hash.clone(),
                previous_hash,
            ));
            previous_hash = hash;
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            LbError::Database(format!("Failed to begin rebuild transaction: {}", e))
        })?;

        for (batch_id, sequence_number, record_count, hash, previous_hash) in updates {
            sqlx::query(
                "UPDATE audit_batch_hashes \
                 SET sequence_number = ?, record_count = ?, hash = ?, previous_hash = ? \
                 WHERE id = ?",
            )
            .bind(sequence_number)
            .bind(record_count)
            .bind(&hash)
            .bind(&previous_hash)
            .bind(batch_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| LbError::Database(format!("Failed to rebuild batch chain: {}", e)))?;
        }

        tx.commit().await.map_err(|e| {
            LbError::Database(format!("Failed to commit rebuild transaction: {}", e))
        })?;

        Ok(())
    }

    /// アーカイブDBからエントリを検索
    pub async fn query_archive(
        &self,
        filter: &AuditLogFilter,
        archive_pool: &SqlitePool,
    ) -> RouterResult<Vec<AuditLogEntry>> {
        let (where_clause, bind_values) = build_where_clause(filter);
        let page = filter.page.unwrap_or(1).max(1);
        let per_page = filter.per_page.unwrap_or(50).max(1);
        let offset = page.saturating_sub(1).saturating_mul(per_page);

        let sql = format!(
            "SELECT {} FROM audit_log_entries {} ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            super::AUDIT_LOG_SELECT_COLUMNS,
            where_clause
        );

        let mut query = sqlx::query_as::<_, AuditLogRow>(&sql);
        for val in &bind_values {
            query = query.bind(val);
        }
        query = query.bind(per_page).bind(offset);

        let rows = query
            .fetch_all(archive_pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to query archive: {}", e)))?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// アーカイブDBのエントリ数を取得
    pub async fn count_archive(
        &self,
        filter: &AuditLogFilter,
        archive_pool: &SqlitePool,
    ) -> RouterResult<i64> {
        let (where_clause, bind_values) = build_where_clause(filter);

        let sql = format!("SELECT COUNT(*) FROM audit_log_entries {}", where_clause);

        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        for val in &bind_values {
            query = query.bind(val);
        }

        let count = query
            .fetch_one(archive_pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to count archive: {}", e)))?;

        Ok(count)
    }

    /// アーカイブDBをFTS5全文検索
    pub async fn search_fts_archive(
        &self,
        search_query: &str,
        filter: &AuditLogFilter,
        archive_pool: &SqlitePool,
    ) -> RouterResult<Vec<AuditLogEntry>> {
        let sanitized = sanitize_fts_query(search_query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let (where_clause, bind_values) = build_where_clause(filter);
        let page = filter.page.unwrap_or(1).max(1);
        let per_page = filter.per_page.unwrap_or(50).max(1);
        let offset = page.saturating_sub(1).saturating_mul(per_page);

        let extra_where = build_extra_where(&where_clause);
        let sql = format!(
            "SELECT e.id, e.timestamp, e.http_method, e.request_path, e.status_code, \
             e.actor_type, e.actor_id, e.actor_username, e.api_key_owner_id, e.client_ip, \
             e.duration_ms, e.input_tokens, e.output_tokens, e.total_tokens, \
             e.model_name, e.endpoint_id, e.detail, e.batch_id, e.is_migrated \
             FROM audit_log_fts fts \
             JOIN audit_log_entries e ON fts.rowid = e.id \
             WHERE fts.audit_log_fts MATCH ? {} \
             ORDER BY e.timestamp DESC LIMIT ? OFFSET ?",
            extra_where
        );

        let mut query = sqlx::query_as::<_, AuditLogRow>(&sql).bind(&sanitized);
        for val in &bind_values {
            query = query.bind(val.as_str());
        }
        query = query.bind(per_page).bind(offset);

        let rows = query.fetch_all(archive_pool).await.map_err(|e| {
            LbError::Database(format!("Failed to search archive audit logs: {}", e))
        })?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// アーカイブDBのFTS検索件数
    pub async fn count_fts_archive(
        &self,
        search_query: &str,
        filter: &AuditLogFilter,
        archive_pool: &SqlitePool,
    ) -> RouterResult<i64> {
        let sanitized = sanitize_fts_query(search_query);
        if sanitized.is_empty() {
            return Ok(0);
        }

        let (where_clause, bind_values) = build_where_clause(filter);
        let extra_where = build_extra_where(&where_clause);
        let sql = format!(
            "SELECT COUNT(*) FROM audit_log_fts fts \
             JOIN audit_log_entries e ON fts.rowid = e.id \
             WHERE fts.audit_log_fts MATCH ? {}",
            extra_where
        );

        let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(&sanitized);
        for val in &bind_values {
            query = query.bind(val.as_str());
        }

        query
            .fetch_one(archive_pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to count archive FTS results: {}", e)))
    }
}

/// アーカイブDBプールを作成
///
/// アーカイブDBファイルが存在しない場合は自動作成し、
/// 必要なテーブル（audit_log_entries + audit_batch_hashes）を作成する。
pub async fn create_archive_pool(path: &str) -> RouterResult<SqlitePool> {
    let url = format!("sqlite:{}?mode=rwc", path);
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(|e| LbError::Database(format!("Failed to create archive pool: {}", e)))?;

    // WALモード設定
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to set WAL mode: {}", e)))?;

    // アーカイブDBにテーブルを作成
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_log_entries (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            http_method TEXT NOT NULL,
            request_path TEXT NOT NULL,
            status_code INTEGER NOT NULL,
            actor_type TEXT NOT NULL,
            actor_id TEXT,
            actor_username TEXT,
            api_key_owner_id TEXT,
            client_ip TEXT,
            duration_ms INTEGER,
            input_tokens INTEGER,
            output_tokens INTEGER,
            total_tokens INTEGER,
            model_name TEXT,
            endpoint_id TEXT,
            detail TEXT,
            batch_id INTEGER,
            is_migrated INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to create archive tables: {}", e)))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_batch_hashes (
            id INTEGER PRIMARY KEY,
            sequence_number INTEGER NOT NULL UNIQUE,
            batch_start TEXT NOT NULL,
            batch_end TEXT NOT NULL,
            record_count INTEGER NOT NULL,
            hash TEXT NOT NULL,
            previous_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(&pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to create archive batch table: {}", e)))?;

    // インデックス
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_archive_timestamp ON audit_log_entries(timestamp)")
        .execute(&pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to create archive index: {}", e)))?;

    sqlx::query(
        "CREATE VIRTUAL TABLE IF NOT EXISTS audit_log_fts USING fts5(
            request_path,
            actor_id,
            actor_username,
            client_ip,
            detail,
            content=audit_log_entries,
            content_rowid=id
        )",
    )
    .execute(&pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to create archive FTS table: {}", e)))?;

    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS audit_log_fts_insert AFTER INSERT ON audit_log_entries BEGIN
            INSERT INTO audit_log_fts(rowid, request_path, actor_id, actor_username, client_ip, detail)
            VALUES (new.id, new.request_path, new.actor_id, new.actor_username, new.client_ip, new.detail);
        END;",
    )
    .execute(&pool)
    .await
    .map_err(|e| {
        LbError::Database(format!(
            "Failed to create archive FTS insert trigger: {}",
            e
        ))
    })?;

    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS audit_log_fts_delete AFTER DELETE ON audit_log_entries BEGIN
            INSERT INTO audit_log_fts(audit_log_fts, rowid, request_path, actor_id, actor_username, client_ip, detail)
            VALUES ('delete', old.id, old.request_path, old.actor_id, old.actor_username, old.client_ip, old.detail);
        END;",
    )
    .execute(&pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to create archive FTS delete trigger: {}", e)))?;

    Ok(pool)
}
