//! リクエスト/レスポンス履歴のストレージ層
//!
//! SQLiteベースでリクエスト履歴を永続化（load balancer.dbと統合）

use crate::common::{
    error::{LbError, RouterResult},
    protocol::{RecordStatus, RequestResponseRecord, RequestType},
};
use crate::config::get_env_with_fallback_parse;
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;
use std::env;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const LEGACY_DATA_DIR_ENV: &str = "LLMLB_DATA_DIR";
const DEFAULT_DATA_DIR: &str = ".llmlb";
const LEGACY_REQUEST_HISTORY_FILE: &str = "request_history.json";
const REQUEST_HISTORY_RETENTION_DAYS_ENV: &str = "LLMLB_REQUEST_HISTORY_RETENTION_DAYS";
const LEGACY_REQUEST_HISTORY_RETENTION_DAYS_ENV: &str = "REQUEST_HISTORY_RETENTION_DAYS";
const REQUEST_HISTORY_CLEANUP_INTERVAL_ENV: &str = "LLMLB_REQUEST_HISTORY_CLEANUP_INTERVAL_SECS";
const LEGACY_REQUEST_HISTORY_CLEANUP_INTERVAL_ENV: &str = "REQUEST_HISTORY_CLEANUP_INTERVAL_SECS";

/// リクエスト履歴ストレージ（SQLite版）
#[derive(Clone)]
pub struct RequestHistoryStorage {
    pool: SqlitePool,
}

impl RequestHistoryStorage {
    /// 新しいストレージインスタンスを作成
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// IDでレコードを取得
    pub async fn get_record_by_id(&self, id: Uuid) -> RouterResult<Option<RequestResponseRecord>> {
        let row = sqlx::query_as::<_, RequestHistoryRow>(
            "SELECT * FROM request_history WHERE id = ? LIMIT 1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to load record: {}", e)))?;

        match row {
            Some(row) => Ok(Some(row.try_into()?)),
            None => Ok(None),
        }
    }

    /// レコードを保存
    pub async fn save_record(&self, record: &RequestResponseRecord) -> RouterResult<()> {
        self.insert_record(record, false).await?;
        Ok(())
    }

    /// 旧JSON履歴ファイルをSQLiteへインポート（存在すれば）
    pub async fn import_legacy_json_if_present(&self) -> RouterResult<usize> {
        let json_path = legacy_request_history_path()?;
        if !json_path.exists() {
            return Ok(0);
        }

        let contents = std::fs::read_to_string(&json_path).map_err(|e| {
            LbError::Internal(format!("Failed to read legacy request history: {}", e))
        })?;

        let records = parse_legacy_records(&contents)?;
        if records.is_empty() {
            tracing::info!(
                "Legacy request history file is empty: {}",
                json_path.display()
            );
        }

        let mut imported = 0usize;
        for record in &records {
            let inserted = self.insert_record(record, true).await?;
            imported += inserted as usize;
        }

        let migrated_path = legacy_migrated_path(&json_path);
        if let Err(err) = std::fs::rename(&json_path, &migrated_path) {
            tracing::warn!(
                "Failed to rename legacy request history to {}: {}",
                migrated_path.display(),
                err
            );
        } else {
            tracing::info!(
                "Legacy request history migrated: {} -> {}",
                json_path.display(),
                migrated_path.display()
            );
        }

        Ok(imported)
    }

    async fn insert_record(
        &self,
        record: &RequestResponseRecord,
        ignore_conflicts: bool,
    ) -> RouterResult<u64> {
        let id = record.id.to_string();
        let timestamp = record.timestamp.to_rfc3339();
        let request_type = format!("{:?}", record.request_type);
        let endpoint_id_str = record.endpoint_id.to_string();
        let endpoint_ip_str = record.endpoint_ip.to_string();
        let client_ip = record.client_ip.map(|ip| ip.to_string());
        let request_body = record.request_body.to_string();
        let response_body = record.response_body.as_ref().map(|v| v.to_string());
        let duration_ms = record.duration_ms as i64;
        let (status, error_message) = match &record.status {
            RecordStatus::Success => ("success".to_string(), None),
            RecordStatus::Error { message } => ("error".to_string(), Some(message.clone())),
        };
        let completed_at = record.completed_at.to_rfc3339();

        let input_tokens = record.input_tokens.map(|v| v as i64);
        let output_tokens = record.output_tokens.map(|v| v as i64);
        let total_tokens = record.total_tokens.map(|v| v as i64);

        let api_key_id = record.api_key_id.map(|id| id.to_string());

        let insert_sql = if ignore_conflicts {
            r#"
            INSERT OR IGNORE INTO request_history (
                id, timestamp, request_type, model, endpoint_id, endpoint_name,
                endpoint_ip, client_ip, request_body, response_body, duration_ms,
                status, error_message, completed_at, input_tokens, output_tokens, total_tokens,
                api_key_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        } else {
            r#"
            INSERT INTO request_history (
                id, timestamp, request_type, model, endpoint_id, endpoint_name,
                endpoint_ip, client_ip, request_body, response_body, duration_ms,
                status, error_message, completed_at, input_tokens, output_tokens, total_tokens,
                api_key_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        };

        let result = sqlx::query(insert_sql)
            .bind(&id)
            .bind(&timestamp)
            .bind(&request_type)
            .bind(&record.model)
            .bind(&endpoint_id_str)
            .bind(&record.endpoint_name)
            .bind(&endpoint_ip_str)
            .bind(&client_ip)
            .bind(&request_body)
            .bind(&response_body)
            .bind(duration_ms)
            .bind(&status)
            .bind(&error_message)
            .bind(&completed_at)
            .bind(input_tokens)
            .bind(output_tokens)
            .bind(total_tokens)
            .bind(&api_key_id)
            .execute(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to save record: {}", e)))?;

        Ok(result.rows_affected())
    }

    /// すべてのレコードを読み込み（タイムスタンプ降順）
    pub async fn load_records(&self) -> RouterResult<Vec<RequestResponseRecord>> {
        let rows = sqlx::query_as::<_, RequestHistoryRow>(
            "SELECT * FROM request_history ORDER BY timestamp DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to load records: {}", e)))?;

        rows.into_iter().map(|row| row.try_into()).collect()
    }

    /// 指定期間より古いレコードを削除
    pub async fn cleanup_old_records(&self, max_age: Duration) -> RouterResult<()> {
        let cutoff = (Utc::now() - max_age).to_rfc3339();

        sqlx::query("DELETE FROM request_history WHERE timestamp < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| LbError::Database(format!("Failed to cleanup records: {}", e)))?;

        Ok(())
    }

    /// 直近N分のリクエスト履歴を分単位で集計して返す（起動時seeding用）
    pub async fn get_recent_history_by_minute(
        &self,
        minutes: i64,
    ) -> RouterResult<Vec<MinuteHistoryPoint>> {
        let cutoff = (Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339();

        let rows = sqlx::query_as::<_, MinuteHistoryRow>(
            r#"
            SELECT
                strftime('%Y-%m-%dT%H:%M:00Z', completed_at) AS minute,
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS success_count,
                SUM(CASE WHEN status != 'success' THEN 1 ELSE 0 END) AS error_count
            FROM request_history
            WHERE completed_at >= ?
            GROUP BY strftime('%Y-%m-%dT%H:%M:00Z', completed_at)
            ORDER BY minute ASC
            "#,
        )
        .bind(&cutoff)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get recent history by minute: {}", e)))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// レコードをフィルタリング＆ページネーション
    pub async fn filter_and_paginate(
        &self,
        filter: &RecordFilter,
        page: usize,
        per_page: usize,
    ) -> RouterResult<FilteredRecords> {
        // クエリを動的に構築
        let mut conditions = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(ref model) = filter.model {
            conditions.push("model LIKE ?");
            params.push(format!("%{}%", model));
        }

        if let Some(endpoint_id) = filter.endpoint_id {
            conditions.push("endpoint_id = ?");
            params.push(endpoint_id.to_string());
        }

        if let Some(ref status) = filter.status {
            conditions.push("status = ?");
            params.push(match status {
                FilterStatus::Success => "success".to_string(),
                FilterStatus::Error => "error".to_string(),
            });
        }

        if let Some(start_time) = filter.start_time {
            conditions.push("timestamp >= ?");
            params.push(start_time.to_rfc3339());
        }

        if let Some(end_time) = filter.end_time {
            conditions.push("timestamp <= ?");
            params.push(end_time.to_rfc3339());
        }

        if let Some(ref client_ip) = filter.client_ip {
            conditions.push("client_ip = ?");
            params.push(client_ip.clone());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // 総件数を取得
        let count_sql = format!(
            "SELECT COUNT(*) as count FROM request_history {}",
            where_clause
        );
        let total_count = self.execute_count_query(&count_sql, &params).await?;

        // ページネーション
        let offset = (page.saturating_sub(1)) * per_page;
        let data_sql = format!(
            "SELECT * FROM request_history {} ORDER BY timestamp DESC LIMIT ? OFFSET ?",
            where_clause
        );

        let rows = self
            .execute_select_query(&data_sql, &params, per_page as i64, offset as i64)
            .await?;

        let records: RouterResult<Vec<RequestResponseRecord>> =
            rows.into_iter().map(|row| row.try_into()).collect();

        Ok(FilteredRecords {
            records: records?,
            total_count,
            page,
            per_page,
        })
    }

    /// カウントクエリを実行
    async fn execute_count_query(&self, sql: &str, params: &[String]) -> RouterResult<usize> {
        // パラメータ数に応じて動的にバインド
        let result = match params.len() {
            0 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .fetch_one(&self.pool)
                    .await
            }
            1 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .fetch_one(&self.pool)
                    .await
            }
            2 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .fetch_one(&self.pool)
                    .await
            }
            3 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .fetch_one(&self.pool)
                    .await
            }
            4 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .fetch_one(&self.pool)
                    .await
            }
            5 => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(&params[4])
                    .fetch_one(&self.pool)
                    .await
            }
            _ => {
                sqlx::query_scalar::<_, i64>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(&params[4])
                    .bind(&params[5])
                    .fetch_one(&self.pool)
                    .await
            }
        };

        result
            .map(|c| c as usize)
            .map_err(|e| LbError::Database(format!("Failed to count records: {}", e)))
    }

    /// SELECTクエリを実行
    async fn execute_select_query(
        &self,
        sql: &str,
        params: &[String],
        limit: i64,
        offset: i64,
    ) -> RouterResult<Vec<RequestHistoryRow>> {
        // パラメータ数に応じて動的にバインド
        let result = match params.len() {
            0 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            1 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            2 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            3 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            4 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            5 => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(&params[4])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
            _ => {
                sqlx::query_as::<_, RequestHistoryRow>(sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(&params[4])
                    .bind(&params[5])
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(&self.pool)
                    .await
            }
        };

        result.map_err(|e| LbError::Database(format!("Failed to query records: {}", e)))
    }

    /// トークン統計を取得（全体）
    pub async fn get_token_statistics(&self) -> RouterResult<TokenStatistics> {
        let row = sqlx::query_as::<_, TokenStatisticsRow>(
            r#"
            SELECT
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens
            FROM request_history
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get token statistics: {}", e)))?;

        Ok(TokenStatistics {
            total_input_tokens: row.total_input_tokens as u64,
            total_output_tokens: row.total_output_tokens as u64,
            total_tokens: row.total_tokens as u64,
        })
    }

    /// トークン統計を取得（モデル別）
    pub async fn get_token_statistics_by_model(&self) -> RouterResult<Vec<ModelTokenStatistics>> {
        let rows = sqlx::query_as::<_, ModelTokenStatisticsRow>(
            r#"
            SELECT
                model,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            GROUP BY model
            ORDER BY total_tokens DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            LbError::Database(format!("Failed to get token statistics by model: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .map(|row| ModelTokenStatistics {
                model: row.model,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }

    /// トークン統計を取得（エンドポイント別）
    pub async fn get_token_statistics_by_endpoint(
        &self,
    ) -> RouterResult<Vec<EndpointTokenStatistics>> {
        let rows = sqlx::query_as::<_, EndpointTokenStatisticsRow>(
            r#"
            SELECT
                endpoint_id,
                endpoint_name,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            GROUP BY endpoint_id
            ORDER BY total_tokens DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            LbError::Database(format!("Failed to get token statistics by endpoint: {}", e))
        })?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let endpoint_id = Uuid::parse_str(&row.endpoint_id).ok()?;
                Some(EndpointTokenStatistics {
                    endpoint_id,
                    endpoint_name: row.endpoint_name,
                    total_input_tokens: row.total_input_tokens as u64,
                    total_output_tokens: row.total_output_tokens as u64,
                    total_tokens: row.total_tokens as u64,
                    request_count: row.request_count as u64,
                })
            })
            .collect())
    }

    /// 日次トークン統計を取得
    pub async fn get_daily_token_statistics(
        &self,
        days: u32,
    ) -> RouterResult<Vec<crate::api::dashboard::DailyTokenStats>> {
        let rows = sqlx::query_as::<_, DailyTokenStatisticsRow>(
            r#"
            SELECT
                DATE(timestamp) as date,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            WHERE timestamp >= DATE('now', '-' || ? || ' days')
            GROUP BY DATE(timestamp)
            ORDER BY date DESC
            "#,
        )
        .bind(days as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get daily token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| crate::api::dashboard::DailyTokenStats {
                date: row.date,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }

    /// 月次トークン統計を取得
    pub async fn get_monthly_token_statistics(
        &self,
        months: u32,
    ) -> RouterResult<Vec<crate::api::dashboard::MonthlyTokenStats>> {
        let rows = sqlx::query_as::<_, MonthlyTokenStatisticsRow>(
            r#"
            SELECT
                strftime('%Y-%m', timestamp) as month,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(
                    SUM(
                        COALESCE(
                            total_tokens,
                            COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)
                        )
                    ),
                    0
                ) as total_tokens,
                COUNT(*) as request_count
            FROM request_history
            WHERE timestamp >= DATE('now', '-' || ? || ' months')
            GROUP BY strftime('%Y-%m', timestamp)
            ORDER BY month DESC
            "#,
        )
        .bind(months as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to get monthly token statistics: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| crate::api::dashboard::MonthlyTokenStats {
                month: row.month,
                total_input_tokens: row.total_input_tokens as u64,
                total_output_tokens: row.total_output_tokens as u64,
                total_tokens: row.total_tokens as u64,
                request_count: row.request_count as u64,
            })
            .collect())
    }
}

