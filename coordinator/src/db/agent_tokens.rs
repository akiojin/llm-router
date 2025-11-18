// T055-T056: エージェントトークンCRUD操作とトークン生成

use chrono::{DateTime, Utc};
use ollama_coordinator_common::auth::{AgentToken, AgentTokenWithPlaintext};
use ollama_coordinator_common::error::CoordinatorError;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

/// エージェントトークンを生成
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `agent_id` - エージェントID
///
/// # Returns
/// * `Ok(AgentTokenWithPlaintext)` - 生成されたエージェントトークン（平文トークン含む）
/// * `Err(CoordinatorError)` - 生成失敗
pub async fn create(
    pool: &SqlitePool,
    agent_id: Uuid,
) -> Result<AgentTokenWithPlaintext, CoordinatorError> {
    let token = generate_agent_token();
    let token_hash = hash_with_sha256(&token);
    let created_at = Utc::now();

    sqlx::query(
        "INSERT INTO agent_tokens (agent_id, token_hash, created_at)
         VALUES (?, ?, ?)",
    )
    .bind(agent_id.to_string())
    .bind(&token_hash)
    .bind(created_at.to_rfc3339())
    .execute(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to create agent token: {}", e)))?;

    Ok(AgentTokenWithPlaintext {
        agent_id,
        token,
        created_at,
    })
}

/// ハッシュ値でエージェントトークンを検索
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `token_hash` - SHA-256ハッシュ
///
/// # Returns
/// * `Ok(Some(AgentToken))` - エージェントトークンが見つかった
/// * `Ok(None)` - エージェントトークンが見つからなかった
/// * `Err(CoordinatorError)` - 検索失敗
pub async fn find_by_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> Result<Option<AgentToken>, CoordinatorError> {
    let row = sqlx::query_as::<_, AgentTokenRow>(
        "SELECT agent_id, token_hash, created_at FROM agent_tokens WHERE token_hash = ?",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to find agent token: {}", e)))?;

    Ok(row.map(|r| r.into_agent_token()))
}

/// エージェントIDでトークンを検索
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `agent_id` - エージェントID
///
/// # Returns
/// * `Ok(Some(AgentToken))` - エージェントトークンが見つかった
/// * `Ok(None)` - エージェントトークンが見つからなかった
/// * `Err(CoordinatorError)` - 検索失敗
pub async fn find_by_agent_id(
    pool: &SqlitePool,
    agent_id: Uuid,
) -> Result<Option<AgentToken>, CoordinatorError> {
    let row = sqlx::query_as::<_, AgentTokenRow>(
        "SELECT agent_id, token_hash, created_at FROM agent_tokens WHERE agent_id = ?",
    )
    .bind(agent_id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to find agent token: {}", e)))?;

    Ok(row.map(|r| r.into_agent_token()))
}

/// エージェントトークンを削除
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `agent_id` - エージェントID
///
/// # Returns
/// * `Ok(())` - 削除成功
/// * `Err(CoordinatorError)` - 削除失敗
pub async fn delete(pool: &SqlitePool, agent_id: Uuid) -> Result<(), CoordinatorError> {
    sqlx::query("DELETE FROM agent_tokens WHERE agent_id = ?")
        .bind(agent_id.to_string())
        .execute(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to delete agent token: {}", e)))?;

    Ok(())
}

/// エージェントトークンを生成（`agt_` + UUID）
///
/// # Returns
/// * `String` - 生成されたエージェントトークン
fn generate_agent_token() -> String {
    let uuid = Uuid::new_v4();
    format!("agt_{}", uuid)
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
struct AgentTokenRow {
    agent_id: String,
    token_hash: String,
    created_at: String,
}

impl AgentTokenRow {
    fn into_agent_token(self) -> AgentToken {
        let agent_id = Uuid::parse_str(&self.agent_id).unwrap();
        let created_at = DateTime::parse_from_rfc3339(&self.created_at)
            .unwrap()
            .with_timezone(&Utc);

        AgentToken {
            agent_id,
            token_hash: self.token_hash,
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::initialize_database;

    async fn setup_test_db() -> SqlitePool {
        initialize_database("sqlite::memory:")
            .await
            .expect("Failed to initialize test database")
    }

    #[tokio::test]
    async fn test_generate_agent_token() {
        let token = generate_agent_token();
        assert!(token.starts_with("agt_"));
        // "agt_" + UUID（36文字）
        assert_eq!(token.len(), 4 + 36);
    }

    #[tokio::test]
    async fn test_create_and_find_agent_token() {
        let pool = setup_test_db().await;

        let agent_id = Uuid::new_v4();
        let token_with_plaintext = create(&pool, agent_id)
            .await
            .expect("Failed to create agent token");

        assert!(token_with_plaintext.token.starts_with("agt_"));
        assert_eq!(token_with_plaintext.agent_id, agent_id);

        // ハッシュで検索
        let token_hash = hash_with_sha256(&token_with_plaintext.token);
        let found = find_by_hash(&pool, &token_hash)
            .await
            .expect("Failed to find agent token");

        assert!(found.is_some());
        let found_token = found.unwrap();
        assert_eq!(found_token.agent_id, agent_id);
    }

    #[tokio::test]
    async fn test_find_by_agent_id() {
        let pool = setup_test_db().await;

        let agent_id = Uuid::new_v4();
        create(&pool, agent_id).await.unwrap();

        let found = find_by_agent_id(&pool, agent_id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().agent_id, agent_id);
    }

    #[tokio::test]
    async fn test_delete_agent_token() {
        let pool = setup_test_db().await;

        let agent_id = Uuid::new_v4();
        let token = create(&pool, agent_id).await.unwrap();

        delete(&pool, agent_id).await.unwrap();

        let token_hash = hash_with_sha256(&token.token);
        let found = find_by_hash(&pool, &token_hash).await.unwrap();
        assert!(found.is_none());
    }
}
