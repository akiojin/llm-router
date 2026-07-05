//! 監査ログストレージ (SPEC-8301d106)

/// アーカイブ処理（別DBへの移送・検索）は archive submodule に分離（arch-review [C3]）
mod archive;
mod statistics;
pub use statistics::{
    DailyTokenStatistics, ModelTokenStatistics, MonthlyTokenStatistics, TokenStatistics,
};

use crate::audit::types::{ActorType, AuditBatchHash, AuditLogEntry, AuditLogFilter};
use crate::common::error::{LbError, RouterResult};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

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

/// 監査ログのDB CRUD操作
#[derive(Clone)]
pub struct AuditLogStorage {
    pool: SqlitePool,
}

/// sqlx::FromRow用の行構造体
#[derive(Debug, sqlx::FromRow)]
struct AuditLogRow {
    id: i64,
    timestamp: String,
    http_method: String,
    request_path: String,
    status_code: i64,
    actor_type: String,
    actor_id: Option<String>,
    actor_username: Option<String>,
    api_key_owner_id: Option<String>,
    client_ip: Option<String>,
    duration_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    model_name: Option<String>,
    endpoint_id: Option<String>,
    detail: Option<String>,
    batch_id: Option<i64>,
    is_migrated: i64,
}

/// `AuditLogRow` に対応する SELECT カラムリスト（順序は構造体フィールドと一致）。
///
/// カラムを増減する際はこの定数と [`AuditLogRow`] の双方を更新すること。
/// 従来は同一リストを 6 箇所の SQL 文へ逐語コピーしていた（arch-review [L4]）。
pub(super) const AUDIT_LOG_SELECT_COLUMNS: &str = "id, timestamp, http_method, request_path, \
     status_code, actor_type, actor_id, actor_username, api_key_owner_id, client_ip, \
     duration_ms, input_tokens, output_tokens, total_tokens, model_name, endpoint_id, \
     detail, batch_id, is_migrated";

/// sqlx::FromRow用の行構造体（バッチハッシュ）
#[derive(Debug, sqlx::FromRow)]
struct AuditBatchHashRow {
    id: i64,
    sequence_number: i64,
    batch_start: String,
    batch_end: String,
    record_count: i64,
    hash: String,
    previous_hash: String,
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

impl TryFrom<AuditLogRow> for AuditLogEntry {
    type Error = LbError;

    fn try_from(row: AuditLogRow) -> Result<Self, Self::Error> {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&row.timestamp)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| LbError::Database(format!("Failed to parse timestamp: {}", e)))?;

        let status_code = u16::try_from(row.status_code)
            .map_err(|e| LbError::Database(format!("Invalid status_code: {}", e)))?;

        Ok(AuditLogEntry {
            id: Some(row.id),
            timestamp,
            http_method: row.http_method,
            request_path: row.request_path,
            status_code,
            actor_type: ActorType::from_str(&row.actor_type),
            actor_id: row.actor_id,
            actor_username: row.actor_username,
            api_key_owner_id: row.api_key_owner_id,
            client_ip: row.client_ip,
            duration_ms: row.duration_ms,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            total_tokens: row.total_tokens,
            model_name: row.model_name,
            endpoint_id: row.endpoint_id,
            detail: row.detail,
            batch_id: row.batch_id,
            is_migrated: row.is_migrated != 0,
        })
    }
}

