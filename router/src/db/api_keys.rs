// T053-T054: APIキーCRUD操作とキー生成

use chrono::{DateTime, Utc};
use llm_router_common::auth::{ApiKey, ApiKeyWithPlaintext};
use llm_router_common::error::RouterError;
use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

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
/// * `Err(RouterError)` - 生成失敗
pub async fn create(
    pool: &SqlitePool,
    name: &str,
    created_by: Uuid,
    expires_at: Option<DateTime<Utc>>,
) -> Result<ApiKeyWithPlaintext, RouterError> {
    let id = Uuid::new_v4();
    let key = generate_api_key();
    let key_hash = hash_with_sha256(&key);
    let created_at = Utc::now();

    sqlx::query(
        "INSERT INTO api_keys (id, key_hash, name, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(&key_hash)
    .bind(name)
    .bind(created_by.to_string())
    .bind(created_at.to_rfc3339())
    .bind(expires_at.map(|dt| dt.to_rfc3339()))
    .execute(pool)
    .await
    .map_err(|e| RouterError::Database(format!("Failed to create API key: {}", e)))?;

    Ok(ApiKeyWithPlaintext {
        id,
        key,
        name: name.to_string(),
        created_at,
        expires_at,
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
/// * `Err(RouterError)` - 検索失敗
pub async fn find_by_hash(
    pool: &SqlitePool,
    key_hash: &str,
) -> Result<Option<ApiKey>, RouterError> {
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, name, created_by, created_at, expires_at FROM api_keys WHERE key_hash = ?"
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| RouterError::Database(format!("Failed to find API key: {}", e)))?;

    Ok(row.map(|r| r.into_api_key()))
}

/// すべてのAPIキーを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(Vec<ApiKey>)` - APIキー一覧
/// * `Err(RouterError)` - 取得失敗
pub async fn list(pool: &SqlitePool) -> Result<Vec<ApiKey>, RouterError> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, name, created_by, created_at, expires_at FROM api_keys ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| RouterError::Database(format!("Failed to list API keys: {}", e)))?;

    Ok(rows.into_iter().map(|r| r.into_api_key()).collect())
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
/// * `Err(RouterError)` - 更新失敗
pub async fn update(
    pool: &SqlitePool,
    id: Uuid,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<ApiKey>, RouterError> {
    let result = sqlx::query("UPDATE api_keys SET name = ?, expires_at = ? WHERE id = ?")
        .bind(name)
        .bind(expires_at.map(|dt| dt.to_rfc3339()))
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| RouterError::Database(format!("Failed to update API key: {}", e)))?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    // 更新後のAPIキーを取得
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, key_hash, name, created_by, created_at, expires_at FROM api_keys WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| RouterError::Database(format!("Failed to find updated API key: {}", e)))?;

    Ok(row.map(|r| r.into_api_key()))
}

/// APIキーを削除
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - APIキーID
///
/// # Returns
/// * `Ok(())` - 削除成功
/// * `Err(RouterError)` - 削除失敗
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), RouterError> {
    sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| RouterError::Database(format!("Failed to delete API key: {}", e)))?;

    Ok(())
}

/// APIキーを生成（`sk_` + 32文字のランダム英数字）
///
/// # Returns
/// * `String` - 生成されたAPIキー
fn generate_api_key() -> String {
    let charset: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();

    let random_part: String = (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
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
    format!("{:x}", result)
}

// SQLiteからの行取得用の内部型
#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: String,
    key_hash: String,
    name: String,
    created_by: String,
    created_at: String,
    expires_at: Option<String>,
}

impl ApiKeyRow {
    fn into_api_key(self) -> ApiKey {
        let id = Uuid::parse_str(&self.id).unwrap();
        let created_by = Uuid::parse_str(&self.created_by).unwrap();
        let created_at = DateTime::parse_from_rfc3339(&self.created_at)
            .unwrap()
            .with_timezone(&Utc);
        let expires_at = self.expires_at.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        ApiKey {
            id,
            key_hash: self.key_hash,
            name: self.name,
            created_by,
            created_at,
            expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::initialize_database;
    use crate::db::users;
    use llm_router_common::auth::UserRole;

    async fn setup_test_db() -> SqlitePool {
        initialize_database("sqlite::memory:")
            .await
            .expect("Failed to initialize test database")
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
        let user = users::create(&pool, "testuser", "hash", UserRole::Admin)
            .await
            .unwrap();

        // APIキーを作成
        let api_key_with_plaintext = create(&pool, "Test API Key", user.id, None)
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
    }

    #[tokio::test]
    async fn test_list_api_keys() {
        let pool = setup_test_db().await;

        let user = users::create(&pool, "testuser", "hash", UserRole::Admin)
            .await
            .unwrap();

        create(&pool, "Key 1", user.id, None).await.unwrap();
        create(&pool, "Key 2", user.id, None).await.unwrap();

        let keys = list(&pool).await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_api_key() {
        let pool = setup_test_db().await;

        let user = users::create(&pool, "testuser", "hash", UserRole::Admin)
            .await
            .unwrap();

        let api_key = create(&pool, "Test Key", user.id, None).await.unwrap();

        delete(&pool, api_key.id).await.unwrap();

        let key_hash = hash_with_sha256(&api_key.key);
        let found = find_by_hash(&pool, &key_hash).await.unwrap();
        assert!(found.is_none());
    }
}