fn legacy_request_history_path() -> RouterResult<PathBuf> {
    if let Ok(dir) = env::var(LEGACY_DATA_DIR_ENV) {
        return Ok(PathBuf::from(dir).join(LEGACY_REQUEST_HISTORY_FILE));
    }

    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| LbError::Internal("Failed to resolve home directory".to_string()))?;

    Ok(PathBuf::from(home)
        .join(DEFAULT_DATA_DIR)
        .join(LEGACY_REQUEST_HISTORY_FILE))
}

fn legacy_migrated_path(original: &Path) -> PathBuf {
    let file_name = original
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(LEGACY_REQUEST_HISTORY_FILE);
    let migrated_name = format!("{}.migrated", file_name);
    original.with_file_name(migrated_name)
}

fn parse_legacy_records(contents: &str) -> RouterResult<Vec<RequestResponseRecord>> {
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Vec<RequestResponseRecord>>(contents) {
        Ok(records) => Ok(records),
        Err(primary_err) => {
            let mut records = Vec::new();
            let stream =
                serde_json::Deserializer::from_str(contents).into_iter::<RequestResponseRecord>();
            for record in stream {
                match record {
                    Ok(item) => records.push(item),
                    Err(err) => return Err(LbError::Common(err.into())),
                }
            }

            if records.is_empty() {
                return Err(LbError::Common(primary_err.into()));
            }

            Ok(records)
        }
    }
}

/// SQLiteから取得した行データ
#[derive(sqlx::FromRow)]
struct RequestHistoryRow {
    id: String,
    timestamp: String,
    request_type: String,
    model: String,
    endpoint_id: String,
    endpoint_name: String,
    endpoint_ip: String,
    client_ip: Option<String>,
    request_body: String,
    response_body: Option<String>,
    duration_ms: i64,
    status: String,
    error_message: Option<String>,
    #[allow(dead_code)]
    completed_at: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    api_key_id: Option<String>,
}

impl TryFrom<RequestHistoryRow> for RequestResponseRecord {
    type Error = LbError;

    fn try_from(row: RequestHistoryRow) -> Result<Self, Self::Error> {
        let id = Uuid::parse_str(&row.id)
            .map_err(|e| LbError::Database(format!("Invalid UUID: {}", e)))?;

        let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
            .map_err(|e| LbError::Database(format!("Invalid timestamp: {}", e)))?
            .with_timezone(&Utc);

        let request_type = match row.request_type.as_str() {
            "AnthropicMessages" => RequestType::AnthropicMessages,
            "Chat" => RequestType::Chat,
            "Generate" => RequestType::Generate,
            "Embeddings" => RequestType::Embeddings,
            "Transcription" => RequestType::Transcription,
            "Speech" => RequestType::Speech,
            "ImageGeneration" => RequestType::ImageGeneration,
            "ImageEdit" => RequestType::ImageEdit,
            "ImageVariation" => RequestType::ImageVariation,
            _ => RequestType::Chat, // フォールバック
        };

        let endpoint_id = Uuid::parse_str(&row.endpoint_id)
            .map_err(|e| LbError::Database(format!("Invalid endpoint UUID: {}", e)))?;

        let endpoint_ip: IpAddr = row
            .endpoint_ip
            .parse()
            .map_err(|e| LbError::Database(format!("Invalid endpoint IP: {}", e)))?;

        let client_ip = row
            .client_ip
            .map(|ip| {
                ip.parse::<IpAddr>()
                    .map_err(|e| LbError::Database(format!("Invalid client IP: {}", e)))
            })
            .transpose()?;

        let request_body: serde_json::Value = serde_json::from_str(&row.request_body)
            .map_err(|e| LbError::Database(format!("Invalid request body: {}", e)))?;

        let response_body = row
            .response_body
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| LbError::Database(format!("Invalid response body: {}", e)))?;

