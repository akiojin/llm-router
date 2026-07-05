//! 監査ログのFTS5全文検索。
//!
//! クエリのサニタイズ、WHERE 句の FTS JOIN 用 AND 変換、MATCH+JOIN 検索と件数集計。

use super::{build_where_clause, AuditLogRow, AuditLogStorage};
use crate::audit::types::{AuditLogEntry, AuditLogFilter};
use crate::common::error::{LbError, RouterResult};

impl AuditLogStorage {
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
}

/// WHERE句をFTS JOIN用のAND条件に変換
pub(crate) fn build_extra_where(where_clause: &str) -> String {
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
pub(crate) fn sanitize_fts_query(query: &str) -> String {
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
