// T040-T041: データベースマイグレーション実行とJSONインポート

use ollama_coordinator_common::error::CoordinatorError;
use sqlx::{migrate::MigrateDatabase, Sqlite, SqlitePool};
use std::path::Path;

/// SQLiteデータベース接続プールを作成してマイグレーションを実行
///
/// # Arguments
/// * `database_url` - データベースURL（例: "sqlite:data/coordinator.db"）
///
/// # Returns
/// * `Ok(SqlitePool)` - 初期化済みデータベースプール
/// * `Err(CoordinatorError)` - 初期化失敗
pub async fn initialize_database(database_url: &str) -> Result<SqlitePool, CoordinatorError> {
    // データベースファイルが存在しない場合は作成
    if !Sqlite::database_exists(database_url)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to check database: {}", e)))?
    {
        tracing::info!("Creating database: {}", database_url);
        Sqlite::create_database(database_url)
            .await
            .map_err(|e| CoordinatorError::Database(format!("Failed to create database: {}", e)))?;
    }

    // 接続プールを作成
    let pool = SqlitePool::connect(database_url)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to connect to database: {}", e)))?;

    // マイグレーションを実行
    run_migrations(&pool).await?;

    Ok(pool)
}

/// マイグレーションを実行（sqlx::migrate!マクロを使用）
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(())` - マイグレーション成功
/// * `Err(CoordinatorError)` - マイグレーション失敗
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), CoordinatorError> {
    tracing::info!("Running database migrations");

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to run migrations: {}", e)))?;

    tracing::info!("Database migrations completed successfully");
    Ok(())
}

/// JSONファイルからエージェントデータをインポート（マイグレーション用）
///
/// 注: この機能は将来的にエージェントデータもSQLiteに移行する際に使用
/// 現在のところ、認証機能はエージェントデータとは独立して動作
///
/// # Arguments
/// * `json_path` - agents.jsonのパス
///
/// # Returns
/// * `Ok(())` - インポート成功、元ファイルを.migratedにリネーム
/// * `Err(CoordinatorError)` - インポート失敗
pub async fn import_agents_from_json(json_path: &str) -> Result<(), CoordinatorError> {
    let path = Path::new(json_path);

    // ファイルが存在しない場合はスキップ
    if !path.exists() {
        tracing::info!("No agents.json found at {}, skipping import", json_path);
        return Ok(());
    }

    // TODO: 将来的にエージェントデータをSQLiteに移行する場合、ここで実装
    // 現在は認証機能のみSQLiteを使用し、エージェントデータは既存のJSONベース実装を継続

    tracing::info!("Agent data import not yet implemented (agents remain in JSON format)");

    // マイグレーション完了マーク（ファイルリネーム）
    let migrated_path = format!("{}.migrated", json_path);
    if let Err(e) = std::fs::rename(path, &migrated_path) {
        tracing::warn!("Failed to rename {} to {}: {}", json_path, migrated_path, e);
    } else {
        tracing::info!("Renamed {} to {}", json_path, migrated_path);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_initialize_database() {
        // テスト用の一時データベース
        let db_url = "sqlite::memory:";

        let pool = initialize_database(db_url)
            .await
            .expect("Failed to initialize database");

        // usersテーブルが作成されているか確認
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'")
                .fetch_one(&pool)
                .await;

        assert!(result.is_ok(), "users table should exist");
    }

    #[tokio::test]
    async fn test_run_migrations() {
        let db_url = "sqlite::memory:";
        let pool = SqlitePool::connect(db_url)
            .await
            .expect("Failed to connect");

        run_migrations(&pool)
            .await
            .expect("Failed to run migrations");

        // api_keysテーブルが作成されているか確認
        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='api_keys'")
                .fetch_one(&pool)
                .await;

        assert!(result.is_ok(), "api_keys table should exist");
    }

    #[tokio::test]
    async fn test_import_agents_from_json_no_file() {
        // 存在しないファイルの場合はエラーなく完了
        let result = import_agents_from_json("/nonexistent/agents.json").await;
        assert!(result.is_ok());
    }
}