        let status = match row.status.as_str() {
            "success" => RecordStatus::Success,
            "error" => RecordStatus::Error {
                message: row.error_message.unwrap_or_default(),
            },
            _ => RecordStatus::Success,
        };

        let completed_at = DateTime::parse_from_rfc3339(&row.completed_at)
            .map_err(|e| LbError::Database(format!("Invalid completed_at: {}", e)))?
            .with_timezone(&Utc);

        Ok(RequestResponseRecord {
            id,
            timestamp,
            request_type,
            model: row.model,
            endpoint_id,
            endpoint_name: row.endpoint_name,
            endpoint_ip,
            client_ip,
            request_body,
            response_body,
            duration_ms: row.duration_ms as u64,
            status,
            completed_at,
            input_tokens: row.input_tokens.map(|v| v as u32),
            output_tokens: row.output_tokens.map(|v| v as u32),
            total_tokens: row.total_tokens.map(|v| v as u32),
            api_key_id: row
                .api_key_id
                .map(|id| {
                    Uuid::parse_str(&id)
                        .map_err(|e| LbError::Database(format!("Invalid api_key_id UUID: {}", e)))
                })
                .transpose()?,
        })
    }
}

/// 分単位の履歴集計ポイント（起動時seeding用）
#[derive(Debug, Clone)]
pub struct MinuteHistoryPoint {
    /// 分（UTC ISO8601形式）
    pub minute: String,
    /// 成功リクエスト数
    pub success_count: i64,
    /// 失敗リクエスト数
    pub error_count: i64,
}

#[derive(sqlx::FromRow)]
struct MinuteHistoryRow {
    minute: String,
    success_count: i64,
    error_count: i64,
}

impl From<MinuteHistoryRow> for MinuteHistoryPoint {
    fn from(row: MinuteHistoryRow) -> Self {
        MinuteHistoryPoint {
            minute: row.minute,
            success_count: row.success_count,
            error_count: row.error_count,
        }
    }
}

/// レコードフィルタ
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    /// モデル名フィルタ（部分一致）
    pub model: Option<String>,
    /// エンドポイントIDフィルタ
    pub endpoint_id: Option<Uuid>,
    /// ステータスフィルタ
    pub status: Option<FilterStatus>,
    /// 開始時刻フィルタ
    pub start_time: Option<DateTime<Utc>>,
    /// 終了時刻フィルタ
    pub end_time: Option<DateTime<Utc>>,
    /// クライアントIPフィルタ（完全一致）
    pub client_ip: Option<String>,
}

impl RecordFilter {
    /// レコードがフィルタ条件に一致するか（テスト用）
    #[cfg(test)]
    pub fn matches(&self, record: &RequestResponseRecord) -> bool {
        if let Some(ref model) = self.model {
            if !record.model.contains(model) {
                return false;
            }
        }

        if let Some(endpoint_id) = self.endpoint_id {
            if record.endpoint_id != endpoint_id {
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

        if let Some(ref client_ip) = self.client_ip {
            match &record.client_ip {
                Some(ip) if ip.to_string() == *client_ip => {}
                _ => return false,
            }
        }

        true
    }
}

/// フィルタ用のステータス
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// トークン統計（全体）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenStatistics {
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
}

/// トークン統計（モデル別）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelTokenStatistics {
    /// モデル名
    pub model: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// トークン統計（エンドポイント別）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndpointTokenStatistics {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// エンドポイント名
    pub endpoint_name: String,
    /// 入力トークン合計
    pub total_input_tokens: u64,
    /// 出力トークン合計
    pub total_output_tokens: u64,
    /// 総トークン合計
    pub total_tokens: u64,
    /// リクエスト数
    pub request_count: u64,
}

/// SQLiteから取得したトークン統計行（全体）
#[derive(sqlx::FromRow)]
struct TokenStatisticsRow {
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
}

/// SQLiteから取得したトークン統計行（モデル別）
#[derive(sqlx::FromRow)]
struct ModelTokenStatisticsRow {
    model: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（エンドポイント別）
#[derive(sqlx::FromRow)]
struct EndpointTokenStatisticsRow {
    endpoint_id: String,
    endpoint_name: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（日次）
#[derive(sqlx::FromRow)]
struct DailyTokenStatisticsRow {
    date: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// SQLiteから取得したトークン統計行（月次）
#[derive(sqlx::FromRow)]
struct MonthlyTokenStatisticsRow {
    month: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_tokens: i64,
    request_count: i64,
}

/// IPランキングの1エントリ
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientIpRanking {
    /// IPアドレス（IPv6は/64プレフィックス）
    pub ip: String,
    /// リクエスト数
    pub request_count: i64,
    /// 最終アクセス時刻
    pub last_seen: String,
    /// 閾値超過フラグ（デフォルトfalse、API層で設定）
    pub is_alert: bool,
    /// 使用APIキー数
    pub api_key_count: i64,
}

/// IPランキングの集計結果
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientIpRankingResult {
    /// ランキング一覧
    pub rankings: Vec<ClientIpRanking>,
    /// 総件数
    pub total_count: usize,
    /// 現在のページ番号
    pub page: usize,
    /// 1ページあたりの件数
    pub per_page: usize,
}

/// SQLiteから取得したIP集計行
#[derive(sqlx::FromRow)]
struct ClientIpRow {
    client_ip: Option<String>,
    request_count: i64,
    last_seen: String,
}

#[derive(Debug, Clone)]
enum ClientIpFilter {
    Exact(String),
    Ipv6Prefix64(String),
}

impl ClientIpFilter {
    fn from_input(ip: &str) -> Self {
        if let Some(raw_prefix) = ip.strip_suffix("/64") {
            if let Ok(v6) = raw_prefix.parse::<std::net::Ipv6Addr>() {
                return Self::Ipv6Prefix64(format!("{v6}/64"));
            }
        }

        if let Ok(parsed_ip) = ip.parse::<IpAddr>() {
            return Self::Exact(parsed_ip.to_string());
        }

        Self::Exact(ip.to_string())
    }
}

#[derive(sqlx::FromRow)]
struct ClientIpValueRow {
    client_ip: String,
}

fn build_in_clause(column: &str, count: usize) -> String {
    let placeholders = std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} IN ({placeholders})")
}

impl RequestHistoryStorage {
    async fn resolve_client_ips_for_filter(&self, ip: &str) -> RouterResult<Vec<String>> {
        let filter = ClientIpFilter::from_input(ip);

        match filter {
            ClientIpFilter::Exact(exact_ip) => Ok(vec![exact_ip]),
            ClientIpFilter::Ipv6Prefix64(prefix64) => {
                let rows: Vec<ClientIpValueRow> = sqlx::query_as(
                    "SELECT DISTINCT client_ip
                     FROM request_history
                     WHERE client_ip IS NOT NULL",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|e| LbError::Database(e.to_string()))?;

                Ok(rows
                    .into_iter()
                    .map(|row| row.client_ip)
                    .filter(|stored_ip| crate::common::ip::ipv6_to_prefix64(stored_ip) == prefix64)
                    .collect())
            }
        }
    }

    /// IPランキングを取得（リクエスト数降順、ページネーション付き）
    ///
    /// IPv6アドレスは/64プレフィックスでグルーピングして集計する。
    pub async fn get_client_ip_ranking(
        &self,
        hours: u32,
        page: usize,
        per_page: usize,
        ip_prefix: Option<&str>,
    ) -> RouterResult<ClientIpRankingResult> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        // SQLiteでIPごとの集計を取得（NULLのclient_ipは除外）
        let query_str = if ip_prefix.is_some() {
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ? AND client_ip LIKE ? || '%'
             GROUP BY client_ip
             ORDER BY request_count DESC"
        } else {
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY client_ip
             ORDER BY request_count DESC"
        };

        let mut query = sqlx::query_as::<_, ClientIpRow>(query_str);
        query = query.bind(&cutoff_str);
        if let Some(prefix) = ip_prefix {
            query = query.bind(prefix);
        }

        let rows: Vec<ClientIpRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        // IPv6 /64グルーピング: Rust側で集約
        use std::collections::HashMap;
        let mut grouped: HashMap<String, (i64, String)> = HashMap::new();

        for row in &rows {
            let ip_str = match &row.client_ip {
                Some(ip) => ip.as_str(),
                None => continue,
            };
            let key = crate::common::ip::ipv6_to_prefix64(ip_str);

            let entry = grouped.entry(key).or_insert_with(|| (0, String::new()));
            entry.0 += row.request_count;
            // last_seenは最新を保持
            if entry.1.is_empty() || row.last_seen > entry.1 {
                entry.1.clone_from(&row.last_seen);
            }
        }

        let mut rankings: Vec<ClientIpRanking> = grouped
            .into_iter()
            .map(|(ip, (count, last_seen))| ClientIpRanking {
                ip,
                request_count: count,
                last_seen,
                is_alert: false,
                api_key_count: 0, // Phase 8で正確な集計に更新
            })
            .collect();

        // リクエスト数降順ソート
        rankings.sort_by_key(|r| std::cmp::Reverse(r.request_count));

        let total_count = rankings.len();
        let offset = (page.saturating_sub(1)) * per_page;
        let paginated = rankings.into_iter().skip(offset).take(per_page).collect();

        Ok(ClientIpRankingResult {
            rankings: paginated,
            total_count,
            page,
            per_page,
        })
    }

