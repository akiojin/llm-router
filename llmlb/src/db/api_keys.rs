// T053-T054: APIキーCRUD操作とキー生成

use crate::common::auth::{ApiKey, ApiKeyPermission, ApiKeyWithPlaintext};
use crate::common::error::{CommonError, LbError};
use chrono::{DateTime, Utc};
use rand::RngExt;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

mod rowmap;
#[cfg(test)]
use rowmap::parse_permissions;
use rowmap::{serialize_permissions, ApiKeyRow};

const DUPLICATE_NAME_VALIDATION_MSG: &str = "API key with this name already exists";

/// APIキーを生成
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `name` - APIキーの説明
/// * `created_by` - 発行したユーザーID
/// * `expires_at` - 有効期限（Noneの場合は無期限）
///
/// # Returns
/// * `Ok(ApiKeyWithPlaintext)` - 生成されたAPIキー（平文キー含む）
/// * `Err(LbError)` - 生成失敗
pub async fn create(
    pool: &SqlitePool,
    name: &str,
    created_by: Uuid,
    expires_at: Option<DateTime<Utc>>,
    permissions: Vec<ApiKeyPermission>,
) -> Result<ApiKeyWithPlaintext, LbError> {
    let id = Uuid::new_v4();
    let key = generate_api_key();
    let key_hash = hash_with_sha256(&key);
    let key_prefix = key.chars().take(10).collect::<String>();
    let created_at = Utc::now();

    let permissions_json = serialize_permissions(&permissions)?;

    sqlx::query(
        "INSERT INTO api_keys (id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(name)
    .bind(created_by.to_string())
    .bind(created_at.to_rfc3339())
    .bind(expires_at.map(|dt| dt.to_rfc3339()))
    .bind(permissions_json)
    .execute(pool)
    .await
    .map_err(|e| map_write_error(e, name, "create API key"))?;

    Ok(ApiKeyWithPlaintext {
        id,
        key,
        key_prefix,
        name: name.to_string(),
        created_at,
        expires_at,
        permissions,
    })
}

/// ハッシュ値でAPIキーを検索
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `key_hash` - SHA-256ハッシュ
///
/// # Returns
/// * `Ok(Some(ApiKey))` - APIキーが見つかった
/// * `Ok(None)` - APIキーが見つからなかった
/// * `Err(LbError)` - 検索失敗
pub async fn find_by_hash(pool: &SqlitePool, key_hash: &str) -> Result<Option<ApiKey>, LbError> {
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions FROM api_keys WHERE key_hash = ?"
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find API key: {}", e)))?;

    row.map(|r| r.into_api_key()).transpose()
}

/// すべてのAPIキーを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(Vec<ApiKey>)` - APIキー一覧
/// * `Err(LbError)` - 取得失敗
pub async fn list(pool: &SqlitePool) -> Result<Vec<ApiKey>, LbError> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions FROM api_keys ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to list API keys: {}", e)))?;

    rows.into_iter().map(|r| r.into_api_key()).collect()
}

/// 指定ユーザーが発行したAPIキーを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `created_by` - 発行者ユーザーID
///
/// # Returns
/// * `Ok(Vec<ApiKey>)` - APIキー一覧
/// * `Err(LbError)` - 取得失敗
pub async fn list_by_creator(pool: &SqlitePool, created_by: Uuid) -> Result<Vec<ApiKey>, LbError> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions
         FROM api_keys
         WHERE created_by = ?
         ORDER BY created_at DESC",
    )
    .bind(created_by.to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to list API keys by creator: {}", e)))?;

    rows.into_iter().map(|r| r.into_api_key()).collect()
}

/// APIキーを更新（名前と有効期限）
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - APIキーID
/// * `name` - 新しい名前
/// * `expires_at` - 新しい有効期限（Noneの場合は無期限）
///
/// # Returns
/// * `Ok(Some(ApiKey))` - 更新後のAPIキー
/// * `Ok(None)` - APIキーが見つからなかった
/// * `Err(LbError)` - 更新失敗
pub async fn update(
    pool: &SqlitePool,
    id: Uuid,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<ApiKey>, LbError> {
    let result = sqlx::query("UPDATE api_keys SET name = ?, expires_at = ? WHERE id = ?")
        .bind(name)
        .bind(expires_at.map(|dt| dt.to_rfc3339()))
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| map_write_error(e, name, "update API key"))?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    // 更新後のAPIキーを取得
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions FROM api_keys WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find updated API key: {}", e)))?;

    row.map(|r| r.into_api_key()).transpose()
}

