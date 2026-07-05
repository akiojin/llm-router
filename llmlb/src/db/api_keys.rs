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
mod tests {
    use super::*;
    use crate::common::auth::UserRole;
    use crate::db::users;

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[test]
    fn parse_permissions_handles_null_and_invalid_values() {
        assert!(parse_permissions(None).is_empty());
        assert!(parse_permissions(Some("".to_string())).is_empty());
        assert!(parse_permissions(Some("not-json".to_string())).is_empty());
    }

    #[test]
    fn parse_permissions_parses_valid_json() {
        let raw = serde_json::to_string(&vec![
            crate::common::auth::ApiKeyPermission::OpenaiInference,
        ])
        .unwrap();
        assert_eq!(
            parse_permissions(Some(raw)),
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference]
        );
    }

    #[tokio::test]
    async fn test_generate_api_key() {
        let key = generate_api_key();
        assert!(key.starts_with("sk_"));
        assert_eq!(key.len(), 3 + 32); // "sk_" + 32文字
    }

    #[tokio::test]
    async fn test_create_and_find_api_key() {
        let pool = setup_test_db().await;

        // テスト用ユーザーを作成
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        // APIキーを作成
        let api_key_with_plaintext = create(
            &pool,
            "Test API Key",
            user.id,
            None,
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference],
        )
        .await
        .expect("Failed to create API key");

        assert!(api_key_with_plaintext.key.starts_with("sk_"));
        assert_eq!(api_key_with_plaintext.name, "Test API Key");

        // ハッシュで検索
        let key_hash = hash_with_sha256(&api_key_with_plaintext.key);
        let found = find_by_hash(&pool, &key_hash)
            .await
            .expect("Failed to find API key");

        assert!(found.is_some());
        let found_key = found.unwrap();
        assert_eq!(found_key.name, "Test API Key");
        assert_eq!(found_key.created_by, user.id);
        assert_eq!(
            found_key.permissions,
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference]
        );
    }

    #[tokio::test]
    async fn test_list_api_keys() {
        let pool = setup_test_db().await;

        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        create(
            &pool,
            "Key 1",
            user.id,
            None,
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();
        create(
            &pool,
            "Key 2",
            user.id,
            None,
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let keys = list(&pool).await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn test_create_duplicate_name_same_creator_rejected() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        create(
            &pool,
            "Duplicate Key",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let err = create(
            &pool,
            "Duplicate Key",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .expect_err("duplicate key name should be rejected");

        assert!(matches!(err, LbError::Common(CommonError::Validation(_))));
    }

    #[tokio::test]
    async fn test_create_same_name_different_creators_allowed() {
        let pool = setup_test_db().await;
        let user_a = users::create(&pool, "user_a", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        let user_b = users::create(&pool, "user_b", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        create(
            &pool,
            "Shared Name",
            user_a.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let key_b = create(
            &pool,
            "Shared Name",
            user_b.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await;

        assert!(
            key_b.is_ok(),
            "different creators should be able to use the same key name"
        );
    }

    #[tokio::test]
    async fn test_delete_api_key() {
        let pool = setup_test_db().await;

        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let api_key = create(
            &pool,
            "Test Key",
            user.id,
            None,
            vec![crate::common::auth::ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        delete(&pool, api_key.id).await.unwrap();

        let key_hash = hash_with_sha256(&api_key.key);
        let found = find_by_hash(&pool, &key_hash).await.unwrap();
        assert!(found.is_none());
    }

    // --- 追加テスト ---

    #[test]
    fn hash_with_sha256_deterministic() {
        let h1 = hash_with_sha256("test-input");
        let h2 = hash_with_sha256("test-input");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn hash_with_sha256_different_inputs_differ() {
        let h1 = hash_with_sha256("input-a");
        let h2 = hash_with_sha256("input-b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn serialize_permissions_roundtrip() {
        let perms = vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::EndpointsRead,
        ];
        let json = serialize_permissions(&perms).unwrap();
        let back: Vec<ApiKeyPermission> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, perms);
    }

    #[tokio::test]
    async fn test_find_by_hash_not_found() {
        let pool = setup_test_db().await;
        let result = find_by_hash(&pool, "nonexistent_hash").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_create_with_expiry() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let expiry = Utc::now() + chrono::Duration::days(30);
        let key = create(
            &pool,
            "Expiring Key",
            user.id,
            Some(expiry),
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let hash = hash_with_sha256(&key.key);
        let found = find_by_hash(&pool, &hash).await.unwrap().unwrap();
        assert!(found.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_create_with_multiple_permissions() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let perms = vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::MetricsRead,
        ];
        let key = create(&pool, "Multi Perm", user.id, None, perms.clone())
            .await
            .unwrap();

        let hash = hash_with_sha256(&key.key);
        let found = find_by_hash(&pool, &hash).await.unwrap().unwrap();
        assert_eq!(found.permissions, perms);
    }

    #[tokio::test]
    async fn test_update_api_key() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let key = create(
            &pool,
            "Original Name",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let updated = update(&pool, key.id, "New Name", None).await.unwrap();
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().name, "New Name");
    }

    #[tokio::test]
    async fn test_update_nonexistent_key_returns_none() {
        let pool = setup_test_db().await;
        let result = update(&pool, Uuid::new_v4(), "name", None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_by_creator() {
        let pool = setup_test_db().await;
        let user_a = users::create(&pool, "user_a", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        let user_b = users::create(&pool, "user_b", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        create(
            &pool,
            "Key A",
            user_a.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();
        create(
            &pool,
            "Key B",
            user_b.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let a_keys = list_by_creator(&pool, user_a.id).await.unwrap();
        assert_eq!(a_keys.len(), 1);
        assert_eq!(a_keys[0].name, "Key A");

        let b_keys = list_by_creator(&pool, user_b.id).await.unwrap();
        assert_eq!(b_keys.len(), 1);
        assert_eq!(b_keys[0].name, "Key B");
    }

    #[tokio::test]
    async fn test_delete_by_creator() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        let other = users::create(&pool, "other", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        let key = create(
            &pool,
            "Owner Key",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        // Other user cannot delete
        let not_deleted = delete_by_creator(&pool, key.id, other.id).await.unwrap();
        assert!(!not_deleted);

        // Owner can delete
        let deleted = delete_by_creator(&pool, key.id, user.id).await.unwrap();
        assert!(deleted);

        let keys = list(&pool).await.unwrap();
        assert!(keys.is_empty());
    }

    // =====================================================================
    // 追加テスト: generate_api_key
    // =====================================================================

    #[test]
    fn test_generate_api_key_format() {
        let key = generate_api_key();
        assert!(key.starts_with("sk_"));
        assert_eq!(key.len(), 35); // "sk_" (3) + 32 chars

        // All characters after prefix are alphanumeric
        let suffix = &key[3..];
        assert!(suffix.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_api_key_uniqueness() {
        let key1 = generate_api_key();
        let key2 = generate_api_key();
        assert_ne!(key1, key2);
    }

    // =====================================================================
    // 追加テスト: hash_with_sha256
    // =====================================================================

    #[test]
    fn test_hash_with_sha256_length() {
        let hash = hash_with_sha256("test");
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_hash_with_sha256_hex_chars() {
        let hash = hash_with_sha256("test");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_with_sha256_empty_input() {
        let hash = hash_with_sha256("");
        assert_eq!(hash.len(), 64);
        // SHA-256 of empty string is well-known
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // =====================================================================
    // 追加テスト: parse_permissions edge cases
    // =====================================================================

    #[test]
    fn test_parse_permissions_whitespace_only() {
        assert!(parse_permissions(Some("   ".to_string())).is_empty());
    }

    #[test]
    fn test_parse_permissions_empty_array() {
        let raw = serde_json::to_string(&Vec::<ApiKeyPermission>::new()).unwrap();
        assert!(parse_permissions(Some(raw)).is_empty());
    }

    #[test]
    fn test_parse_permissions_all_permissions() {
        let all = ApiKeyPermission::all();
        let raw = serde_json::to_string(&all).unwrap();
        let parsed = parse_permissions(Some(raw));
        assert_eq!(parsed.len(), all.len());
    }

    // =====================================================================
    // 追加テスト: serialize_permissions
    // =====================================================================

    #[test]
    fn test_serialize_permissions_empty() {
        let json = serialize_permissions(&[]).unwrap();
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_serialize_permissions_single() {
        let perms = vec![ApiKeyPermission::OpenaiInference];
        let json = serialize_permissions(&perms).unwrap();
        let back: Vec<ApiKeyPermission> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, perms);
    }

    // =====================================================================
    // 追加テスト: DB操作 - key_prefix
    // =====================================================================

    #[tokio::test]
    async fn test_api_key_prefix_is_first_10_chars() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let api_key = create(
            &pool,
            "Test",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let expected_prefix: String = api_key.key.chars().take(10).collect();
        assert_eq!(api_key.key_prefix, expected_prefix);

        // Verify it's stored in DB
        let key_hash = hash_with_sha256(&api_key.key);
        let found = find_by_hash(&pool, &key_hash).await.unwrap().unwrap();
        assert_eq!(found.key_prefix, Some(expected_prefix));
    }

    // =====================================================================
    // 追加テスト: DB操作 - update_by_creator
    // =====================================================================

    #[tokio::test]
    async fn test_update_by_creator_success() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let key = create(
            &pool,
            "Original",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let expiry = Utc::now() + chrono::Duration::days(7);
        let updated = update_by_creator(&pool, key.id, user.id, "Updated", Some(expiry))
            .await
            .unwrap();
        assert!(updated.is_some());
        let updated = updated.unwrap();
        assert_eq!(updated.name, "Updated");
        assert!(updated.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_update_by_creator_wrong_owner() {
        let pool = setup_test_db().await;
        let owner = users::create(&pool, "owner", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        let other = users::create(&pool, "other", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        let key = create(
            &pool,
            "Key",
            owner.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        // Wrong owner can't update
        let result = update_by_creator(&pool, key.id, other.id, "Hacked", None)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_by_creator_duplicate_name_rejected() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        create(
            &pool,
            "Key-A",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let key_b = create(
            &pool,
            "Key-B",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let err = update_by_creator(&pool, key_b.id, user.id, "Key-A", None)
            .await
            .expect_err("duplicate rename should be rejected");
        assert!(matches!(err, LbError::Common(CommonError::Validation(_))));
    }

    #[tokio::test]
    async fn test_update_by_creator_allows_expiry_change_for_legacy_duplicate_name() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        create(
            &pool,
            "Legacy Name",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let duplicate = create(
            &pool,
            "Temporary Name",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        sqlx::query("DROP TRIGGER IF EXISTS trg_api_keys_reject_duplicate_name_update")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("UPDATE api_keys SET name = ? WHERE id = ?")
            .bind("Legacy Name")
            .bind(duplicate.id.to_string())
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "CREATE TRIGGER IF NOT EXISTS trg_api_keys_reject_duplicate_name_update
             BEFORE UPDATE OF name, created_by ON api_keys
             FOR EACH ROW
             WHEN EXISTS (
                 SELECT 1
                 FROM api_keys
                 WHERE created_by = NEW.created_by
                  AND name = NEW.name
                  AND id <> OLD.id
             )
             BEGIN
                 SELECT RAISE(ABORT, 'API key name already exists for this user');
             END;",
        )
        .execute(&pool)
        .await
        .unwrap();

        let expiry = Utc::now() + chrono::Duration::days(14);
        let updated = update_by_creator(&pool, duplicate.id, user.id, "Legacy Name", Some(expiry))
            .await
            .expect("expiry-only update should work for legacy duplicates")
            .expect("legacy duplicate row should still exist");

        assert_eq!(updated.name, "Legacy Name");
        assert_eq!(updated.id, duplicate.id);
        assert_eq!(updated.expires_at, Some(expiry));
    }

    #[tokio::test]
    async fn test_update_by_creator_nonexistent_key() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "user", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let result = update_by_creator(&pool, Uuid::new_v4(), user.id, "Name", None)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // =====================================================================
    // 追加テスト: DB操作 - update with expiry change
    // =====================================================================

    #[tokio::test]
    async fn test_update_add_and_remove_expiry() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        // Create without expiry
        let key = create(
            &pool,
            "Key",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        // Add expiry
        let expiry = Utc::now() + chrono::Duration::days(30);
        let updated = update(&pool, key.id, "Key", Some(expiry)).await.unwrap();
        assert!(updated.unwrap().expires_at.is_some());

        // Remove expiry
        let updated = update(&pool, key.id, "Key", None).await.unwrap();
        assert!(updated.unwrap().expires_at.is_none());
    }

    // =====================================================================
    // 追加テスト: DB操作 - delete nonexistent key
    // =====================================================================

    #[tokio::test]
    async fn test_delete_nonexistent_key_is_noop() {
        let pool = setup_test_db().await;
        // Should not error
        delete(&pool, Uuid::new_v4()).await.unwrap();
    }

    // =====================================================================
    // 追加テスト: DB操作 - delete_by_creator nonexistent key
    // =====================================================================

    #[tokio::test]
    async fn test_delete_by_creator_nonexistent_key() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "user", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let deleted = delete_by_creator(&pool, Uuid::new_v4(), user.id)
            .await
            .unwrap();
        assert!(!deleted);
    }

    // =====================================================================
    // 追加テスト: DB操作 - list_by_creator empty
    // =====================================================================

    #[tokio::test]
    async fn test_list_by_creator_empty() {
        let pool = setup_test_db().await;
        let keys = list_by_creator(&pool, Uuid::new_v4()).await.unwrap();
        assert!(keys.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - list empty db
    // =====================================================================

    #[tokio::test]
    async fn test_list_empty_db() {
        let pool = setup_test_db().await;
        let keys = list(&pool).await.unwrap();
        assert!(keys.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - create with no permissions
    // =====================================================================

    #[tokio::test]
    async fn test_create_with_no_permissions() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        let key = create(&pool, "No Perm Key", user.id, None, vec![])
            .await
            .unwrap();

        let hash = hash_with_sha256(&key.key);
        let found = find_by_hash(&pool, &hash).await.unwrap().unwrap();
        assert!(found.permissions.is_empty());
    }

    // =====================================================================
    // 追加テスト: DB操作 - multiple keys ordering
    // =====================================================================

    #[tokio::test]
    async fn test_list_ordering_by_created_at_desc() {
        let pool = setup_test_db().await;
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        create(
            &pool,
            "Key 1",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        // Small delay to ensure different created_at
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        create(
            &pool,
            "Key 2",
            user.id,
            None,
            vec![ApiKeyPermission::OpenaiInference],
        )
        .await
        .unwrap();

        let keys = list(&pool).await.unwrap();
        assert_eq!(keys.len(), 2);
        // DESC order: Key 2 first
        assert_eq!(keys[0].name, "Key 2");
        assert_eq!(keys[1].name, "Key 1");
    }
}