    /// 過去N時間のIP別リクエスト数を取得（閾値チェック用）
    ///
    /// IPv6は/64プレフィックスでグルーピングして集計する。
    pub async fn get_ip_request_counts_since(
        &self,
        hours: u32,
    ) -> RouterResult<std::collections::HashMap<String, i64>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<ClientIpRow> = sqlx::query_as(
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY client_ip",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for row in &rows {
            let ip_str = match &row.client_ip {
                Some(ip) => ip.as_str(),
                None => continue,
            };
            let key = crate::common::ip::ipv6_to_prefix64(ip_str);
            *counts.entry(key).or_insert(0) += row.request_count;
        }

        Ok(counts)
    }

    /// ユニークIP数の1時間刻みタイムラインを取得
    ///
    /// 指定時間数分の24ポイント（各時間帯のユニークIP数）を返す。
    /// データがない時間帯はunique_ips=0で埋める。
    pub async fn get_unique_ip_timeline(
        &self,
        hours: u32,
    ) -> RouterResult<Vec<UniqueIpTimelinePoint>> {
        let now = Utc::now();
        let cutoff = now - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<TimelineClientRow> = sqlx::query_as(
            "SELECT strftime('%Y-%m-%dT%H:00:00', timestamp) as hour,
                    client_ip
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY strftime('%Y-%m-%dT%H:00:00', timestamp), client_ip
             ORDER BY hour ASC",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        // 24時間分のポイントを生成（データがない時間帯は0埋め）
        use std::collections::{HashMap, HashSet};
        let mut grouped: HashMap<String, HashSet<String>> = HashMap::new();
        for row in rows {
            let normalized_ip = crate::common::ip::ipv6_to_prefix64(&row.client_ip);
            grouped.entry(row.hour).or_default().insert(normalized_ip);
        }
        let row_map: HashMap<String, i64> = grouped
            .into_iter()
            .map(|(hour, unique_ips)| (hour, unique_ips.len() as i64))
            .collect();

        let mut timeline = Vec::with_capacity(hours as usize);
        for h in (1..=hours).rev() {
            let point_time = now - Duration::hours(h as i64);
            let hour_key = point_time.format("%Y-%m-%dT%H:00:00").to_string();
            let unique_ips = row_map.get(&hour_key).copied().unwrap_or(0);
            timeline.push(UniqueIpTimelinePoint {
                hour: hour_key,
                unique_ips,
            });
        }

        Ok(timeline)
    }

    /// モデル別リクエスト分布を取得
    ///
    /// 指定時間数内のモデル別リクエスト数とパーセンテージを返す。
    pub async fn get_model_distribution_by_clients(
        &self,
        hours: u32,
    ) -> RouterResult<Vec<ModelDistribution>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<ModelDistRow> = sqlx::query_as(
            "SELECT model, COUNT(*) as request_count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY model
             ORDER BY request_count DESC",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        let total: i64 = rows.iter().map(|r| r.request_count).sum();

        let distributions = rows
            .into_iter()
            .map(|r| {
                let percentage = if total > 0 {
                    (r.request_count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                ModelDistribution {
                    model: r.model,
                    request_count: r.request_count,
                    percentage: (percentage * 10.0).round() / 10.0, // 小数点1位
                }
            })
            .collect();

        Ok(distributions)
    }

    /// リクエストヒートマップを取得（曜日×時間帯）
    ///
    /// 指定時間数内のリクエストを曜日(0-6)×時間帯(0-23)で集計。
    /// データがないセルはcount=0で埋める。
    pub async fn get_request_heatmap(
        &self,
        hours: u32,
        ip_prefix: Option<&str>,
    ) -> RouterResult<Vec<HeatmapCell>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let query_str = if ip_prefix.is_some() {
            "SELECT CAST(strftime('%w', timestamp) AS INTEGER) as day_of_week,
                    CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ? AND client_ip LIKE ? || '%'
             GROUP BY strftime('%w', timestamp), strftime('%H', timestamp)"
        } else {
            "SELECT CAST(strftime('%w', timestamp) AS INTEGER) as day_of_week,
                    CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY strftime('%w', timestamp), strftime('%H', timestamp)"
        };

        let mut query = sqlx::query_as::<_, HeatmapRow>(query_str);
        query = query.bind(&cutoff_str);
        if let Some(prefix) = ip_prefix {
            query = query.bind(prefix);
        }

        let rows: Vec<HeatmapRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        // 7×24 = 168セルの完全マトリックスを生成
        use std::collections::HashMap;
        let row_map: HashMap<(i64, i64), i64> = rows
            .into_iter()
            .map(|r| ((r.day_of_week, r.hour), r.count))
            .collect();

        let mut cells = Vec::with_capacity(168);
        for dow in 0..7 {
            for h in 0..24 {
                let count = row_map.get(&(dow, h)).copied().unwrap_or(0);
                cells.push(HeatmapCell {
                    day_of_week: dow,
                    hour: h,
                    count,
                });
            }
        }

        Ok(cells)
    }

    /// 特定IPのドリルダウン詳細を取得
    pub async fn get_client_detail(&self, ip: &str, limit: usize) -> RouterResult<ClientDetail> {
        let matched_ips = self.resolve_client_ips_for_filter(ip).await?;
        if matched_ips.is_empty() {
            return Ok(ClientDetail::empty());
        }

        let where_clause = build_in_clause("client_ip", matched_ips.len());

        // 合計リクエスト数・初回/最終アクセス
        let summary_sql = format!(
            "SELECT COUNT(*) as total_requests,
                    MIN(timestamp) as first_seen,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE {where_clause}"
        );
        let mut summary_query = sqlx::query_as::<_, ClientSummaryRow>(&summary_sql);
        for matched_ip in &matched_ips {
            summary_query = summary_query.bind(matched_ip);
        }
        let summary: Option<ClientSummaryRow> = summary_query
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let (total_requests, first_seen, last_seen) = match summary {
            Some(s) if s.total_requests > 0 => (s.total_requests, s.first_seen, s.last_seen),
            _ => return Ok(ClientDetail::empty()),
        };

        // 直近リクエスト
        let recent_sql = format!(
            "SELECT id, timestamp, model, status, duration_ms
             FROM request_history
             WHERE {where_clause}
             ORDER BY timestamp DESC
             LIMIT ?"
        );
        let mut recent_query = sqlx::query_as::<_, ClientRecentRequestRow>(&recent_sql);
        for matched_ip in &matched_ips {
            recent_query = recent_query.bind(matched_ip);
        }
        let recent: Vec<ClientRecentRequestRow> = recent_query
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let recent_requests: Vec<ClientRecentRequest> = recent
            .into_iter()
            .map(|r| ClientRecentRequest {
                id: r.id,
                timestamp: r.timestamp,
                model: r.model,
                status: r.status,
                duration_ms: r.duration_ms,
            })
            .collect();

        // モデル分布
        let model_sql = format!(
            "SELECT model, COUNT(*) as request_count
             FROM request_history
             WHERE {where_clause}
             GROUP BY model
             ORDER BY request_count DESC"
        );
        let mut model_query = sqlx::query_as::<_, ModelDistRow>(&model_sql);
        for matched_ip in &matched_ips {
            model_query = model_query.bind(matched_ip);
        }
        let model_rows: Vec<ModelDistRow> = model_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let total_for_pct: i64 = model_rows.iter().map(|r| r.request_count).sum();
        let model_distribution: Vec<ModelDistribution> = model_rows
            .into_iter()
            .map(|r| {
                let pct = if total_for_pct > 0 {
                    (r.request_count as f64 / total_for_pct as f64) * 100.0
                } else {
                    0.0
                };
                ModelDistribution {
                    model: r.model,
                    request_count: r.request_count,
                    percentage: (pct * 10.0).round() / 10.0,
                }
            })
            .collect();

        // 時間帯パターン（24時間）
        let hourly_sql = format!(
            "SELECT CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE {where_clause}
             GROUP BY strftime('%H', timestamp)
             ORDER BY hour ASC"
        );
        let mut hourly_query = sqlx::query_as::<_, HourlyPatternRow>(&hourly_sql);
        for matched_ip in &matched_ips {
            hourly_query = hourly_query.bind(matched_ip);
        }
        let hourly_rows: Vec<HourlyPatternRow> = hourly_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        use std::collections::HashMap;
        let hourly_map: HashMap<i64, i64> =
            hourly_rows.into_iter().map(|r| (r.hour, r.count)).collect();
        let hourly_pattern: Vec<HourlyPattern> = (0..24)
            .map(|h| HourlyPattern {
                hour: h,
                count: hourly_map.get(&h).copied().unwrap_or(0),
            })
            .collect();

        Ok(ClientDetail {
            total_requests,
            first_seen,
            last_seen,
            recent_requests,
            model_distribution,
            hourly_pattern,
        })
    }

    /// 特定IPのAPIキー別リクエスト数を取得
    pub async fn get_client_api_keys(&self, ip: &str) -> RouterResult<Vec<ClientApiKeyUsage>> {
        let matched_ips = self.resolve_client_ips_for_filter(ip).await?;
        if matched_ips.is_empty() {
            return Ok(vec![]);
        }
        let where_clause = build_in_clause("rh.client_ip", matched_ips.len());
        let sql = format!(
            "SELECT rh.api_key_id, ak.name as key_name, COUNT(*) as request_count
             FROM request_history rh
             LEFT JOIN api_keys ak ON rh.api_key_id = ak.id
             WHERE {where_clause} AND rh.api_key_id IS NOT NULL
             GROUP BY rh.api_key_id
             ORDER BY request_count DESC"
        );
        let mut query = sqlx::query_as::<_, ClientApiKeyRow>(&sql);
        for matched_ip in &matched_ips {
            query = query.bind(matched_ip);
        }
        let rows: Vec<ClientApiKeyRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| ClientApiKeyUsage {
                api_key_id: r.api_key_id,
                name: r.key_name,
                request_count: r.request_count,
            })
            .collect())
    }
}

/// APIキー別リクエスト数
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientApiKeyUsage {
    /// APIキーID
    pub api_key_id: String,
    /// キー名（削除済みの場合None）
    pub name: Option<String>,
    /// リクエスト数
    pub request_count: i64,
}

/// SQLiteから取得したAPIキー集計行
#[derive(sqlx::FromRow)]
struct ClientApiKeyRow {
    api_key_id: String,
    key_name: Option<String>,
    request_count: i64,
}

/// IPドリルダウン詳細
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientDetail {
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 初回アクセス時刻
    pub first_seen: Option<String>,
    /// 最終アクセス時刻
    pub last_seen: Option<String>,
    /// 直近リクエスト
    pub recent_requests: Vec<ClientRecentRequest>,
    /// モデル分布
    pub model_distribution: Vec<ModelDistribution>,
    /// 時間帯パターン（24時間）
    pub hourly_pattern: Vec<HourlyPattern>,
}

impl ClientDetail {
    fn empty() -> Self {
        Self {
            total_requests: 0,
            first_seen: None,
            last_seen: None,
            recent_requests: vec![],
            model_distribution: vec![],
            hourly_pattern: (0..24)
                .map(|h| HourlyPattern { hour: h, count: 0 })
                .collect(),
        }
    }
}

/// 直近リクエスト
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientRecentRequest {
    /// リクエストID
    pub id: String,
    /// タイムスタンプ
    pub timestamp: String,
    /// モデル名
    pub model: String,
    /// ステータス
    pub status: String,
    /// レスポンス時間(ms)
    pub duration_ms: Option<i64>,
}

/// 時間帯パターン
#[derive(Debug, Clone, serde::Serialize)]
pub struct HourlyPattern {
    /// 時間帯 (0-23)
    pub hour: i64,
    /// リクエスト数
    pub count: i64,
}

/// SQLiteから取得したサマリ行
#[derive(sqlx::FromRow)]
struct ClientSummaryRow {
    total_requests: i64,
    first_seen: Option<String>,
    last_seen: Option<String>,
}

/// SQLiteから取得した直近リクエスト行
#[derive(sqlx::FromRow)]
struct ClientRecentRequestRow {
    id: String,
    timestamp: String,
    model: String,
    status: String,
    duration_ms: Option<i64>,
}

/// SQLiteから取得した時間帯パターン行
#[derive(sqlx::FromRow)]
struct HourlyPatternRow {
    hour: i64,
    count: i64,
}

/// ヒートマップの1セル
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeatmapCell {
    /// 曜日 (0=日曜, 1=月曜, ..., 6=土曜)
    pub day_of_week: i64,
    /// 時間帯 (0-23)
    pub hour: i64,
    /// リクエスト数
    pub count: i64,
}

/// SQLiteから取得したヒートマップ行
#[derive(sqlx::FromRow)]
struct HeatmapRow {
    day_of_week: i64,
    hour: i64,
    count: i64,
}

/// タイムラインの1ポイント
#[derive(Debug, Clone, serde::Serialize)]
pub struct UniqueIpTimelinePoint {
    /// 時間帯（ISO 8601形式、時間単位に丸め）
    pub hour: String,
    /// ユニークIP数
    pub unique_ips: i64,
}

/// モデル別リクエスト分布
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelDistribution {
    /// モデル名
    pub model: String,
    /// リクエスト数
    pub request_count: i64,
    /// パーセンテージ（小数点1位）
    pub percentage: f64,
}

/// SQLiteから取得したタイムライン生データ行
#[derive(sqlx::FromRow)]
struct TimelineClientRow {
    hour: String,
    client_ip: String,
}

/// SQLiteから取得したモデル分布行
#[derive(sqlx::FromRow)]
struct ModelDistRow {
    model: String,
    request_count: i64,
}

/// 定期クリーンアップタスクを開始
pub fn start_cleanup_task(storage: Arc<RequestHistoryStorage>) {
    let retention_days = get_env_with_fallback_parse(
        REQUEST_HISTORY_RETENTION_DAYS_ENV,
        LEGACY_REQUEST_HISTORY_RETENTION_DAYS_ENV,
        7i64,
    );
    let interval_secs = get_env_with_fallback_parse(
        REQUEST_HISTORY_CLEANUP_INTERVAL_ENV,
        LEGACY_REQUEST_HISTORY_CLEANUP_INTERVAL_ENV,
        3600u64,
    );

    if retention_days <= 0 {
        tracing::info!("Request history cleanup disabled ({} <= 0)", retention_days);
        return;
    }

    tokio::spawn(async move {
        // 起動時に1回実行
        let retention = Duration::days(retention_days);
        if let Err(e) = storage.cleanup_old_records(retention).await {
            tracing::error!("Initial cleanup failed: {}", e);
        }

        // 1時間ごとに実行
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;

            if let Err(e) = storage.cleanup_old_records(retention).await {
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
    use crate::common::protocol::RequestType;
    use serial_test::serial;
    use tempfile::tempdir;

    async fn create_test_pool() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    fn create_test_record(timestamp: DateTime<Utc>) -> RequestResponseRecord {
        RequestResponseRecord {
            id: Uuid::new_v4(),
            timestamp,
            request_type: RequestType::Chat,
            model: "test-model".to_string(),
            endpoint_id: Uuid::new_v4(),
            endpoint_name: "test-node".to_string(),
            endpoint_ip: "192.168.1.100".parse::<IpAddr>().unwrap(),
            client_ip: Some("10.0.0.10".parse::<IpAddr>().unwrap()),
            request_body: serde_json::json!({"test": "request"}),
            response_body: Some(serde_json::json!({"test": "response"})),
            duration_ms: 1000,
            status: RecordStatus::Success,
            completed_at: timestamp + Duration::seconds(1),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id: None,
        }
    }

    #[tokio::test]
    async fn test_save_and_load_record() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);
        let record = create_test_record(Utc::now());

        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, record.id);
    }

    #[tokio::test]
    async fn test_cleanup_old_records() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

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
    }

