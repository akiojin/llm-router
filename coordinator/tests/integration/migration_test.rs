//! データベースマイグレーション統合テスト
//!
//! T014: JSONからSQLiteへのマイグレーション

use ollama_coordinator_coordinator::db::migrations::{
    import_agents_from_json, initialize_database,
};

/// T014: JSONからSQLiteへのマイグレーションテスト
#[tokio::test]
async fn test_json_to_sqlite_migration() {
    // テスト用の一時データベース（メモリ内）
    let db_url = "sqlite::memory:";

    // データベースを初期化してマイグレーションを実行
    let pool = initialize_database(db_url)
        .await
        .expect("Failed to initialize database");

    // usersテーブルが作成されているか確認
    let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'")
        .fetch_one(&pool)
        .await;
    assert!(result.is_ok(), "users table should be created");

    // api_keysテーブルが作成されているか確認
    let result =
        sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='api_keys'")
            .fetch_one(&pool)
            .await;
    assert!(result.is_ok(), "api_keys table should be created");

    // agent_tokensテーブルが作成されているか確認
    let result =
        sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='agent_tokens'")
            .fetch_one(&pool)
            .await;
    assert!(result.is_ok(), "agent_tokens table should be created");

    // JSONインポート（存在しないファイルでもエラーにならないことを確認）
    let result = import_agents_from_json("/nonexistent/agents.json").await;
    assert!(
        result.is_ok(),
        "Import should succeed even without JSON file"
    );
}

/// T014: SQLiteスキーマ作成テスト
#[tokio::test]
async fn test_sqlite_schema_creation() {
    // テスト用の一時データベース
    let db_url = "sqlite::memory:";

    // データベースを初期化
    let pool = initialize_database(db_url)
        .await
        .expect("Failed to initialize database");

    // users テーブルのカラムを確認（カラム数をカウント）
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('users')")
        .fetch_one(&pool)
        .await
        .expect("Failed to get table info");
    assert!(count > 0, "users table should have columns");
    assert!(count >= 6, "users table should have at least 6 columns (id, username, password_hash, role, created_at, last_login)");

    // api_keys テーブルに外部キーがあることを確認
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_list('api_keys')")
        .fetch_one(&pool)
        .await
        .expect("Failed to get foreign keys");
    assert!(count > 0, "api_keys should have foreign key to users");

    // インデックスが作成されているか確認
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND tbl_name='users'",
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to get indexes");
    assert!(count > 0, "users table should have indexes");
}
