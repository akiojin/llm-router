//! APIキーの DB 行マッピングと権限（permissions）シリアライズ。
//!
//! `ApiKeyRow`（sqlx 行）から `ApiKey` への変換、および権限 JSON の
//! パース・シリアライズを集約する。CRUD 本体は親モジュールに残す。

use crate::common::auth::{ApiKey, ApiKeyPermission};
use crate::common::error::LbError;
use chrono::{DateTime, Utc};
use tracing::warn;
use uuid::Uuid;

// SQLiteからの行取得用の内部型
#[derive(sqlx::FromRow)]
pub(super) struct ApiKeyRow {
    id: String,
    key_hash: String,
    key_prefix: Option<String>,
    pub(super) name: String,
    created_by: String,
    created_at: String,
    expires_at: Option<String>,
    permissions: Option<String>,
}

impl ApiKeyRow {
    /// DB行を `ApiKey` へ変換する。
    ///
    /// 認証ホットパス（`find_by_hash`）からも呼ばれるため、不正な行データで
    /// panic させず `LbError` を返す。`expires_at` のパース失敗のみ無期限扱いで
    /// 緩和する（致命的でないため）。
    pub(super) fn into_api_key(self) -> Result<ApiKey, LbError> {
        let id = Uuid::parse_str(&self.id)
            .map_err(|e| LbError::Database(format!("Invalid API key id '{}': {}", self.id, e)))?;
        let created_by = Uuid::parse_str(&self.created_by).map_err(|e| {
            LbError::Database(format!(
                "Invalid API key created_by '{}': {}",
                self.created_by, e
            ))
        })?;
        let created_at = DateTime::parse_from_rfc3339(&self.created_at)
            .map_err(|e| {
                LbError::Database(format!(
                    "Invalid API key created_at '{}': {}",
                    self.created_at, e
                ))
            })?
            .with_timezone(&Utc);
        let expires_at = self.expires_at.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        let permissions = parse_permissions(self.permissions);

        Ok(ApiKey {
            id,
            key_hash: self.key_hash,
            key_prefix: self.key_prefix,
            name: self.name,
            created_by,
            created_at,
            expires_at,
            permissions,
        })
    }
}

pub(super) fn parse_permissions(permissions: Option<String>) -> Vec<ApiKeyPermission> {
    match permissions {
        None => {
            // Migration should backfill, but be safe: default-deny.
            warn!("API key permissions are NULL; treating as no permissions");
            Vec::new()
        }
        Some(raw) if raw.trim().is_empty() => {
            warn!("API key permissions are empty; treating as no permissions");
            Vec::new()
        }
        Some(raw) => match serde_json::from_str::<Vec<ApiKeyPermission>>(&raw) {
            Ok(permissions) => permissions,
            Err(err) => {
                warn!(
                    "Failed to parse API key permissions JSON; treating as no permissions: {}",
                    err
                );
                Vec::new()
            }
        },
    }
}

pub(super) fn serialize_permissions(permissions: &[ApiKeyPermission]) -> Result<String, LbError> {
    serde_json::to_string(permissions)
        .map_err(|e| LbError::Database(format!("Failed to serialize permissions: {}", e)))
}