    #[tokio::test]
    async fn test_filter_by_model() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

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
    }

    #[tokio::test]
    async fn test_pagination() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        // 5件のレコードを作成
        for i in 0..5 {
            let mut record = create_test_record(Utc::now() - Duration::seconds(i));
            record.id = Uuid::new_v4(); // 一意のIDを確保
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
    }

    #[tokio::test]
    #[serial]
    async fn test_import_legacy_request_history_json() {
        let temp_dir = tempdir().expect("temp dir");
        std::env::set_var(LEGACY_DATA_DIR_ENV, temp_dir.path());

        let json_path = temp_dir.path().join(LEGACY_REQUEST_HISTORY_FILE);
        let record = create_test_record(Utc::now());
        let records = vec![record.clone()];
        std::fs::write(&json_path, serde_json::to_string(&records).unwrap()).unwrap();

        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let imported = storage.import_legacy_json_if_present().await.unwrap();
        assert_eq!(imported, 1);

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, record.id);

        let migrated = temp_dir
            .path()
            .join(format!("{}.migrated", LEGACY_REQUEST_HISTORY_FILE));
        assert!(migrated.exists());

        std::env::remove_var(LEGACY_DATA_DIR_ENV);
    }

    // T-6: request_historyテーブルへのトークン保存テスト
    #[tokio::test]
    async fn test_save_and_load_record_with_tokens() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.input_tokens = Some(100);
        record.output_tokens = Some(50);
        record.total_tokens = Some(150);

        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].input_tokens, Some(100));
        assert_eq!(loaded[0].output_tokens, Some(50));
        assert_eq!(loaded[0].total_tokens, Some(150));
    }

    #[tokio::test]
    async fn test_save_and_load_record_with_partial_tokens() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.input_tokens = Some(100);
        // output_tokens と total_tokens は None

        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].input_tokens, Some(100));
        assert_eq!(loaded[0].output_tokens, None);
        assert_eq!(loaded[0].total_tokens, None);
    }

    // T-7: トークン集計クエリテスト（累計/日次/月次）
    #[tokio::test]
    async fn test_token_aggregation_total() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        // 複数レコードを作成
        for i in 0..3 {
            let mut record = create_test_record(Utc::now() - Duration::seconds(i));
            record.id = Uuid::new_v4();
            record.input_tokens = Some(100);
            record.output_tokens = Some(50);
            record.total_tokens = Some(150);
            storage.save_record(&record).await.unwrap();
        }

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 300);
        assert_eq!(stats.total_output_tokens, 150);
        assert_eq!(stats.total_tokens, 450);
    }

    #[tokio::test]
    async fn test_token_aggregation_total_infers_total_tokens_when_null() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let now = Utc::now();

        // total_tokens が NULL の場合は input_tokens + output_tokens を合算する
        let mut record_1 = create_test_record(now);
        record_1.input_tokens = Some(100);
        record_1.output_tokens = Some(50);
        record_1.total_tokens = None;
        storage.save_record(&record_1).await.unwrap();

        // output_tokens が NULL の場合は input_tokens のみをカウントする（SQL上は +0 になる）
        let mut record_2 = create_test_record(now - Duration::seconds(1));
        record_2.input_tokens = Some(10);
        record_2.output_tokens = None;
        record_2.total_tokens = None;
        storage.save_record(&record_2).await.unwrap();

        // input_tokens が NULL の場合は output_tokens のみをカウントする（SQL上は 0+output になる）
        let mut record_3 = create_test_record(now - Duration::seconds(2));
        record_3.input_tokens = None;
        record_3.output_tokens = Some(5);
        record_3.total_tokens = None;
        storage.save_record(&record_3).await.unwrap();

        // total_tokens がある場合はそれを優先する
        let mut record_4 = create_test_record(now - Duration::seconds(3));
        record_4.input_tokens = None;
        record_4.output_tokens = None;
        record_4.total_tokens = Some(7);
        storage.save_record(&record_4).await.unwrap();

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 110);
        assert_eq!(stats.total_output_tokens, 55);
        assert_eq!(stats.total_tokens, 172);
    }

    #[tokio::test]
    async fn test_token_aggregation_by_model() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        // モデルAのレコード
        let mut record_a = create_test_record(Utc::now());
        record_a.model = "model-a".to_string();
        record_a.input_tokens = Some(100);
        record_a.output_tokens = Some(50);
        record_a.total_tokens = Some(150);
        storage.save_record(&record_a).await.unwrap();

        // モデルBのレコード
        let mut record_b = create_test_record(Utc::now());
        record_b.id = Uuid::new_v4();
        record_b.model = "model-b".to_string();
        record_b.input_tokens = Some(200);
        record_b.output_tokens = Some(100);
        record_b.total_tokens = Some(300);
        storage.save_record(&record_b).await.unwrap();

        let stats = storage.get_token_statistics_by_model().await.unwrap();
        assert_eq!(stats.len(), 2);

        let model_a_stats = stats.iter().find(|s| s.model == "model-a").unwrap();
        assert_eq!(model_a_stats.total_input_tokens, 100);
        assert_eq!(model_a_stats.total_output_tokens, 50);

        let model_b_stats = stats.iter().find(|s| s.model == "model-b").unwrap();
        assert_eq!(model_b_stats.total_input_tokens, 200);
        assert_eq!(model_b_stats.total_output_tokens, 100);
    }

    #[tokio::test]
    async fn test_token_aggregation_by_model_infers_total_tokens_when_null() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record_a = create_test_record(Utc::now());
        record_a.model = "model-a".to_string();
        record_a.input_tokens = Some(100);
        record_a.output_tokens = Some(50);
        record_a.total_tokens = None;
        storage.save_record(&record_a).await.unwrap();

        let mut record_b = create_test_record(Utc::now());
        record_b.id = Uuid::new_v4();
        record_b.model = "model-b".to_string();
        record_b.input_tokens = Some(10);
        record_b.output_tokens = None;
        record_b.total_tokens = None;
        storage.save_record(&record_b).await.unwrap();

        let stats = storage.get_token_statistics_by_model().await.unwrap();

        let model_a_stats = stats.iter().find(|s| s.model == "model-a").unwrap();
        assert_eq!(model_a_stats.total_tokens, 150);

        let model_b_stats = stats.iter().find(|s| s.model == "model-b").unwrap();
        assert_eq!(model_b_stats.total_tokens, 10);
    }

    #[tokio::test]
    async fn test_token_aggregation_by_endpoint() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let endpoint_id_1 = Uuid::new_v4();
        let endpoint_id_2 = Uuid::new_v4();

        // エンドポイント1のレコード
        let mut record_1 = create_test_record(Utc::now());
        record_1.endpoint_id = endpoint_id_1;
        record_1.input_tokens = Some(100);
        record_1.output_tokens = Some(50);
        record_1.total_tokens = Some(150);
        storage.save_record(&record_1).await.unwrap();

        // エンドポイント2のレコード
        let mut record_2 = create_test_record(Utc::now());
        record_2.id = Uuid::new_v4();
        record_2.endpoint_id = endpoint_id_2;
        record_2.input_tokens = Some(200);
        record_2.output_tokens = Some(100);
        record_2.total_tokens = Some(300);
        storage.save_record(&record_2).await.unwrap();

        let stats = storage.get_token_statistics_by_endpoint().await.unwrap();
        assert_eq!(stats.len(), 2);

        let endpoint_1_stats = stats
            .iter()
            .find(|s| s.endpoint_id == endpoint_id_1)
            .unwrap();
        assert_eq!(endpoint_1_stats.total_input_tokens, 100);

        let endpoint_2_stats = stats
            .iter()
            .find(|s| s.endpoint_id == endpoint_id_2)
            .unwrap();
        assert_eq!(endpoint_2_stats.total_input_tokens, 200);
    }

    #[tokio::test]
    async fn test_daily_token_stats_infer_total_tokens_when_null() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.input_tokens = Some(100);
        record.output_tokens = Some(50);
        record.total_tokens = None;
        storage.save_record(&record).await.unwrap();

        let stats = storage.get_daily_token_statistics(30).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 100);
        assert_eq!(stats[0].total_output_tokens, 50);
        assert_eq!(stats[0].total_tokens, 150);
        assert_eq!(stats[0].request_count, 1);
    }

    #[tokio::test]
    async fn test_monthly_token_stats_infer_total_tokens_when_null() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.input_tokens = Some(100);
        record.output_tokens = Some(50);
        record.total_tokens = None;
        storage.save_record(&record).await.unwrap();

        let stats = storage.get_monthly_token_statistics(12).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_input_tokens, 100);
        assert_eq!(stats[0].total_output_tokens, 50);
        assert_eq!(stats[0].total_tokens, 150);
        assert_eq!(stats[0].request_count, 1);
    }

    /// T012 [US5]: get_recent_history_by_minute が分単位でリクエスト履歴を
    /// 正しく集計できることを検証
    #[tokio::test]
    async fn test_get_recent_history_by_minute() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let now = Utc::now();

        // 成功レコードを3件保存
        for i in 0..3 {
            let mut record = create_test_record(now - Duration::seconds(i * 10));
            record.status = RecordStatus::Success;
            record.completed_at = now - Duration::seconds(i * 10);
            storage.save_record(&record).await.unwrap();
        }

        // 失敗レコードを1件保存（同じ分内）
        let mut fail_record = create_test_record(now - Duration::seconds(5));
        fail_record.status = RecordStatus::Error {
            message: "test error".to_string(),
        };
        fail_record.completed_at = now - Duration::seconds(5);
        storage.save_record(&fail_record).await.unwrap();

        // 直近10分の集計を取得
        let history = storage.get_recent_history_by_minute(10).await.unwrap();

        // 少なくとも1つの分単位エントリが返ること
        assert!(!history.is_empty(), "should have at least one minute entry");

        // 全エントリの合計を検証
        let total_success: i64 = history.iter().map(|h| h.success_count).sum();
        let total_error: i64 = history.iter().map(|h| h.error_count).sum();
        assert_eq!(total_success, 3, "should have 3 success records");
        assert_eq!(total_error, 1, "should have 1 error record");
    }

    // --- Additional tests ---

    #[tokio::test]
    async fn test_get_record_by_id() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let record = create_test_record(Utc::now());
        storage.save_record(&record).await.unwrap();

        let found = storage.get_record_by_id(record.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, record.id);
    }

    #[tokio::test]
    async fn test_get_record_by_id_not_found() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let found = storage.get_record_by_id(Uuid::new_v4()).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_load_records_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let loaded = storage.load_records().await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn test_load_records_ordering() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let now = Utc::now();
        let old = create_test_record(now - Duration::hours(2));
        let new = create_test_record(now);

        storage.save_record(&old).await.unwrap();
        storage.save_record(&new).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 2);
        // Descending order: newest first
        assert_eq!(loaded[0].id, new.id);
        assert_eq!(loaded[1].id, old.id);
    }

    #[tokio::test]
    async fn test_save_error_record() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.status = RecordStatus::Error {
            message: "connection refused".to_string(),
        };
        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        match &loaded[0].status {
            RecordStatus::Error { message } => {
                assert_eq!(message, "connection refused");
            }
            _ => panic!("expected error status"),
        }
    }

    #[tokio::test]
    async fn test_filter_by_status() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut success = create_test_record(Utc::now());
        success.status = RecordStatus::Success;
        storage.save_record(&success).await.unwrap();

        let mut error = create_test_record(Utc::now() - Duration::seconds(1));
        error.status = RecordStatus::Error {
            message: "fail".to_string(),
        };
        storage.save_record(&error).await.unwrap();

        // Filter success only
        let filter = RecordFilter {
            status: Some(FilterStatus::Success),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 1);
        assert_eq!(result.records[0].id, success.id);

        // Filter error only
        let filter = RecordFilter {
            status: Some(FilterStatus::Error),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 1);
        assert_eq!(result.records[0].id, error.id);
    }

    #[tokio::test]
    async fn test_filter_by_endpoint_id() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let ep1 = Uuid::new_v4();
        let ep2 = Uuid::new_v4();

        let mut r1 = create_test_record(Utc::now());
        r1.endpoint_id = ep1;
        storage.save_record(&r1).await.unwrap();

        let mut r2 = create_test_record(Utc::now() - Duration::seconds(1));
        r2.endpoint_id = ep2;
        storage.save_record(&r2).await.unwrap();

        let filter = RecordFilter {
            endpoint_id: Some(ep1),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 1);
        assert_eq!(result.records[0].endpoint_id, ep1);
    }

    #[tokio::test]
    async fn test_filter_by_time_range() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let now = Utc::now();
        let old = create_test_record(now - Duration::hours(5));
        let mid = create_test_record(now - Duration::hours(2));
        let recent = create_test_record(now);

        storage.save_record(&old).await.unwrap();
        storage.save_record(&mid).await.unwrap();
        storage.save_record(&recent).await.unwrap();

        // Filter: last 3 hours
        let filter = RecordFilter {
            start_time: Some(now - Duration::hours(3)),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 2);
    }

    #[tokio::test]
    async fn test_cleanup_does_not_remove_recent() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let record = create_test_record(Utc::now());
        storage.save_record(&record).await.unwrap();

        // Cleanup with 1 day retention should keep today's records
        storage
            .cleanup_old_records(Duration::days(1))
            .await
            .unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn test_token_statistics_empty_db() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let stats = storage.get_token_statistics().await.unwrap();
        assert_eq!(stats.total_input_tokens, 0);
        assert_eq!(stats.total_output_tokens, 0);
        assert_eq!(stats.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_token_statistics_by_model_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let stats = storage.get_token_statistics_by_model().await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_token_statistics_by_endpoint_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let stats = storage.get_token_statistics_by_endpoint().await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_record_with_api_key_id() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let api_key = Uuid::new_v4();
        let mut record = create_test_record(Utc::now());
        record.api_key_id = Some(api_key);
        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].api_key_id, Some(api_key));
    }

    #[tokio::test]
    async fn test_record_with_no_client_ip() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.client_ip = None;
        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].client_ip.is_none());
    }

    #[test]
    fn test_record_filter_matches() {
        let record = create_test_record(Utc::now());

        // Empty filter matches everything
        let filter = RecordFilter::default();
        assert!(filter.matches(&record));

        // Model filter (partial match)
        let filter = RecordFilter {
            model: Some("test".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        // Non-matching model
        let filter = RecordFilter {
            model: Some("nonexistent".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&record));
    }

    #[test]
    fn test_parse_legacy_records_empty() {
        let records = parse_legacy_records("").unwrap();
        assert!(records.is_empty());

        let records = parse_legacy_records("   ").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_parse_legacy_records_invalid_json() {
        let result = parse_legacy_records("not json at all");
        assert!(result.is_err());
    }

    // =====================================================================
    // 追加テスト: RecordFilter
    // =====================================================================

    #[test]
    fn test_record_filter_matches_endpoint_id() {
        let record = create_test_record(Utc::now());

        let filter = RecordFilter {
            endpoint_id: Some(record.endpoint_id),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        let filter = RecordFilter {
            endpoint_id: Some(Uuid::new_v4()),
            ..Default::default()
        };
        assert!(!filter.matches(&record));
    }

    #[test]
    fn test_record_filter_matches_status() {
        let mut record = create_test_record(Utc::now());
        record.status = RecordStatus::Success;

        let filter = RecordFilter {
            status: Some(FilterStatus::Success),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        let filter = RecordFilter {
            status: Some(FilterStatus::Error),
            ..Default::default()
        };
        assert!(!filter.matches(&record));

        // Error record
        record.status = RecordStatus::Error {
            message: "fail".to_string(),
        };
        let filter = RecordFilter {
            status: Some(FilterStatus::Error),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        let filter = RecordFilter {
            status: Some(FilterStatus::Success),
            ..Default::default()
        };
        assert!(!filter.matches(&record));
    }

    #[test]
    fn test_record_filter_matches_time_range() {
        let now = Utc::now();
        let record = create_test_record(now);

        // start_time before record -> match
        let filter = RecordFilter {
            start_time: Some(now - Duration::hours(1)),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        // start_time after record -> no match
        let filter = RecordFilter {
            start_time: Some(now + Duration::hours(1)),
            ..Default::default()
        };
        assert!(!filter.matches(&record));

        // end_time after record -> match
        let filter = RecordFilter {
            end_time: Some(now + Duration::hours(1)),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        // end_time before record -> no match
        let filter = RecordFilter {
            end_time: Some(now - Duration::hours(1)),
            ..Default::default()
        };
        assert!(!filter.matches(&record));
    }

    #[test]
    fn test_record_filter_matches_client_ip() {
        let record = create_test_record(Utc::now());
        // record has client_ip = Some("10.0.0.10")

        let filter = RecordFilter {
            client_ip: Some("10.0.0.10".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        let filter = RecordFilter {
            client_ip: Some("192.168.1.1".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&record));

        // Record with no client_ip
        let mut record_no_ip = create_test_record(Utc::now());
        record_no_ip.client_ip = None;
        let filter = RecordFilter {
            client_ip: Some("10.0.0.10".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&record_no_ip));
    }

    #[test]
    fn test_record_filter_matches_combined() {
        let record = create_test_record(Utc::now());

        // Multiple filters that all match
        let filter = RecordFilter {
            model: Some("test".to_string()),
            endpoint_id: Some(record.endpoint_id),
            status: Some(FilterStatus::Success),
            ..Default::default()
        };
        assert!(filter.matches(&record));

        // One filter doesn't match -> overall doesn't match
        let filter = RecordFilter {
            model: Some("test".to_string()),
            endpoint_id: Some(Uuid::new_v4()), // wrong endpoint
            status: Some(FilterStatus::Success),
            ..Default::default()
        };
        assert!(!filter.matches(&record));
    }

    // =====================================================================
    // 追加テスト: FilterStatus deserialization
    // =====================================================================

    #[test]
    fn test_filter_status_deserialize() {
        let success: FilterStatus = serde_json::from_str("\"success\"").unwrap();
        assert_eq!(success, FilterStatus::Success);

        let error: FilterStatus = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(error, FilterStatus::Error);
    }

    // =====================================================================
    // 追加テスト: FilteredRecords serialize
    // =====================================================================

    #[test]
    fn test_filtered_records_serialize() {
        let fr = FilteredRecords {
            records: vec![],
            total_count: 0,
            page: 1,
            per_page: 10,
        };
        let json = serde_json::to_value(&fr).unwrap();
        assert_eq!(json["total_count"], 0);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 10);
    }

    // =====================================================================
    // 追加テスト: TokenStatistics serialize
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
    // 追加テスト: ModelTokenStatistics serialize
    // =====================================================================

    #[test]
    fn test_model_token_statistics_serialize() {
        let stats = ModelTokenStatistics {
            model: "gpt-4".to_string(),
            total_input_tokens: 200,
            total_output_tokens: 100,
            total_tokens: 300,
            request_count: 5,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["request_count"], 5);
    }

    // =====================================================================
    // 追加テスト: EndpointTokenStatistics serialize
    // =====================================================================

    #[test]
    fn test_endpoint_token_statistics_serialize() {
        let ep_id = Uuid::new_v4();
        let stats = EndpointTokenStatistics {
            endpoint_id: ep_id,
            endpoint_name: "ep-1".to_string(),
            total_input_tokens: 300,
            total_output_tokens: 150,
            total_tokens: 450,
            request_count: 10,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["endpoint_name"], "ep-1");
        assert_eq!(json["request_count"], 10);
    }

    // =====================================================================
    // 追加テスト: MinuteHistoryPoint
    // =====================================================================

    #[test]
    fn test_minute_history_point_from_row() {
        let row = MinuteHistoryRow {
            minute: "2024-01-01T12:00:00Z".to_string(),
            success_count: 5,
            error_count: 2,
        };
        let point: MinuteHistoryPoint = row.into();
        assert_eq!(point.minute, "2024-01-01T12:00:00Z");
        assert_eq!(point.success_count, 5);
        assert_eq!(point.error_count, 2);
    }

    // =====================================================================
    // 追加テスト: legacy_migrated_path
    // =====================================================================

    #[test]
    fn test_legacy_migrated_path() {
        let original = std::path::Path::new("/data/request_history.json");
        let migrated = legacy_migrated_path(original);
        assert_eq!(
            migrated,
            std::path::PathBuf::from("/data/request_history.json.migrated")
        );
    }

    // =====================================================================
    // 追加テスト: parse_legacy_records valid array
    // =====================================================================

    #[test]
    fn test_parse_legacy_records_valid_array() {
        let record = create_test_record(Utc::now());
        let json = serde_json::to_string(&vec![&record]).unwrap();
        let records = parse_legacy_records(&json).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, record.id);
    }

    // =====================================================================
    // 追加テスト: ClientIpFilter
    // =====================================================================

    #[test]
    fn test_client_ip_filter_exact_ipv4() {
        let f = ClientIpFilter::from_input("192.168.1.1");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "192.168.1.1"),
            _ => panic!("expected Exact"),
        }
    }

    #[test]
    fn test_client_ip_filter_exact_ipv6() {
        let f = ClientIpFilter::from_input("::1");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "::1"),
            _ => panic!("expected Exact"),
        }
    }

    #[test]
    fn test_client_ip_filter_ipv6_prefix64() {
        let f = ClientIpFilter::from_input("2001:db8::/64");
        match f {
            ClientIpFilter::Ipv6Prefix64(prefix) => assert!(prefix.contains("/64")),
            _ => panic!("expected Ipv6Prefix64"),
        }
    }

    #[test]
    fn test_client_ip_filter_invalid_input() {
        // Non-IP string treated as exact
        let f = ClientIpFilter::from_input("not-an-ip");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "not-an-ip"),
            _ => panic!("expected Exact"),
        }
    }

    // =====================================================================
    // 追加テスト: build_in_clause
    // =====================================================================

    #[test]
    fn test_build_in_clause() {
        let clause = build_in_clause("client_ip", 3);
        assert_eq!(clause, "client_ip IN (?, ?, ?)");
    }

    #[test]
    fn test_build_in_clause_single() {
        let clause = build_in_clause("col", 1);
        assert_eq!(clause, "col IN (?)");
    }

    // =====================================================================
    // 追加テスト: ClientDetail::empty
    // =====================================================================

    #[test]
    fn test_client_detail_empty() {
        let detail = ClientDetail::empty();
        assert_eq!(detail.total_requests, 0);
        assert!(detail.first_seen.is_none());
        assert!(detail.last_seen.is_none());
        assert!(detail.recent_requests.is_empty());
        assert!(detail.model_distribution.is_empty());
        assert_eq!(detail.hourly_pattern.len(), 24);
        for (i, hp) in detail.hourly_pattern.iter().enumerate() {
            assert_eq!(hp.hour, i as i64);
            assert_eq!(hp.count, 0);
        }
    }

    // =====================================================================
    // 追加テスト: HeatmapCell serialize
    // =====================================================================

    #[test]
    fn test_heatmap_cell_serialize() {
        let cell = HeatmapCell {
            day_of_week: 0,
            hour: 12,
            count: 42,
        };
        let json = serde_json::to_value(&cell).unwrap();
        assert_eq!(json["day_of_week"], 0);
        assert_eq!(json["hour"], 12);
        assert_eq!(json["count"], 42);
    }

    // =====================================================================
    // 追加テスト: DB操作 - filter_and_paginate with client_ip
    // =====================================================================

    #[tokio::test]
    async fn test_filter_by_client_ip() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut r1 = create_test_record(Utc::now());
        r1.client_ip = Some("10.0.0.1".parse().unwrap());
        storage.save_record(&r1).await.unwrap();

        let mut r2 = create_test_record(Utc::now() - Duration::seconds(1));
        r2.client_ip = Some("10.0.0.2".parse().unwrap());
        storage.save_record(&r2).await.unwrap();

        let filter = RecordFilter {
            client_ip: Some("10.0.0.1".to_string()),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 1);
        assert_eq!(
            result.records[0].client_ip,
            Some("10.0.0.1".parse().unwrap())
        );
    }

    // =====================================================================
    // 追加テスト: DB操作 - filter_and_paginate empty results
    // =====================================================================

    #[tokio::test]
    async fn test_filter_and_paginate_no_results() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let filter = RecordFilter {
            model: Some("nonexistent-model".to_string()),
            ..Default::default()
        };
        let result = storage.filter_and_paginate(&filter, 1, 10).await.unwrap();
        assert_eq!(result.total_count, 0);
        assert!(result.records.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - filter_and_paginate page 0
    // =====================================================================

    #[tokio::test]
    async fn test_filter_and_paginate_page_zero() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let record = create_test_record(Utc::now());
        storage.save_record(&record).await.unwrap();

        // page 0 should behave like page 1 (saturating_sub)
        let filter = RecordFilter::default();
        let result = storage.filter_and_paginate(&filter, 0, 10).await.unwrap();
        assert_eq!(result.records.len(), 1);
    }

    // =====================================================================
    // 追加テスト: DB操作 - multiple request types
    // =====================================================================

    #[tokio::test]
    async fn test_save_record_various_request_types() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let request_types = [
            RequestType::Chat,
            RequestType::Embeddings,
            RequestType::Transcription,
            RequestType::Speech,
            RequestType::ImageGeneration,
        ];

        for (i, rt) in request_types.iter().enumerate() {
            let mut record = create_test_record(Utc::now() - Duration::seconds(i as i64));
            record.request_type = *rt;
            storage.save_record(&record).await.unwrap();
        }

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 5);
    }

    // =====================================================================
    // 追加テスト: DB操作 - save record without response body
    // =====================================================================

    #[tokio::test]
    async fn test_save_record_without_response_body() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let mut record = create_test_record(Utc::now());
        record.response_body = None;
        storage.save_record(&record).await.unwrap();

        let loaded = storage.load_records().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].response_body.is_none());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_client_ip_ranking empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_client_ip_ranking_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let result = storage
            .get_client_ip_ranking(24, 1, 10, None)
            .await
            .unwrap();
        assert!(result.rankings.is_empty());
        assert_eq!(result.total_count, 0);
        assert_eq!(result.page, 1);
        assert_eq!(result.per_page, 10);
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_client_ip_ranking with data
    // =====================================================================

    #[tokio::test]
    async fn test_get_client_ip_ranking_with_data() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        // Insert records with different client IPs
        for i in 0..3 {
            let mut r = create_test_record(Utc::now() - Duration::seconds(i));
            r.client_ip = Some("10.0.0.1".parse().unwrap());
            storage.save_record(&r).await.unwrap();
        }
        let mut r2 = create_test_record(Utc::now() - Duration::seconds(10));
        r2.client_ip = Some("10.0.0.2".parse().unwrap());
        storage.save_record(&r2).await.unwrap();

        let result = storage
            .get_client_ip_ranking(24, 1, 10, None)
            .await
            .unwrap();
        assert_eq!(result.total_count, 2);
        // First should be the one with more requests
        assert_eq!(result.rankings[0].request_count, 3);
        assert_eq!(result.rankings[1].request_count, 1);
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_ip_request_counts_since
    // =====================================================================

    #[tokio::test]
    async fn test_get_ip_request_counts_since_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let counts = storage.get_ip_request_counts_since(24).await.unwrap();
        assert!(counts.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_request_heatmap
    // =====================================================================

    #[tokio::test]
    async fn test_get_request_heatmap_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let cells = storage.get_request_heatmap(168, None).await.unwrap();
        // Always 7*24 = 168 cells
        assert_eq!(cells.len(), 168);
        // All zero counts
        for cell in &cells {
            assert_eq!(cell.count, 0);
        }
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_unique_ip_timeline
    // =====================================================================

    #[tokio::test]
    async fn test_get_unique_ip_timeline_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let timeline = storage.get_unique_ip_timeline(24).await.unwrap();
        assert_eq!(timeline.len(), 24);
        for point in &timeline {
            assert_eq!(point.unique_ips, 0);
        }
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_model_distribution_by_clients empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_model_distribution_by_clients_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let dist = storage.get_model_distribution_by_clients(24).await.unwrap();
        assert!(dist.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_client_detail empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_client_detail_unknown_ip() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let detail = storage
            .get_client_detail("192.168.99.99", 10)
            .await
            .unwrap();
        assert_eq!(detail.total_requests, 0);
        assert!(detail.recent_requests.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - get_client_api_keys empty
    // =====================================================================

    #[tokio::test]
    async fn test_get_client_api_keys_empty() {
        let pool = create_test_pool().await;
        let storage = RequestHistoryStorage::new(pool);

        let keys = storage.get_client_api_keys("10.0.0.1").await.unwrap();
        assert!(keys.is_empty());
    }
}