/// APIキーを更新（名前と有効期限、発行者限定）
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - APIキーID
/// * `created_by` - 発行者ユーザーID
/// * `name` - 新しい名前
/// * `expires_at` - 新しい有効期限（Noneの場合は無期限）
///
/// # Returns
/// * `Ok(Some(ApiKey))` - 更新後のAPIキー
/// * `Ok(None)` - APIキーが見つからなかった
/// * `Err(LbError)` - 更新失敗
pub async fn update_by_creator(
    pool: &SqlitePool,
    id: Uuid,
    created_by: Uuid,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<ApiKey>, LbError> {
    let existing = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions
         FROM api_keys
         WHERE id = ? AND created_by = ?",
    )
    .bind(id.to_string())
    .bind(created_by.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find API key by creator: {}", e)))?;

    let Some(existing) = existing else {
        return Ok(None);
    };

    let expires_at = expires_at.map(|dt| dt.to_rfc3339());
    let result = if existing.name == name {
        sqlx::query(
            "UPDATE api_keys
             SET expires_at = ?
             WHERE id = ? AND created_by = ?",
        )
        .bind(expires_at.clone())
        .bind(id.to_string())
        .bind(created_by.to_string())
        .execute(pool)
        .await
        .map_err(|e| map_write_error(e, name, "update API key expiry by creator"))?
    } else {
        sqlx::query(
            "UPDATE api_keys
             SET name = ?, expires_at = ?
             WHERE id = ? AND created_by = ?",
        )
        .bind(name)
        .bind(expires_at)
        .bind(id.to_string())
        .bind(created_by.to_string())
        .execute(pool)
        .await
        .map_err(|e| map_write_error(e, name, "update API key by creator"))?
    };

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, key_prefix, name, created_by, created_at, expires_at, permissions
         FROM api_keys
         WHERE id = ? AND created_by = ?",
    )
    .bind(id.to_string())
    .bind(created_by.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find updated API key by creator: {}", e)))?;

    row.map(|r| r.into_api_key()).transpose()
}

/// APIキーを削除
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - APIキーID
///
/// # Returns
/// * `Ok(())` - 削除成功
/// * `Err(LbError)` - 削除失敗
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), LbError> {
    sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete API key: {}", e)))?;

    Ok(())
}

/// APIキーを削除（発行者限定）
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - APIキーID
/// * `created_by` - 発行者ユーザーID
///
/// # Returns
/// * `Ok(true)` - 削除成功
/// * `Ok(false)` - 削除対象なし
/// * `Err(LbError)` - 削除失敗
pub async fn delete_by_creator(
    pool: &SqlitePool,
    id: Uuid,
    created_by: Uuid,
) -> Result<bool, LbError> {
    let result = sqlx::query("DELETE FROM api_keys WHERE id = ? AND created_by = ?")
        .bind(id.to_string())
        .bind(created_by.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete API key by creator: {}", e)))?;

    Ok(result.rows_affected() > 0)
}

/// APIキーを生成（`sk_` + 32文字のランダム英数字）
///
/// # Returns
/// * `String` - 生成されたAPIキー
fn generate_api_key() -> String {
    let charset: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();

    let random_part: String = (0..32)
        .map(|_| {
            let idx = rng.random_range(0..charset.len());
            charset[idx] as char
        })
        .collect();

    format!("sk_{}", random_part)
}

/// SHA-256ハッシュ化ヘルパー関数
///
/// # Arguments
/// * `input` - ハッシュ化する文字列
///
/// # Returns
/// * `String` - 16進数表現のSHA-256ハッシュ（64文字）
fn hash_with_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_duplicate_name_violation(err: &sqlx::Error) -> bool {
    let message = err.to_string();
    message.contains("API key name already exists for this user")
        || message.contains("UNIQUE constraint failed: api_keys.created_by, api_keys.name")
}

fn map_write_error(err: sqlx::Error, name: &str, operation: &str) -> LbError {
    if is_duplicate_name_violation(&err) {
        return LbError::Common(CommonError::Validation(format!(
            "{}: '{}'",
            DUPLICATE_NAME_VALIDATION_MSG, name
        )));
    }

    LbError::Database(format!("Failed to {}: {}", operation, err))
}

#[cfg(test)]
mod tests;
