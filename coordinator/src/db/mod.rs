//! データベースアクセス層
//!
//! SQLiteデータベースへの接続とクエリ実行

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use ollama_coordinator_common::error::{CoordinatorError, CoordinatorResult};

/// データベース接続プールを作成
pub async fn create_pool(database_url: &str) -> CoordinatorResult<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|e| CoordinatorError::Database(e.to_string()))?;

    // マイグレーション実行
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Migration failed: {}", e)))?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_pool_with_invalid_url() {
        let result = create_pool("invalid://url").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoordinatorError::Database(_)));
    }
}