impl AuditLogStorage {
    /// 新しいAuditLogStorageを作成
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 監査ログを一括挿入
    pub async fn insert_batch(&self, entries: &[AuditLogEntry]) -> RouterResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| LbError::Database(format!("Failed to begin transaction: {}", e)))?;

        for entry in entries {
            let timestamp_str = entry.timestamp.to_rfc3339();
            let actor_type_str = entry.actor_type.as_str();
            let status_code = entry.status_code as i64;
            let is_migrated: i64 = if entry.is_migrated { 1 } else { 0 };

            sqlx::query(
                r#"INSERT INTO audit_log_entries (
                    timestamp, http_method, request_path, status_code,
                    actor_type, actor_id, actor_username, api_key_owner_id,
                    client_ip, duration_ms, input_tokens, output_tokens,
                    total_tokens, model_name, endpoint_id, detail,
                    batch_id, is_migrated
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&timestamp_str)
            .bind(&entry.http_method)
            .bind(&entry.request_path)
            .bind(status_code)
            .bind(actor_type_str)
            .bind(&entry.actor_id)
            .bind(&entry.actor_username)
            .bind(&entry.api_key_owner_id)
            .bind(&entry.client_ip)
            .bind(entry.duration_ms)
            .bind(entry.input_tokens)
            .bind(entry.output_tokens)
            .bind(entry.total_tokens)
            .bind(&entry.model_name)
            .bind(&entry.endpoint_id)
            .bind(&entry.detail)
            .bind(entry.batch_id)
            .bind(is_migrated)
            .execute(&mut *tx)
            .await
            .map_err(|e| LbError::Database(format!("Failed to insert audit log: {}", e)))?;
        }

        tx.commit()
            .await
            .map_err(|e| LbError::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    /// フィルタ条件に基づいて監査ログを検索
    pub async fn query(&self, filter: &AuditLogFilter) -> RouterResult<Vec<AuditLogEntry>> {
        let (where_clause, bind_values) = build_where_clause(filter);
        let page = filter.page.unwrap_or(1).max(1);
        let per_page = filter.per_page.unwrap_or(50).max(1);
        let offset = page.saturating_sub(1).saturating_mul(per_page);

        let sql = format!(
            "SELECT {} FROM audit_log_entries {} ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            AUDIT_LOG_SELECT_COLUMNS, where_clause
        );

        let mut query = sqlx::query_as::<_, AuditLogRow>(&sql);
        for val in &bind_values {
            query = query.bind(val.as_str());
        }
        query = query.bind(per_page).bind(offset);

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to query audit logs: {}", e)))?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// フィルタ条件に基づいてレコード数を取得
    pub async fn count(&self, filter: &AuditLogFilter) -> RouterResult<i64> {
        let (where_clause, bind_values) = build_where_clause(filter);
        let sql = format!(
            "SELECT COUNT(*) as cnt FROM audit_log_entries {}",
            where_clause
        );

        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        for val in &bind_values {
            query = query.bind(val.as_str());
        }

        query
            .fetch_one(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to count audit logs: {}", e)))
    }

    /// IDで監査ログを取得
    pub async fn get_by_id(&self, id: i64) -> RouterResult<Option<AuditLogEntry>> {
        let sql = format!(
            "SELECT {} FROM audit_log_entries WHERE id = ?",
            AUDIT_LOG_SELECT_COLUMNS
        );
        let row = sqlx::query_as::<_, AuditLogRow>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to get audit log by id: {}", e)))?;

        match row {
            Some(r) => Ok(Some(AuditLogEntry::try_from(r)?)),
            None => Ok(None),
        }
    }

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

    /// バッチ内エントリを取得
    pub async fn get_entries_for_batch(&self, batch_id: i64) -> RouterResult<Vec<AuditLogEntry>> {
        let sql = format!(
            "SELECT {} FROM audit_log_entries WHERE batch_id = ? ORDER BY timestamp ASC",
            AUDIT_LOG_SELECT_COLUMNS
        );
        let rows = sqlx::query_as::<_, AuditLogRow>(&sql)
            .bind(batch_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to get entries for batch: {}", e)))?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// エントリのbatch_idを更新
    pub async fn update_entries_batch_id(
        &self,
        entry_ids: &[i64],
        batch_id: i64,
    ) -> RouterResult<()> {
        if entry_ids.is_empty() {
            return Ok(());
        }

        let placeholders: Vec<&str> = entry_ids.iter().map(|_| "?").collect();
        let sql = format!(
            "UPDATE audit_log_entries SET batch_id = ? WHERE id IN ({})",
            placeholders.join(", ")
        );

        let mut query = sqlx::query(&sql).bind(batch_id);
        for id in entry_ids {
            query = query.bind(id);
        }

        query
            .execute(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to update batch_id: {}", e)))?;

        Ok(())
    }

    /// バッチハッシュ挿入とエントリの batch_id 更新を単一トランザクションで実行
    ///
    /// `insert_batch_hash`（バッチ行のINSERT）と `update_entries_batch_id`
    /// （対象エントリのbatch_id更新）を同一の sqlx トランザクションに包む。
    /// batch_id 更新が失敗した場合はバッチ行の挿入もロールバックされるため、
    /// 「record_count=N のバッチ行は残るのに N 件のエントリは batch_id=NULL のまま」
    /// という状態が生じない。これがないと未割当エントリが再取得されて別バッチへ
    /// 二重に組み込まれ、ハッシュチェーン検証が恒久的に不整合となりうる。
    ///
    /// ハッシュ計算自体は呼び出し側で行い、ここではトランザクション境界のみを担う。
    pub async fn insert_batch_hash_with_entries(
        &self,
        batch: &AuditBatchHash,
        entry_ids: &[i64],
    ) -> RouterResult<i64> {
        let batch_start_str = batch.batch_start.to_rfc3339();
        let batch_end_str = batch.batch_end.to_rfc3339();

        let mut tx = self.pool.begin().await.map_err(|e| {
            LbError::Database(format!("Failed to begin batch hash transaction: {}", e))
        })?;

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
        .execute(&mut *tx)
        .await
        .map_err(|e| LbError::Database(format!("Failed to insert batch hash: {}", e)))?;

        let batch_id = result.last_insert_rowid();

        if !entry_ids.is_empty() {
            let placeholders: Vec<&str> = entry_ids.iter().map(|_| "?").collect();
            let sql = format!(
                "UPDATE audit_log_entries SET batch_id = ? WHERE id IN ({})",
                placeholders.join(", ")
            );

            let mut query = sqlx::query(&sql).bind(batch_id);
            for id in entry_ids {
                query = query.bind(id);
            }

            query
                .execute(&mut *tx)
                .await
                .map_err(|e| LbError::Database(format!("Failed to update batch_id: {}", e)))?;
        }

        tx.commit().await.map_err(|e| {
            LbError::Database(format!("Failed to commit batch hash transaction: {}", e))
        })?;

        Ok(batch_id)
    }

    /// FTS5全文検索で監査ログを検索
    pub async fn search_fts(
        &self,
        search_query: &str,
        filter: &AuditLogFilter,
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

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to search audit logs: {}", e)))?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// FTS5検索結果のカウント
    pub async fn count_fts(
        &self,
        search_query: &str,
        filter: &AuditLogFilter,
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
            .fetch_one(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to count FTS results: {}", e)))
    }

    /// バッチ未割当のエントリを取得（batch_id IS NULL AND is_migrated = 0）
    pub async fn get_unbatched_entries(&self) -> RouterResult<Vec<AuditLogEntry>> {
        let sql = format!(
            "SELECT {} FROM audit_log_entries \
             WHERE batch_id IS NULL AND is_migrated = 0 ORDER BY timestamp ASC",
            AUDIT_LOG_SELECT_COLUMNS
        );
        let rows = sqlx::query_as::<_, AuditLogRow>(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to get unbatched entries: {}", e)))?;

        rows.into_iter()
            .map(AuditLogEntry::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    /// HTTPメソッド別のエントリ数を取得
    pub async fn count_by_method(&self) -> RouterResult<Vec<(String, i64)>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT http_method, COUNT(*) as cnt FROM audit_log_entries GROUP BY http_method ORDER BY cnt DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to count by method: {}", e)))?;
        Ok(rows)
    }

    /// アクター種別のエントリ数を取得
    pub async fn count_by_actor_type(&self) -> RouterResult<Vec<(String, i64)>> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT actor_type, COUNT(*) as cnt FROM audit_log_entries GROUP BY actor_type ORDER BY cnt DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to count by actor type: {}", e)))?;
        Ok(rows)
    }
}

/// フィルタからWHERE句とバインド値を構築
fn build_where_clause(filter: &AuditLogFilter) -> (String, Vec<String>) {
    let mut conditions: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(ref actor_type) = filter.actor_type {
        conditions.push("actor_type = ?".to_string());
        bind_values.push(actor_type.clone());
    }

    if let Some(ref actor_id) = filter.actor_id {
        conditions.push("actor_id = ?".to_string());
        bind_values.push(actor_id.clone());
    }

    if let Some(ref http_method) = filter.http_method {
        conditions.push("http_method = ?".to_string());
        bind_values.push(http_method.clone());
    }

    if let Some(ref request_path) = filter.request_path {
        conditions.push("request_path = ?".to_string());
        bind_values.push(request_path.clone());
    }

    if let Some(status_code) = filter.status_code {
        conditions.push("status_code = ?".to_string());
        bind_values.push(status_code.to_string());
    }

    if let Some(ref time_from) = filter.time_from {
        conditions.push("timestamp >= ?".to_string());
        bind_values.push(time_from.to_rfc3339());
    }

    if let Some(ref time_to) = filter.time_to {
        conditions.push("timestamp <= ?".to_string());
        bind_values.push(time_to.to_rfc3339());
    }

    if let Some(ref client_ip) = filter.client_ip {
        conditions.push("client_ip LIKE ?".to_string());
        bind_values.push(format!("{}%", client_ip));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    (where_clause, bind_values)
}

/// WHERE句をFTS JOIN用のAND条件に変換
fn build_extra_where(where_clause: &str) -> String {
    if where_clause.is_empty() {
        String::new()
    } else {
        // "WHERE x = ? AND y = ?" -> "AND x = ? AND y = ?"
        // build_where_clauseでカラム名にテーブルプレフィックスがないので
        // JOINクエリ用にeプレフィックスを付与
        // arch-review [L5]: バイト index [6..] は prefix 変更時に panic し得るため strip_prefix で堅牢化
        let conditions = where_clause.strip_prefix("WHERE ").unwrap_or(where_clause);
        let prefixed = conditions
            .replace("actor_type", "e.actor_type")
            .replace("actor_id", "e.actor_id")
            .replace("http_method", "e.http_method")
            .replace("request_path", "e.request_path")
            .replace("status_code", "e.status_code")
            .replace("timestamp", "e.timestamp")
            .replace("client_ip", "e.client_ip");
        format!("AND {}", prefixed)
    }
}

/// FTS5クエリの特殊文字をサニタイズ
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| {
            let clean = word.replace('"', "");
            if clean.is_empty() {
                return String::new();
            }
            format!("\"{}\"", clean)
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::types::ActorType;
    use chrono::Utc;

    async fn create_test_pool() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    fn make_entry(method: &str, path: &str, status: u16, actor: ActorType) -> AuditLogEntry {
        AuditLogEntry {
            id: None,
            timestamp: Utc::now(),
            http_method: method.to_string(),
            request_path: path.to_string(),
            status_code: status,
            actor_type: actor,
            actor_id: Some("test-actor".to_string()),
            actor_username: Some("tester".to_string()),
            api_key_owner_id: None,
            client_ip: Some("127.0.0.1".to_string()),
            duration_ms: Some(42),
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            model_name: Some("test-model".to_string()),
            endpoint_id: Some("ep-1".to_string()),
            detail: None,
            batch_id: None,
            is_migrated: false,
        }
    }

    #[tokio::test]
    async fn test_insert_batch_and_query() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
        ];

        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter::default();
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 2);

        // ORDER BY timestamp DESC なので最新が先
        assert!(results[0].id.is_some());
        assert!(results[1].id.is_some());
    }

    #[tokio::test]
    async fn test_query_with_actor_type_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("GET", "/v1/models", 401, ActorType::Anonymous),
        ];

        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_type, ActorType::User);
    }

    #[tokio::test]
    async fn test_query_with_pagination() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entries = Vec::new();
        for i in 0..5 {
            let mut entry = make_entry("GET", "/v1/models", 200, ActorType::User);
            entry.duration_ms = Some(i);
            entries.push(entry);
        }

        storage.insert_batch(&entries).await.unwrap();

        // ページ1: 2件
        let filter = AuditLogFilter {
            page: Some(1),
            per_page: Some(2),
            ..Default::default()
        };
        let page1 = storage.query(&filter).await.unwrap();
        assert_eq!(page1.len(), 2);

        // ページ2: 2件
        let filter = AuditLogFilter {
            page: Some(2),
            per_page: Some(2),
            ..Default::default()
        };
        let page2 = storage.query(&filter).await.unwrap();
        assert_eq!(page2.len(), 2);

        // ページ3: 1件
        let filter = AuditLogFilter {
            page: Some(3),
            per_page: Some(2),
            ..Default::default()
        };
        let page3 = storage.query(&filter).await.unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[tokio::test]
    async fn test_count() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("GET", "/v1/models", 401, ActorType::Anonymous),
        ];

        storage.insert_batch(&entries).await.unwrap();

        let total = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(total, 3);

        let user_count = storage
            .count(&AuditLogFilter {
                actor_type: Some("user".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(user_count, 1);
    }

    #[tokio::test]
    async fn test_get_by_id() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![make_entry(
            "POST",
            "/v1/chat/completions",
            200,
            ActorType::ApiKey,
        )];
        storage.insert_batch(&entries).await.unwrap();

        let all = storage.query(&AuditLogFilter::default()).await.unwrap();
        let id = all[0].id.unwrap();

        let found = storage.get_by_id(id).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.http_method, "POST");
        assert_eq!(found.request_path, "/v1/chat/completions");
        assert_eq!(found.status_code, 200);
        assert_eq!(found.actor_type, ActorType::ApiKey);

        // 存在しないID
        let not_found = storage.get_by_id(99999).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_insert_batch_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        // 空配列の場合はエラーにならない
        storage.insert_batch(&[]).await.unwrap();

        let total = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(total, 0);
    }

    fn make_token_entry(
        model: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        input: i64,
        output: i64,
        total: i64,
    ) -> AuditLogEntry {
        AuditLogEntry {
            id: None,
            timestamp,
            http_method: "POST".to_string(),
            request_path: "/v1/chat/completions".to_string(),
            status_code: 200,
            actor_type: ActorType::ApiKey,
            actor_id: Some("test-key".to_string()),
            actor_username: None,
            api_key_owner_id: None,
            client_ip: Some("127.0.0.1".to_string()),
            duration_ms: Some(100),
            input_tokens: Some(input),
            output_tokens: Some(output),
            total_tokens: Some(total),
            model_name: Some(model.to_string()),
            endpoint_id: Some("ep-1".to_string()),
            detail: None,
            batch_id: None,
            is_migrated: false,
        }
    }

    fn make_token_entry_without_total(
        model: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        input: i64,
        output: i64,
    ) -> AuditLogEntry {
        let mut entry = make_token_entry(model, timestamp, input, output, 0);
        entry.total_tokens = None;
        entry
    }

    #[tokio::test]
    async fn test_get_token_statistics() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry("model-b", now, 200, 100, 300),
            make_token_entry("model-a", now, 50, 25, 75),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 350);
        assert_eq!(stats.total_output_tokens, 175);
        assert_eq!(stats.total_tokens, 525);
    }

    #[tokio::test]
    async fn test_get_token_statistics_infers_total_when_total_tokens_null() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry_without_total("model-a", now, 70, 30),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 170);
        assert_eq!(stats.total_output_tokens, 80);
        assert_eq!(stats.total_tokens, 250);
    }

    #[tokio::test]
    async fn test_get_token_statistics_by_model() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry("model-b", now, 200, 100, 300),
            make_token_entry("model-a", now, 50, 25, 75),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_token_statistics_by_model().await.unwrap();
        assert_eq!(stats.len(), 2);

        // ORDER BY total_tokens DESC なので model-b が先
        assert_eq!(stats[0].model_name, "model-b");
        assert_eq!(stats[0].total_input_tokens, 200);
        assert_eq!(stats[0].total_output_tokens, 100);
        assert_eq!(stats[0].total_tokens, 300);

        assert_eq!(stats[1].model_name, "model-a");
        assert_eq!(stats[1].total_input_tokens, 150);
        assert_eq!(stats[1].total_output_tokens, 75);
        assert_eq!(stats[1].total_tokens, 225);
    }

    #[tokio::test]
    async fn test_get_daily_token_statistics() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry("model-b", now, 200, 100, 300),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_daily_token_statistics(7).await.unwrap();
        // 同日のエントリなので1日分
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 300);
        assert_eq!(stats[0].total_output_tokens, 150);
        assert_eq!(stats[0].total_tokens, 450);
    }

    #[tokio::test]
    async fn test_get_daily_token_statistics_infers_total_when_total_tokens_null() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry_without_total("model-b", now, 20, 10),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_daily_token_statistics(7).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 120);
        assert_eq!(stats[0].total_output_tokens, 60);
        assert_eq!(stats[0].total_tokens, 180);
        assert_eq!(stats[0].request_count, 2);
    }

    #[tokio::test]
    async fn test_get_monthly_token_statistics() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 100, 50, 150),
            make_token_entry("model-b", now, 200, 100, 300),
            make_token_entry("model-a", now, 50, 25, 75),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_monthly_token_statistics(3).await.unwrap();
        // 同月のエントリなので1月分
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 350);
        assert_eq!(stats[0].total_output_tokens, 175);
        assert_eq!(stats[0].total_tokens, 525);
    }

    #[tokio::test]
    async fn test_get_monthly_token_statistics_infers_total_when_total_tokens_null() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let entries = vec![
            make_token_entry("model-a", now, 40, 20, 60),
            make_token_entry_without_total("model-b", now, 10, 5),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let stats = storage.get_monthly_token_statistics(3).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 50);
        assert_eq!(stats[0].total_output_tokens, 25);
        assert_eq!(stats[0].total_tokens, 75);
        assert_eq!(stats[0].request_count, 2);
    }

    #[tokio::test]
    async fn test_insert_and_get_batch_hash() {
        use crate::audit::types::AuditBatchHash;

        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: now,
            batch_end: now,
            record_count: 10,
            hash: "abc123".to_string(),
            previous_hash: "0".repeat(64),
        };

        let id = storage.insert_batch_hash(&batch).await.unwrap();
        assert!(id > 0);

        let all = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, Some(id));
        assert_eq!(all[0].sequence_number, 1);
        assert_eq!(all[0].record_count, 10);
        assert_eq!(all[0].hash, "abc123");
        assert_eq!(all[0].previous_hash, "0".repeat(64));

        // 2つ目のバッチを追加
        let batch2 = AuditBatchHash {
            id: None,
            sequence_number: 2,
            batch_start: now,
            batch_end: now,
            record_count: 5,
            hash: "def456".to_string(),
            previous_hash: "abc123".to_string(),
        };
        storage.insert_batch_hash(&batch2).await.unwrap();

        let all = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(all.len(), 2);
        // ORDER BY sequence_number ASC
        assert_eq!(all[0].sequence_number, 1);
        assert_eq!(all[1].sequence_number, 2);
    }

    #[tokio::test]
    async fn test_get_latest_batch_hash() {
        use crate::audit::types::AuditBatchHash;

        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        // 空の場合はNone
        let latest = storage.get_latest_batch_hash().await.unwrap();
        assert!(latest.is_none());

        let now = Utc::now();
        let batch1 = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: now,
            batch_end: now,
            record_count: 10,
            hash: "hash1".to_string(),
            previous_hash: "0".repeat(64),
        };
        storage.insert_batch_hash(&batch1).await.unwrap();

        let batch2 = AuditBatchHash {
            id: None,
            sequence_number: 2,
            batch_start: now,
            batch_end: now,
            record_count: 5,
            hash: "hash2".to_string(),
            previous_hash: "hash1".to_string(),
        };
        storage.insert_batch_hash(&batch2).await.unwrap();

        let latest = storage.get_latest_batch_hash().await.unwrap();
        assert!(latest.is_some());
        let latest = latest.unwrap();
        assert_eq!(latest.sequence_number, 2);
        assert_eq!(latest.hash, "hash2");
    }

    #[tokio::test]
    async fn test_get_entries_for_batch() {
        use crate::audit::types::AuditBatchHash;

        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        // エントリを挿入
        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("GET", "/health", 200, ActorType::Anonymous),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // バッチハッシュを挿入
        let now = Utc::now();
        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: now,
            batch_end: now,
            record_count: 2,
            hash: "batchhash".to_string(),
            previous_hash: "0".repeat(64),
        };
        let batch_id = storage.insert_batch_hash(&batch).await.unwrap();

        // 最初の2エントリのbatch_idを更新
        let all = storage.query(&AuditLogFilter::default()).await.unwrap();
        let entry_ids: Vec<i64> = all.iter().take(2).filter_map(|e| e.id).collect();
        storage
            .update_entries_batch_id(&entry_ids, batch_id)
            .await
            .unwrap();

        // バッチ内エントリを取得
        let batch_entries = storage.get_entries_for_batch(batch_id).await.unwrap();
        assert_eq!(batch_entries.len(), 2);
        for entry in &batch_entries {
            assert_eq!(entry.batch_id, Some(batch_id));
        }

        // 存在しないバッチID
        let empty = storage.get_entries_for_batch(99999).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_search_fts_by_path() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("GET", "/v1/embeddings", 200, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // "chat"を含むパスを検索
        let results = storage
            .search_fts("chat", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_path, "/v1/chat/completions");
    }

    #[tokio::test]
    async fn test_search_fts_by_actor() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.actor_id = Some("alice".to_string());
        entry1.actor_username = Some("Alice Smith".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.actor_id = Some("bob-key".to_string());
        entry2.actor_username = None;

        storage.insert_batch(&[entry1, entry2]).await.unwrap();

        // actor_usernameで検索
        let results = storage
            .search_fts("Alice", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_username, Some("Alice Smith".to_string()));

        // actor_idで検索
        let results = storage
            .search_fts("bob", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_id, Some("bob-key".to_string()));
    }

    #[tokio::test]
    async fn test_search_fts_with_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/models/list", 200, ActorType::ApiKey),
            make_entry("GET", "/v1/models/detail", 401, ActorType::Anonymous),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // "models"で検索（全3件ヒット）
        let results = storage
            .search_fts("models", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);

        // "models"で検索 + actor_typeフィルタ
        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            ..Default::default()
        };
        let results = storage.search_fts("models", &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_type, ActorType::User);
    }

    #[tokio::test]
    async fn test_search_fts_no_results() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![make_entry("GET", "/v1/models", 200, ActorType::User)];
        storage.insert_batch(&entries).await.unwrap();

        let results = storage
            .search_fts("nonexistent", &AuditLogFilter::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_fts_basic() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/api/users", 200, ActorType::User);
        entry1.actor_username = Some("admin".to_string());
        let entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        let mut entry3 = make_entry("DELETE", "/api/endpoints", 200, ActorType::User);
        entry3.actor_username = Some("admin".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        let filter = AuditLogFilter::default();

        // "users" でマッチするのは1件
        let results = storage.search_fts("users", &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_path, "/api/users");

        // "completions" でマッチするのは1件
        let results = storage.search_fts("completions", &filter).await.unwrap();
        assert_eq!(results.len(), 1);

        // "api" でマッチするのは2件（/api/users, /api/endpoints）
        let results = storage.search_fts("api", &filter).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_count_fts() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/models/list", 200, ActorType::ApiKey),
            make_entry("GET", "/v1/chat/completions", 200, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // "models" → 2件
        let count = storage
            .count_fts("models", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 2);

        // "models" + actor_typeフィルタ → 1件
        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            ..Default::default()
        };
        let count = storage.count_fts("models", &filter).await.unwrap();
        assert_eq!(count, 1);

        // 空クエリ → 0
        let count = storage
            .count_fts("", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_sanitize_fts_query() {
        // 通常の単語
        assert_eq!(sanitize_fts_query("hello"), "\"hello\"");

        // 複数単語
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\" \"world\"");

        // 特殊文字のエスケープ
        assert_eq!(sanitize_fts_query("he\"llo"), "\"hello\"");

        // 空文字列
        assert_eq!(sanitize_fts_query(""), "");

        // 空白のみ
        assert_eq!(sanitize_fts_query("   "), "");
    }

    #[tokio::test]
    async fn test_request_history_migration() {
        // request_historyからaudit_log_entriesへのデータ移行SQLを検証
        let pool = crate::db::test_utils::test_db_pool().await;

        // request_historyにテストデータを挿入（全マイグレーション後のフルスキーマ）
        sqlx::query(
            r#"INSERT INTO request_history
                (id, timestamp, request_type, model, endpoint_id, endpoint_name, endpoint_ip,
                 request_body, duration_ms, status, completed_at,
                 input_tokens, output_tokens, total_tokens)
            VALUES
                ('req-1', '2024-01-15T10:00:00+00:00', 'chat', 'llama-3', 'ep-1', 'machine-1', '127.0.0.1',
                 '{}', 500, 'success', '2024-01-15T10:00:01+00:00',
                 100, 50, 150),
                ('req-2', '2024-01-15T11:00:00+00:00', 'chat', 'gpt-4', 'ep-2', 'machine-2', '127.0.0.1',
                 '{}', 1000, 'success', '2024-01-15T11:00:01+00:00',
                 200, 100, 300),
                ('req-3', '2024-01-15T12:00:00+00:00', 'chat', 'llama-3', 'ep-1', 'machine-1', '127.0.0.1',
                 '{}', 200, 'error', '2024-01-15T12:00:01+00:00',
                 50, 0, 50)"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // 017_audit_log.sqlのマイグレーションSQLを手動実行してデータ移行を検証
        sqlx::query(
            r#"INSERT INTO audit_log_entries (
                timestamp, http_method, request_path, status_code,
                actor_type, actor_id, duration_ms,
                input_tokens, output_tokens, total_tokens,
                model_name, endpoint_id, is_migrated
            )
            SELECT
                rh.timestamp,
                'POST',
                '/v1/chat/completions',
                CASE WHEN rh.error_message IS NULL THEN 200 ELSE 500 END,
                'api_key',
                'unknown',
                rh.duration_ms,
                rh.input_tokens,
                rh.output_tokens,
                rh.total_tokens,
                rh.model,
                rh.endpoint_id,
                1
            FROM request_history rh"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let storage = AuditLogStorage::new(pool.clone());

        // 移行されたエントリを確認
        let filter = AuditLogFilter::default();
        let entries = storage.query(&filter).await.unwrap();
        assert_eq!(
            entries.len(),
            3,
            "All 3 request_history records should be migrated"
        );

        // is_migrated=1で移行されていることを確認
        for entry in &entries {
            assert!(
                entry.is_migrated,
                "Migrated entries should have is_migrated=true"
            );
        }

        // 移行データの内容を確認
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM audit_log_entries WHERE is_migrated = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 3);

        // エラーありレコードはstatus_code=500に変換されていることを確認
        let error_entry: (i64,) = sqlx::query_as(
            "SELECT status_code FROM audit_log_entries WHERE model_name = 'llama-3' \
             AND duration_ms = 200",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // req-3は status='error' だが error_message=NULL なので 200 になる
        assert_eq!(error_entry.0, 200);

        // トークン統計が移行データから正しく集計されること
        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 350);
        assert_eq!(stats.total_output_tokens, 150);
        assert_eq!(stats.total_tokens, 500);

        // モデル別統計
        let by_model = storage.get_token_statistics_by_model().await.unwrap();
        assert_eq!(by_model.len(), 2); // llama-3, gpt-4
    }

    #[tokio::test]
    async fn test_archive_old_entries() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        // アーカイブ用インメモリDBを作成
        let archive_pool = super::create_archive_pool(":memory:").await.unwrap();

        // 古いエントリ（100日前）と新しいエントリ（1日前）を挿入
        let now = chrono::Utc::now();
        let old_time = now - chrono::Duration::days(100);
        let new_time = now - chrono::Duration::days(1);

        let entries = vec![
            AuditLogEntry {
                timestamp: old_time,
                ..make_entry("GET", "/api/old-1", 200, ActorType::User)
            },
            AuditLogEntry {
                timestamp: old_time - chrono::Duration::hours(1),
                ..make_entry("POST", "/api/old-2", 200, ActorType::User)
            },
            AuditLogEntry {
                timestamp: new_time,
                ..make_entry("GET", "/api/new-1", 200, ActorType::User)
            },
        ];
        storage.insert_batch(&entries).await.unwrap();

        // メインDBに3件あることを確認
        let main_count = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(main_count, 3);

        // アーカイブ実行（90日保持）
        let archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(archived, 2, "2 old entries should be archived");

        // メインDBに1件のみ残る
        let main_count = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(main_count, 1);

        // アーカイブDBに2件移動
        let archive_storage = AuditLogStorage::new(archive_pool.clone());
        let archive_count = archive_storage
            .count(&AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(archive_count, 2);

        // アーカイブDBからクエリ可能
        let archive_entries = storage
            .query_archive(&AuditLogFilter::default(), &archive_pool)
            .await
            .unwrap();
        assert_eq!(archive_entries.len(), 2);
    }

    #[tokio::test]
    async fn test_archive_no_old_entries() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        let archive_pool = super::create_archive_pool(":memory:").await.unwrap();

        // 新しいエントリのみ
        let entries = vec![make_entry("GET", "/api/new", 200, ActorType::User)];
        storage.insert_batch(&entries).await.unwrap();

        // アーカイブ対象なし
        let archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(archived, 0);

        // メインDBに1件残る
        let main_count = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(main_count, 1);
    }

    #[tokio::test]
    async fn test_archive_with_batch_hashes() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        let archive_pool = super::create_archive_pool(":memory:").await.unwrap();

        let now = chrono::Utc::now();
        let old_time = now - chrono::Duration::days(100);

        // バッチハッシュを作成
        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: old_time - chrono::Duration::hours(1),
            batch_end: old_time,
            record_count: 1,
            hash: "abc123".to_string(),
            previous_hash: "0".repeat(64),
        };
        let batch_id = storage.insert_batch_hash(&batch).await.unwrap();

        // 古いエントリをバッチに関連付けて挿入
        let mut entry = make_entry("GET", "/api/old", 200, ActorType::User);
        entry.timestamp = old_time;
        storage.insert_batch(&[entry]).await.unwrap();

        // エントリのbatch_idを更新
        sqlx::query("UPDATE audit_log_entries SET batch_id = ? WHERE request_path = '/api/old'")
            .bind(batch_id)
            .execute(&pool)
            .await
            .unwrap();

        // アーカイブ実行
        let archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(archived, 1);

        // アーカイブDBにバッチハッシュもコピーされている
        let archive_batch: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM audit_batch_hashes WHERE id = ?")
                .bind(batch_id)
                .fetch_optional(&archive_pool)
                .await
                .unwrap();
        assert!(
            archive_batch.is_some(),
            "Batch hash should be copied to archive"
        );
    }

    #[tokio::test]
    async fn test_archive_keeps_main_hash_chain_verifiable() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let archive_pool = super::create_archive_pool(":memory:").await.unwrap();

        let now = chrono::Utc::now();
        let old_time = now - chrono::Duration::days(100);
        let new_time = now - chrono::Duration::days(1);

        let mut old_entry = make_entry("GET", "/api/old", 200, ActorType::User);
        old_entry.timestamp = old_time;
        let mut new_entry = make_entry("GET", "/api/new", 200, ActorType::User);
        new_entry.timestamp = new_time;
        storage
            .insert_batch(&[old_entry.clone(), new_entry.clone()])
            .await
            .unwrap();

        let old_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/old'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let new_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/new'")
                .fetch_one(&pool)
                .await
                .unwrap();

        old_entry.id = Some(old_id.0);
        new_entry.id = Some(new_id.0);

        let old_hash = crate::audit::hash_chain::compute_batch_hash(
            crate::audit::hash_chain::GENESIS_HASH,
            1,
            &old_time,
            &old_time,
            1,
            &[old_entry.clone()],
        );
        let old_batch_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: 1,
                batch_start: old_time,
                batch_end: old_time,
                record_count: 1,
                hash: old_hash.clone(),
                previous_hash: crate::audit::hash_chain::GENESIS_HASH.to_string(),
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[old_id.0], old_batch_id)
            .await
            .unwrap();

        let new_hash = crate::audit::hash_chain::compute_batch_hash(
            &old_hash,
            2,
            &new_time,
            &new_time,
            1,
            &[new_entry.clone()],
        );
        let new_batch_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: 2,
                batch_start: new_time,
                batch_end: new_time,
                record_count: 1,
                hash: new_hash,
                previous_hash: old_hash,
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[new_id.0], new_batch_id)
            .await
            .unwrap();

        let before = crate::audit::hash_chain::verify_chain(&storage)
            .await
            .unwrap();
        assert!(before.valid);
        assert_eq!(before.batches_checked, 2);

        let archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(archived, 1);

        let after = crate::audit::hash_chain::verify_chain(&storage)
            .await
            .unwrap();
        assert!(
            after.valid,
            "main DB hash chain must stay valid after archive"
        );
        assert_eq!(after.batches_checked, 1);

        let remaining_batches = storage.get_all_batch_hashes().await.unwrap();
        assert_eq!(remaining_batches.len(), 1);
        assert_eq!(remaining_batches[0].sequence_number, 2);
        assert_eq!(
            remaining_batches[0].previous_hash,
            crate::audit::hash_chain::GENESIS_HASH
        );
    }

    #[tokio::test]
    async fn test_archive_multiple_runs_preserve_archive_hash_rows() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());
        let archive_pool = super::create_archive_pool(":memory:").await.unwrap();

        let now = chrono::Utc::now();
        let old_time_1 = now - chrono::Duration::days(200);
        let old_time_2 = now - chrono::Duration::days(120);
        let keep_time = now - chrono::Duration::days(1);

        let mut old_entry_1 = make_entry("GET", "/api/old-1", 200, ActorType::User);
        old_entry_1.timestamp = old_time_1;
        let mut old_entry_2 = make_entry("GET", "/api/old-2", 200, ActorType::User);
        old_entry_2.timestamp = old_time_2;
        let mut keep_entry = make_entry("GET", "/api/keep", 200, ActorType::User);
        keep_entry.timestamp = keep_time;
        storage
            .insert_batch(&[old_entry_1, old_entry_2, keep_entry])
            .await
            .unwrap();

        let old_1_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/old-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let old_2_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/old-2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let keep_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/keep'")
                .fetch_one(&pool)
                .await
                .unwrap();

        let batch_1_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: 1,
                batch_start: old_time_1,
                batch_end: old_time_1,
                record_count: 1,
                hash: "hash-1".to_string(),
                previous_hash: crate::audit::hash_chain::GENESIS_HASH.to_string(),
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[old_1_id.0], batch_1_id)
            .await
            .unwrap();

        let batch_2_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: 2,
                batch_start: old_time_2,
                batch_end: old_time_2,
                record_count: 1,
                hash: "hash-2".to_string(),
                previous_hash: "hash-1".to_string(),
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[old_2_id.0], batch_2_id)
            .await
            .unwrap();

        let batch_3_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: 3,
                batch_start: keep_time,
                batch_end: keep_time,
                record_count: 1,
                hash: "hash-3".to_string(),
                previous_hash: "hash-2".to_string(),
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[keep_id.0], batch_3_id)
            .await
            .unwrap();

        let first_archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(first_archived, 2);

        let second_old_time = now - chrono::Duration::days(95);
        let mut second_old_entry = make_entry("GET", "/api/old-3", 200, ActorType::User);
        second_old_entry.timestamp = second_old_time;
        storage.insert_batch(&[second_old_entry]).await.unwrap();

        let old_3_id: (i64,) =
            sqlx::query_as("SELECT id FROM audit_log_entries WHERE request_path = '/api/old-3'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let latest_batch = storage.get_latest_batch_hash().await.unwrap().unwrap();
        let batch_4_id = storage
            .insert_batch_hash(&crate::audit::types::AuditBatchHash {
                id: None,
                sequence_number: latest_batch.sequence_number + 1,
                batch_start: second_old_time,
                batch_end: second_old_time,
                record_count: 1,
                hash: "hash-4".to_string(),
                previous_hash: latest_batch.hash,
            })
            .await
            .unwrap();
        storage
            .update_entries_batch_id(&[old_3_id.0], batch_4_id)
            .await
            .unwrap();

        let second_archived = storage
            .archive_old_entries(90, &archive_pool)
            .await
            .unwrap();
        assert_eq!(second_archived, 1);

        let missing_hash_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) \
             FROM audit_log_entries e \
             LEFT JOIN audit_batch_hashes b ON e.batch_id = b.id \
             WHERE e.batch_id IS NOT NULL AND b.id IS NULL",
        )
        .fetch_one(&archive_pool)
        .await
        .unwrap();
        assert_eq!(
            missing_hash_count.0, 0,
            "every archived batched entry must have a matching archive batch hash row"
        );
    }

    // --- 追加テスト ---

    #[tokio::test]
    async fn test_get_token_statistics_empty_db() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 0);
        assert_eq!(stats.total_output_tokens, 0);
        assert_eq!(stats.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_get_token_statistics_by_model_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let stats = storage.get_token_statistics_by_model().await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_count_by_method() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("DELETE", "/api/endpoints/1", 200, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let counts = storage.count_by_method().await.unwrap();
        assert!(!counts.is_empty());
        // GET=2 が最多
        assert_eq!(counts[0].0, "GET");
        assert_eq!(counts[0].1, 2);
    }

    #[tokio::test]
    async fn test_count_by_actor_type() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let counts = storage.count_by_actor_type().await.unwrap();
        assert!(!counts.is_empty());
        assert_eq!(counts[0].0, "user");
        assert_eq!(counts[0].1, 2);
    }

    #[tokio::test]
    async fn test_get_unbatched_entries() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool.clone());

        let entries = vec![
            make_entry("GET", "/api/a", 200, ActorType::User),
            make_entry("POST", "/api/b", 200, ActorType::ApiKey),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // All entries are unbatched initially
        let unbatched = storage.get_unbatched_entries().await.unwrap();
        assert_eq!(unbatched.len(), 2);

        // Assign one to a batch
        let batch = AuditBatchHash {
            id: None,
            sequence_number: 1,
            batch_start: Utc::now(),
            batch_end: Utc::now(),
            record_count: 1,
            hash: "test".to_string(),
            previous_hash: "0".repeat(64),
        };
        let batch_id = storage.insert_batch_hash(&batch).await.unwrap();

        let all = storage.query(&AuditLogFilter::default()).await.unwrap();
        let first_id = all[0].id.unwrap();
        storage
            .update_entries_batch_id(&[first_id], batch_id)
            .await
            .unwrap();

        let unbatched = storage.get_unbatched_entries().await.unwrap();
        assert_eq!(unbatched.len(), 1);
    }

    #[tokio::test]
    async fn test_update_entries_batch_id_empty_ids() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        // Empty list should be a no-op
        storage.update_entries_batch_id(&[], 1).await.unwrap();
    }

    #[tokio::test]
    async fn test_query_with_http_method_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
            make_entry("DELETE", "/api/endpoints/1", 204, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            http_method: Some("POST".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].http_method, "POST");
    }

    #[tokio::test]
    async fn test_query_with_status_code_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("GET", "/v1/models", 401, ActorType::Anonymous),
            make_entry("POST", "/v1/chat/completions", 500, ActorType::ApiKey),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            status_code: Some(401),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status_code, 401);
    }

    #[tokio::test]
    async fn test_query_with_request_path_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/v1/models", 200, ActorType::User),
            make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            request_path: Some("/v1/models".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_path, "/v1/models");
    }

    #[test]
    fn test_build_where_clause_empty_filter() {
        let filter = AuditLogFilter::default();
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.is_empty());
        assert!(values.is_empty());
    }

    #[test]
    fn test_build_where_clause_multiple_filters() {
        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            http_method: Some("GET".to_string()),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.starts_with("WHERE"));
        assert!(clause.contains("AND"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_build_extra_where_empty() {
        let result = build_extra_where("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_extra_where_with_conditions() {
        let result = build_extra_where("WHERE actor_type = ? AND http_method = ?");
        assert!(result.starts_with("AND"));
        assert!(result.contains("e.actor_type"));
        assert!(result.contains("e.http_method"));
    }

    #[test]
    fn test_sanitize_fts_query_special_chars() {
        // Double quotes are stripped
        assert_eq!(sanitize_fts_query("te\"st"), "\"test\"");
        // Multiple words with quotes
        assert_eq!(sanitize_fts_query("he\"llo wor\"ld"), "\"hello\" \"world\"");
    }

    #[tokio::test]
    async fn test_count_fts_empty_query() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let count = storage
            .count_fts("", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    // =====================================================================
    // 追加テスト: AuditLogRow -> AuditLogEntry conversion
    // =====================================================================

    #[test]
    fn test_audit_log_entry_try_from_row() {
        let row = AuditLogRow {
            id: 1,
            timestamp: "2024-06-15T12:00:00+00:00".to_string(),
            http_method: "GET".to_string(),
            request_path: "/v1/models".to_string(),
            status_code: 200,
            actor_type: "user".to_string(),
            actor_id: Some("user-1".to_string()),
            actor_username: Some("admin".to_string()),
            api_key_owner_id: None,
            client_ip: Some("127.0.0.1".to_string()),
            duration_ms: Some(42),
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            model_name: Some("gpt-4".to_string()),
            endpoint_id: Some("ep-1".to_string()),
            detail: Some("test detail".to_string()),
            batch_id: None,
            is_migrated: 0,
        };

        let entry = AuditLogEntry::try_from(row).unwrap();
        assert_eq!(entry.id, Some(1));
        assert_eq!(entry.http_method, "GET");
        assert_eq!(entry.request_path, "/v1/models");
        assert_eq!(entry.status_code, 200);
        assert_eq!(entry.actor_type, ActorType::User);
        assert_eq!(entry.actor_id, Some("user-1".to_string()));
        assert_eq!(entry.actor_username, Some("admin".to_string()));
        assert_eq!(entry.client_ip, Some("127.0.0.1".to_string()));
        assert_eq!(entry.duration_ms, Some(42));
        assert_eq!(entry.input_tokens, Some(100));
        assert_eq!(entry.output_tokens, Some(50));
        assert_eq!(entry.total_tokens, Some(150));
        assert_eq!(entry.model_name, Some("gpt-4".to_string()));
        assert!(!entry.is_migrated);
    }

    #[test]
    fn test_audit_log_entry_try_from_row_migrated() {
        let row = AuditLogRow {
            id: 2,
            timestamp: "2024-06-15T12:00:00+00:00".to_string(),
            http_method: "POST".to_string(),
            request_path: "/v1/chat/completions".to_string(),
            status_code: 200,
            actor_type: "api_key".to_string(),
            actor_id: None,
            actor_username: None,
            api_key_owner_id: None,
            client_ip: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            model_name: None,
            endpoint_id: None,
            detail: None,
            batch_id: Some(5),
            is_migrated: 1,
        };

        let entry = AuditLogEntry::try_from(row).unwrap();
        assert!(entry.is_migrated);
        assert_eq!(entry.batch_id, Some(5));
        assert!(entry.actor_id.is_none());
        assert!(entry.client_ip.is_none());
        assert!(entry.duration_ms.is_none());
    }

    #[test]
    fn test_audit_log_entry_try_from_invalid_timestamp() {
        let row = AuditLogRow {
            id: 1,
            timestamp: "not-a-date".to_string(),
            http_method: "GET".to_string(),
            request_path: "/".to_string(),
            status_code: 200,
            actor_type: "user".to_string(),
            actor_id: None,
            actor_username: None,
            api_key_owner_id: None,
            client_ip: None,
            duration_ms: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            model_name: None,
            endpoint_id: None,
            detail: None,
            batch_id: None,
            is_migrated: 0,
        };

        let result = AuditLogEntry::try_from(row);
        assert!(result.is_err());
    }

    // =====================================================================
    // 追加テスト: AuditBatchHashRow -> AuditBatchHash conversion
    // =====================================================================

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

    // =====================================================================
    // 追加テスト: TokenStatistics
    // =====================================================================

    #[test]
    fn test_token_statistics_serialize() {
        let stats = TokenStatistics {
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_input_tokens"], 100);
        assert_eq!(json["total_output_tokens"], 50);
        assert_eq!(json["total_tokens"], 150);
    }

    // =====================================================================
    // 追加テスト: ModelTokenStatistics
    // =====================================================================

    #[test]
    fn test_model_token_statistics_serialize() {
        let stats = ModelTokenStatistics {
            model_name: "llama".to_string(),
            total_input_tokens: 200,
            total_output_tokens: 100,
            total_tokens: 300,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["model_name"], "llama");
        assert_eq!(json["total_tokens"], 300);
    }

    // =====================================================================
    // 追加テスト: DailyTokenStatistics
    // =====================================================================

    #[test]
    fn test_daily_token_statistics_serialize() {
        let stats = DailyTokenStatistics {
            date: "2024-06-15".to_string(),
            total_input_tokens: 500,
            total_output_tokens: 250,
            total_tokens: 750,
            request_count: 10,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["date"], "2024-06-15");
        assert_eq!(json["request_count"], 10);
    }

    // =====================================================================
    // 追加テスト: MonthlyTokenStatistics
    // =====================================================================

    #[test]
    fn test_monthly_token_statistics_serialize() {
        let stats = MonthlyTokenStatistics {
            month: "2024-06".to_string(),
            total_input_tokens: 5000,
            total_output_tokens: 2500,
            total_tokens: 7500,
            request_count: 100,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["month"], "2024-06");
        assert_eq!(json["request_count"], 100);
    }

    // =====================================================================
    // 追加テスト: build_where_clause single filter
    // =====================================================================

    #[test]
    fn test_build_where_clause_actor_id_only() {
        let filter = AuditLogFilter {
            actor_id: Some("user-123".to_string()),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.starts_with("WHERE"));
        assert!(clause.contains("actor_id"));
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "user-123");
    }

    #[test]
    fn test_build_where_clause_request_path_only() {
        let filter = AuditLogFilter {
            request_path: Some("/v1/models".to_string()),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.contains("request_path"));
        assert_eq!(values[0], "/v1/models");
    }

    #[test]
    fn test_build_where_clause_status_code_only() {
        let filter = AuditLogFilter {
            status_code: Some(401),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.contains("status_code"));
        assert_eq!(values[0], "401");
    }

    #[test]
    fn test_build_where_clause_time_range() {
        let now = Utc::now();
        let filter = AuditLogFilter {
            time_from: Some(now - chrono::Duration::hours(1)),
            time_to: Some(now),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.contains("timestamp >= ?"));
        assert!(clause.contains("timestamp <= ?"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_build_where_clause_all_filters() {
        let now = Utc::now();
        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            actor_id: Some("id-1".to_string()),
            http_method: Some("GET".to_string()),
            request_path: Some("/api".to_string()),
            status_code: Some(200),
            time_from: Some(now),
            time_to: Some(now),
            ..Default::default()
        };
        let (clause, values) = build_where_clause(&filter);
        assert!(clause.starts_with("WHERE"));
        assert_eq!(values.len(), 7);
    }

    // =====================================================================
    // 追加テスト: build_extra_where prefix replacement
    // =====================================================================

    #[test]
    fn test_build_extra_where_replaces_all_column_names() {
        let where_clause = "WHERE actor_type = ? AND actor_id = ? AND http_method = ? AND request_path = ? AND status_code = ? AND timestamp >= ?";
        let result = build_extra_where(where_clause);
        assert!(result.starts_with("AND"));
        assert!(result.contains("e.actor_type"));
        assert!(result.contains("e.actor_id"));
        assert!(result.contains("e.http_method"));
        assert!(result.contains("e.request_path"));
        assert!(result.contains("e.status_code"));
        assert!(result.contains("e.timestamp"));
    }

    // =====================================================================
    // 追加テスト: sanitize_fts_query edge cases
    // =====================================================================

    #[test]
    fn test_sanitize_fts_query_only_quotes() {
        // A word that is only quotes should be filtered out
        assert_eq!(sanitize_fts_query("\"\"\""), "");
    }

    #[test]
    fn test_sanitize_fts_query_tabs_and_newlines() {
        // Whitespace characters are split by split_whitespace
        assert_eq!(
            sanitize_fts_query("hello\tworld\nnew"),
            "\"hello\" \"world\" \"new\""
        );
    }

    // =====================================================================
    // 追加テスト: DB操作 - query with time range filter
    // =====================================================================

    #[tokio::test]
    async fn test_query_with_time_range_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let now = Utc::now();
        let old_entry = AuditLogEntry {
            timestamp: now - chrono::Duration::hours(5),
            ..make_entry("GET", "/api/old", 200, ActorType::User)
        };
        let new_entry = AuditLogEntry {
            timestamp: now,
            ..make_entry("GET", "/api/new", 200, ActorType::User)
        };
        storage.insert_batch(&[old_entry, new_entry]).await.unwrap();

        // Filter: last 2 hours
        let filter = AuditLogFilter {
            time_from: Some(now - chrono::Duration::hours(2)),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_path, "/api/new");
    }

    // =====================================================================
    // 追加テスト: DB操作 - query with actor_id filter
    // =====================================================================

    #[tokio::test]
    async fn test_query_with_actor_id_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/api/a", 200, ActorType::User);
        entry1.actor_id = Some("alice".to_string());
        let mut entry2 = make_entry("GET", "/api/b", 200, ActorType::User);
        entry2.actor_id = Some("bob".to_string());
        storage.insert_batch(&[entry1, entry2]).await.unwrap();

        let filter = AuditLogFilter {
            actor_id: Some("alice".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_id, Some("alice".to_string()));
    }

    // =====================================================================
    // 追加テスト: DB操作 - count with pagination does not affect count
    // =====================================================================

    #[tokio::test]
    async fn test_count_ignores_pagination() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/a", 200, ActorType::User),
            make_entry("GET", "/b", 200, ActorType::User),
            make_entry("GET", "/c", 200, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            page: Some(1),
            per_page: Some(1),
            ..Default::default()
        };
        let count = storage.count(&filter).await.unwrap();
        assert_eq!(count, 3); // count ignores pagination
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_by_id returns correct fields
    // =====================================================================

    #[tokio::test]
    async fn test_get_by_id_with_all_fields() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry = make_entry("DELETE", "/api/users/1", 204, ActorType::User);
        entry.actor_id = Some("admin-user".to_string());
        entry.actor_username = Some("Admin".to_string());
        entry.detail = Some("Deleted user 1".to_string());
        entry.input_tokens = None;
        entry.output_tokens = None;
        entry.total_tokens = None;
        entry.model_name = None;
        storage.insert_batch(&[entry]).await.unwrap();

        let all = storage.query(&AuditLogFilter::default()).await.unwrap();
        let id = all[0].id.unwrap();

        let found = storage.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(found.http_method, "DELETE");
        assert_eq!(found.request_path, "/api/users/1");
        assert_eq!(found.status_code, 204);
        assert_eq!(found.actor_id, Some("admin-user".to_string()));
        assert_eq!(found.actor_username, Some("Admin".to_string()));
        assert_eq!(found.detail, Some("Deleted user 1".to_string()));
    }

    // =====================================================================
    // 追加テスト: DB操作 - search_fts empty query
    // =====================================================================

    #[tokio::test]
    async fn test_search_fts_empty_query() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![make_entry("GET", "/v1/models", 200, ActorType::User)];
        storage.insert_batch(&entries).await.unwrap();

        let results = storage
            .search_fts("", &AuditLogFilter::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - search_fts whitespace-only query
    // =====================================================================

    #[tokio::test]
    async fn test_search_fts_whitespace_query() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![make_entry("GET", "/v1/models", 200, ActorType::User)];
        storage.insert_batch(&entries).await.unwrap();

        let results = storage
            .search_fts("   ", &AuditLogFilter::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - count_by_method empty db
    // =====================================================================

    #[tokio::test]
    async fn test_count_by_method_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let counts = storage.count_by_method().await.unwrap();
        assert!(counts.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - count_by_actor_type empty db
    // =====================================================================

    #[tokio::test]
    async fn test_count_by_actor_type_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let counts = storage.count_by_actor_type().await.unwrap();
        assert!(counts.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_unbatched_entries empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_unbatched_entries_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let unbatched = storage.get_unbatched_entries().await.unwrap();
        assert!(unbatched.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_all_batch_hashes empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_all_batch_hashes_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let hashes = storage.get_all_batch_hashes().await.unwrap();
        assert!(hashes.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - daily token statistics empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_daily_token_statistics_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let stats = storage.get_daily_token_statistics(30).await.unwrap();
        assert!(stats.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - monthly token statistics empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_monthly_token_statistics_empty() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let stats = storage.get_monthly_token_statistics(12).await.unwrap();
        assert!(stats.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - insert_batch multiple times
    // =====================================================================

    #[tokio::test]
    async fn test_insert_batch_multiple_batches() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let batch1 = vec![make_entry("GET", "/a", 200, ActorType::User)];
        let batch2 = vec![
            make_entry("POST", "/b", 201, ActorType::ApiKey),
            make_entry("DELETE", "/c", 204, ActorType::User),
        ];

        storage.insert_batch(&batch1).await.unwrap();
        storage.insert_batch(&batch2).await.unwrap();

        let total = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(total, 3);
    }

    // =====================================================================
    // 追加テスト: DB操作 - query with combined filters
    // =====================================================================

    #[tokio::test]
    async fn test_query_combined_actor_type_and_method() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/a", 200, ActorType::User),
            make_entry("POST", "/b", 200, ActorType::User),
            make_entry("GET", "/c", 200, ActorType::ApiKey),
        ];
        storage.insert_batch(&entries).await.unwrap();

        let filter = AuditLogFilter {
            actor_type: Some("user".to_string()),
            http_method: Some("GET".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].request_path, "/a");
    }

    // =====================================================================
    // 追加テスト: DB操作 - query pagination boundary
    // =====================================================================

    #[tokio::test]
    async fn test_query_pagination_beyond_last_page() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let entries = vec![
            make_entry("GET", "/a", 200, ActorType::User),
            make_entry("GET", "/b", 200, ActorType::User),
        ];
        storage.insert_batch(&entries).await.unwrap();

        // Page far beyond data
        let filter = AuditLogFilter {
            page: Some(100),
            per_page: Some(10),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert!(results.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - token statistics with mixed null
    // =====================================================================

    #[tokio::test]
    async fn test_get_token_statistics_all_null_tokens() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry = make_entry("POST", "/v1/chat", 200, ActorType::ApiKey);
        entry.input_tokens = None;
        entry.output_tokens = None;
        entry.total_tokens = None;
        storage.insert_batch(&[entry]).await.unwrap();

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 0);
        assert_eq!(stats.total_output_tokens, 0);
        assert_eq!(stats.total_tokens, 0);
    }

    // =====================================================================
    // 追加テスト: DB操作 - model stats excludes null model_name
    // =====================================================================

    #[tokio::test]
    async fn test_get_token_statistics_by_model_excludes_null_model() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry = make_entry("POST", "/v1/chat", 200, ActorType::ApiKey);
        entry.model_name = None;
        entry.input_tokens = Some(100);
        entry.output_tokens = Some(50);
        entry.total_tokens = Some(150);
        storage.insert_batch(&[entry]).await.unwrap();

        let stats = storage.get_token_statistics_by_model().await.unwrap();
        // model_name IS NULL entries are excluded by WHERE clause
        assert!(stats.is_empty());
    }

    // =====================================================================
    // 追加テスト: client_ip LIKE prefix filtering
    // =====================================================================

    #[tokio::test]
    async fn test_query_with_client_ip_prefix_filter_ipv4() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("192.168.1.1".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("192.168.2.1".to_string());

        let mut entry3 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry3.client_ip = Some("10.0.0.1".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        // "192.168.1" で前方一致フィルタ
        let filter = AuditLogFilter {
            client_ip: Some("192.168.1".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].client_ip, Some("192.168.1.1".to_string()));

        // "192.168" で前方一致フィルタ（2件マッチ）
        let filter = AuditLogFilter {
            client_ip: Some("192.168".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 2);

        // "10.0" で前方一致フィルタ
        let filter = AuditLogFilter {
            client_ip: Some("10.0".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].client_ip, Some("10.0.0.1".to_string()));
    }

    #[tokio::test]
    async fn test_query_with_client_ip_prefix_filter_ipv6() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("2001:db8::1".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("2001:db9::1".to_string());

        let mut entry3 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry3.client_ip = Some("::1".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        // "2001:db8" で前方一致フィルタ
        let filter = AuditLogFilter {
            client_ip: Some("2001:db8".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].client_ip, Some("2001:db8::1".to_string()));

        // "::" で前方一致フィルタ（"::1" のみマッチ）
        let filter = AuditLogFilter {
            client_ip: Some("::".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].client_ip, Some("::1".to_string()));
    }

    #[tokio::test]
    async fn test_query_with_client_ip_and_other_filters() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("192.168.1.1".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("192.168.1.2".to_string());

        let mut entry3 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry3.client_ip = Some("10.0.0.1".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        // client_ip + actor_type フィルタ
        let filter = AuditLogFilter {
            client_ip: Some("192.168.1".to_string()),
            actor_type: Some("user".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_type, ActorType::User);
        assert_eq!(results[0].client_ip, Some("192.168.1.1".to_string()));

        // client_ip + http_method フィルタ
        let filter = AuditLogFilter {
            client_ip: Some("192.168.1".to_string()),
            http_method: Some("POST".to_string()),
            ..Default::default()
        };
        let results = storage.query(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].http_method, "POST");
        assert_eq!(results[0].client_ip, Some("192.168.1.2".to_string()));
    }

    #[tokio::test]
    async fn test_count_with_client_ip_filter() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("192.168.1.1".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("192.168.2.1".to_string());

        let mut entry3 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry3.client_ip = Some("10.0.0.1".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        let total = storage.count(&AuditLogFilter::default()).await.unwrap();
        assert_eq!(total, 3);

        let count = storage
            .count(&AuditLogFilter {
                client_ip: Some("192.168".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(count, 2);

        let count = storage
            .count(&AuditLogFilter {
                client_ip: Some("10.0".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(count, 1);

        let count = storage
            .count(&AuditLogFilter {
                client_ip: Some("1.1.1".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_search_fts_with_client_ip() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("192.168.1.10".to_string());
        entry1.actor_username = Some("alice".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("10.0.0.5".to_string());

        let mut entry3 = make_entry("DELETE", "/api/endpoints", 200, ActorType::User);
        entry3.client_ip = Some("192.168.2.20".to_string());
        entry3.actor_username = Some("bob".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        let filter = AuditLogFilter::default();

        // "192.168" で2件マッチ（192.168.1.10と192.168.2.20）
        let results = storage.search_fts("192.168", &filter).await.unwrap();
        assert_eq!(results.len(), 2);

        // "10.0.0" で1件マッチ（10.0.0.5）
        let results = storage.search_fts("10.0.0", &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].client_ip.as_deref(), Some("10.0.0.5"));

        // "alice" で1件マッチ（actor_usernameで検索）
        let results = storage.search_fts("alice", &filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].actor_username.as_deref(), Some("alice"));
    }

    #[tokio::test]
    async fn test_count_fts_with_client_ip() {
        let pool = create_test_pool().await;
        let storage = AuditLogStorage::new(pool);

        let mut entry1 = make_entry("GET", "/v1/models", 200, ActorType::User);
        entry1.client_ip = Some("203.0.113.1".to_string());

        let mut entry2 = make_entry("POST", "/v1/chat/completions", 200, ActorType::ApiKey);
        entry2.client_ip = Some("203.0.113.2".to_string());

        let mut entry3 = make_entry("GET", "/api/endpoints", 200, ActorType::User);
        entry3.client_ip = Some("198.51.100.1".to_string());

        storage
            .insert_batch(&[entry1, entry2, entry3])
            .await
            .unwrap();

        // "203.0.113" で2件
        let count = storage
            .count_fts("203.0.113", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 2);

        // "198.51.100" で1件
        let count = storage
            .count_fts("198.51.100", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 1);

        // "999" で0件
        let count = storage
            .count_fts("999", &AuditLogFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
