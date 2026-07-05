//! リクエスト履歴の書き込み経路（save / legacy import / INSERT 本体）。

use super::legacy::{legacy_migrated_path, legacy_request_history_path, parse_legacy_records};
use super::RequestHistoryStorage;
use crate::common::error::{LbError, RouterResult};
use crate::common::protocol::{RecordStatus, RequestResponseRecord};

impl RequestHistoryStorage {
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
}
