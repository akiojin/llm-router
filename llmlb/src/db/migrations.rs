// T040-T041: データベースマイグレーション実行とJSONインポート

use crate::common::error::LbError;
use sqlx::{migrate::MigrateDatabase, Row, Sqlite, SqlitePool};

/// SQLiteデータベース接続プールを作成してマイグレーションを実行
///
/// # Arguments
/// * `database_url` - データベースURL（例: "sqlite:data/load balancer.db"）
///
/// # Returns
/// * `Ok(SqlitePool)` - 初期化済みデータベースプール
/// * `Err(LbError)` - 初期化失敗
pub async fn initialize_database(database_url: &str) -> Result<SqlitePool, LbError> {
    // データベースファイルが存在しない場合は作成
    if !Sqlite::database_exists(database_url)
        .await
        .map_err(|e| LbError::Database(format!("Failed to check database: {}", e)))?
    {
        tracing::info!("Creating database: {}", database_url);
        Sqlite::create_database(database_url)
            .await
            .map_err(|e| LbError::Database(format!("Failed to create database: {}", e)))?;
    }

    // 接続プールを作成
    let pool = SqlitePool::connect(database_url)
        .await
        .map_err(|e| LbError::Database(format!("Failed to connect to database: {}", e)))?;

    // マイグレーションを実行
    run_migrations(&pool).await?;

    Ok(pool)
}

// Legacy migration checksum compatibility for Issue #420.
// These migration files were renamed during SPEC refactors, which changed sqlx's
// stored checksums without changing the actual migration version numbers.
const MIGRATION_005_OLD_CHECKSUM: [u8; 48] = [
    0xbb, 0x58, 0x31, 0x50, 0x93, 0xaf, 0x8c, 0xc7, 0x44, 0xed, 0x00, 0xf7, 0xdd, 0xe3, 0xc4, 0xd5,
    0xd2, 0xca, 0xdb, 0xf4, 0xa8, 0x92, 0x20, 0x0e, 0x4f, 0x39, 0xbf, 0xdf, 0xd3, 0x34, 0x61, 0xfa,
    0x3e, 0x7f, 0x72, 0xeb, 0x9a, 0xd3, 0x33, 0xc6, 0x05, 0xb2, 0xc3, 0xe7, 0x78, 0xd0, 0x2d, 0xee,
];
const MIGRATION_005_V440_CHECKSUM: [u8; 48] = [
    0x5b, 0x77, 0x47, 0x63, 0xce, 0xd7, 0xd8, 0xbc, 0x14, 0xe9, 0x6b, 0x88, 0x1c, 0x33, 0x90, 0x73,
    0x5a, 0xe9, 0x92, 0x74, 0x46, 0xbd, 0x0e, 0x82, 0xc4, 0x2a, 0xe5, 0xe5, 0x8d, 0x0b, 0xcf, 0x50,
    0x43, 0xb4, 0xbf, 0x00, 0xa2, 0x8e, 0x3a, 0x95, 0x89, 0xa8, 0x1c, 0x08, 0x9c, 0x26, 0xcc, 0xa0,
];
const MIGRATION_005_NEW_CHECKSUM: [u8; 48] = [
    0x0f, 0xa6, 0x82, 0x71, 0xef, 0x76, 0x91, 0xb0, 0x57, 0x9d, 0xcb, 0x19, 0x4e, 0x01, 0x99, 0x89,
    0x78, 0xf8, 0xdf, 0x1d, 0x4b, 0x21, 0x5c, 0xce, 0x18, 0xb1, 0x26, 0x1b, 0x38, 0x57, 0x60, 0x35,
    0x0c, 0x13, 0x32, 0xa3, 0xd8, 0x3c, 0xc8, 0x54, 0x1a, 0x84, 0xa0, 0x0b, 0x0d, 0xea, 0x65, 0xa5,
];
const MIGRATION_006_OLD_CHECKSUM: [u8; 48] = [
    0x75, 0x40, 0x0a, 0xfd, 0x5b, 0xb7, 0x64, 0x14, 0xab, 0xc1, 0x00, 0x00, 0x6f, 0x5b, 0x53, 0xb0,
    0x17, 0xdc, 0xe0, 0x93, 0xd9, 0x00, 0xc8, 0xf2, 0x63, 0x01, 0x4a, 0x4b, 0xe8, 0xd6, 0xc5, 0x2b,
    0x39, 0x8a, 0xbd, 0xb4, 0xc3, 0x5e, 0xad, 0xf9, 0xbb, 0x14, 0xfa, 0xbd, 0xfb, 0x68, 0x63, 0x96,
];
const MIGRATION_006_NEW_CHECKSUM: [u8; 48] = [
    0xd1, 0x49, 0xbf, 0xea, 0xbc, 0x07, 0x3e, 0x9e, 0x1d, 0x6c, 0xd5, 0xda, 0x10, 0x12, 0x9c, 0xca,
    0x54, 0xf8, 0xa4, 0x67, 0xf4, 0xe8, 0xc9, 0x6c, 0x9f, 0xce, 0x43, 0xb5, 0x0a, 0x7d, 0xfb, 0xc9,
    0x7c, 0xf7, 0x35, 0xd4, 0xba, 0xff, 0xef, 0x10, 0x61, 0x1b, 0xb3, 0xb5, 0xfd, 0x76, 0xd3, 0x33,
];
const MIGRATION_007_OLD_CHECKSUM: [u8; 48] = [
    0xbc, 0x4c, 0xa0, 0x47, 0x18, 0x85, 0xa3, 0xd3, 0x1d, 0x6c, 0x91, 0x58, 0x5f, 0x76, 0x9f, 0xd8,
    0x79, 0xf6, 0xcb, 0x0a, 0x8c, 0xe2, 0x66, 0xc6, 0x05, 0xd5, 0xb1, 0x3d, 0xc7, 0x8c, 0x2f, 0x9e,
    0xa4, 0x32, 0x7e, 0x30, 0x94, 0x7b, 0x13, 0x15, 0x06, 0x38, 0x81, 0x55, 0x8b, 0xca, 0x0f, 0xa7,
];
const MIGRATION_007_NEW_CHECKSUM: [u8; 48] = [
    0x94, 0xc9, 0x05, 0x28, 0xc7, 0xb9, 0x96, 0xef, 0xb9, 0x22, 0x22, 0xa4, 0x46, 0xa2, 0x68, 0xae,
    0xce, 0xe8, 0x62, 0xc9, 0xc1, 0xd5, 0x50, 0x0b, 0x50, 0x37, 0xcf, 0x2d, 0xf3, 0x19, 0xe9, 0xa5,
    0xdb, 0x1f, 0x65, 0xd9, 0x1e, 0xc3, 0x45, 0x2a, 0xe8, 0x63, 0xa9, 0x2a, 0x8f, 0x6e, 0xd4, 0x6b,
];
const MIGRATION_008_OLD_CHECKSUM: [u8; 48] = [
    0x40, 0xc9, 0xe6, 0x46, 0x26, 0x8e, 0xa3, 0xfb, 0xe8, 0x0b, 0xd5, 0x99, 0x7d, 0xa8, 0x94, 0x44,
    0x41, 0x49, 0x7d, 0x42, 0x06, 0xc1, 0xa9, 0x45, 0xd5, 0x97, 0xdc, 0x16, 0x32, 0x35, 0x9d, 0x1d,
    0xae, 0xd4, 0x00, 0xb7, 0xdb, 0x44, 0x3f, 0x7c, 0xf7, 0x8a, 0xd8, 0xb4, 0x72, 0xc0, 0x56, 0xf5,
];
const MIGRATION_008_NEW_CHECKSUM: [u8; 48] = [
    0x67, 0x2d, 0x7a, 0xbb, 0xd4, 0x38, 0xba, 0x86, 0xdf, 0x5b, 0xd4, 0xec, 0xa1, 0x23, 0x70, 0x05,
    0xc3, 0xf1, 0xf0, 0x6b, 0x65, 0xc9, 0x16, 0xfb, 0x1a, 0x98, 0x2b, 0x13, 0x09, 0xf7, 0x1c, 0x0d,
    0x28, 0x8c, 0x43, 0xce, 0x0d, 0xc7, 0x27, 0xdc, 0x5b, 0x7e, 0xdd, 0x53, 0xae, 0x1f, 0x9f, 0x47,
];
const MIGRATION_010_OLD_CHECKSUM: [u8; 48] = [
    0x45, 0xeb, 0x00, 0x87, 0x16, 0x2b, 0x72, 0x68, 0x49, 0xd1, 0xf9, 0x13, 0xa1, 0xef, 0x90, 0x20,
    0x37, 0x70, 0xe2, 0xb5, 0xac, 0xca, 0xd9, 0x95, 0x6b, 0x27, 0x59, 0x18, 0xf1, 0x8c, 0x99, 0xe7,
    0x84, 0xbe, 0xc3, 0x23, 0xe7, 0x7b, 0xa9, 0xd9, 0x83, 0x21, 0xf0, 0x79, 0xe0, 0x96, 0xb9, 0x0f,
];
const MIGRATION_010_NEW_CHECKSUM: [u8; 48] = [
    0x1e, 0x74, 0xfb, 0x8f, 0x5b, 0x52, 0xc5, 0x5b, 0x04, 0x27, 0xf6, 0xc1, 0xc2, 0x3a, 0x96, 0x19,
    0x23, 0x28, 0x96, 0x3f, 0xa2, 0x3f, 0x80, 0x92, 0x47, 0x13, 0x97, 0x6b, 0xbb, 0x94, 0xcd, 0xb7,
    0xe5, 0x0c, 0x42, 0x8d, 0xb4, 0x3e, 0xad, 0x10, 0xa0, 0x5a, 0x80, 0x09, 0xea, 0x3a, 0x40, 0x2d,
];
const MIGRATION_011_OLD_CHECKSUM: [u8; 48] = [
    0xa6, 0xce, 0x7e, 0xf6, 0x6b, 0xad, 0xaa, 0x37, 0x9c, 0x28, 0x41, 0xe4, 0x60, 0x10, 0xf9, 0x75,
    0x17, 0x52, 0x78, 0xe0, 0x5f, 0x76, 0x27, 0xe3, 0xd0, 0x15, 0x77, 0x1a, 0x50, 0xa4, 0x06, 0xcb,
    0x60, 0xc7, 0x1e, 0x41, 0xfb, 0xdc, 0xb4, 0xb3, 0x2f, 0x44, 0x4f, 0x35, 0x3b, 0x6a, 0x5a, 0xa0,
];
const MIGRATION_011_NEW_CHECKSUM: [u8; 48] = [
    0x09, 0x01, 0x56, 0x3f, 0x6c, 0x4a, 0xa9, 0xc0, 0xae, 0x72, 0x4e, 0xc3, 0x08, 0xf4, 0xf4, 0xa4,
    0x87, 0x83, 0x23, 0x56, 0x6b, 0xe3, 0x31, 0x9b, 0x05, 0x1d, 0xf6, 0xab, 0xfa, 0x38, 0x94, 0xe0,
    0xd9, 0xf1, 0x52, 0xb7, 0xef, 0x1a, 0x3c, 0x6d, 0x7a, 0xa4, 0x0d, 0xe2, 0x33, 0xe0, 0x42, 0xc0,
];
const MIGRATION_014_OLD_CHECKSUM: [u8; 48] = [
    0xf6, 0xeb, 0x48, 0x0a, 0x08, 0xcb, 0xc5, 0x2f, 0x59, 0x8b, 0xd8, 0xa8, 0x80, 0x58, 0x3a, 0x8d,
    0x68, 0x2e, 0x6f, 0x44, 0xe5, 0x62, 0x27, 0x59, 0x40, 0x02, 0x06, 0x43, 0xa6, 0x2b, 0xa2, 0xdd,
    0x8f, 0xd6, 0xb7, 0x60, 0xc8, 0x85, 0x08, 0x84, 0x54, 0x74, 0xee, 0xa0, 0x2a, 0xc9, 0xae, 0x47,
];
const MIGRATION_014_NEW_CHECKSUM: [u8; 48] = [
    0x5f, 0xcb, 0x39, 0x8f, 0x23, 0x62, 0x44, 0xbe, 0x93, 0x6c, 0xc7, 0x3a, 0x29, 0x4b, 0x6b, 0xc7,
    0xe2, 0x37, 0x29, 0xe8, 0xe9, 0xf8, 0x01, 0xb1, 0xdb, 0xfe, 0x95, 0x56, 0xa2, 0x9b, 0xb5, 0xcc,
    0xe7, 0x1b, 0x49, 0x92, 0xc7, 0x1b, 0x3f, 0x94, 0x91, 0xb0, 0xd8, 0x00, 0x82, 0xdd, 0xe8, 0x0f,
];

struct MigrationChecksumOverride {
    version: i64,
    old: &'static [u8; 48],
    new: &'static [u8; 48],
}

const MIGRATION_CHECKSUM_OVERRIDES: &[MigrationChecksumOverride] = &[
    MigrationChecksumOverride {
        version: 5,
        old: &MIGRATION_005_OLD_CHECKSUM,
        new: &MIGRATION_005_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 5,
        old: &MIGRATION_005_V440_CHECKSUM,
        new: &MIGRATION_005_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 6,
        old: &MIGRATION_006_OLD_CHECKSUM,
        new: &MIGRATION_006_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 7,
        old: &MIGRATION_007_OLD_CHECKSUM,
        new: &MIGRATION_007_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 8,
        old: &MIGRATION_008_OLD_CHECKSUM,
        new: &MIGRATION_008_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 10,
        old: &MIGRATION_010_OLD_CHECKSUM,
        new: &MIGRATION_010_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 11,
        old: &MIGRATION_011_OLD_CHECKSUM,
        new: &MIGRATION_011_NEW_CHECKSUM,
    },
    MigrationChecksumOverride {
        version: 14,
        old: &MIGRATION_014_OLD_CHECKSUM,
        new: &MIGRATION_014_NEW_CHECKSUM,
    },
];

async fn reconcile_migration_checksums(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if row.0 == 0 {
        return Ok(());
    }

    for override_entry in MIGRATION_CHECKSUM_OVERRIDES {
        let checksum_row = sqlx::query("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
            .bind(override_entry.version)
            .fetch_optional(pool)
            .await?;
        let Some(checksum_row) = checksum_row else {
            continue;
        };

        let checksum: Vec<u8> = checksum_row.try_get("checksum")?;
        if checksum == override_entry.old.as_slice() {
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
                .bind(override_entry.new.as_slice())
                .bind(override_entry.version)
                .execute(pool)
                .await?;
            tracing::info!(
                version = override_entry.version,
                "Updated legacy migration checksum to current format"
            );
        }
    }

    Ok(())
}

/// マイグレーションを実行（sqlx::migrate!マクロを使用）
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(())` - マイグレーション成功
/// * `Err(LbError)` - マイグレーション失敗
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), LbError> {
    tracing::info!("Running database migrations");

    reconcile_migration_checksums(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to reconcile migrations: {}", e)))?;

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to run migrations: {}", e)))?;

    tracing::info!("Database migrations completed successfully");
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

    // --- 追加テスト ---

    #[tokio::test]
    async fn test_migrations_create_endpoints_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='endpoints'")
                .fetch_one(&pool)
                .await;
        assert!(result.is_ok(), "endpoints table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_settings_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='settings'")
                .fetch_one(&pool)
                .await;
        assert!(result.is_ok(), "settings table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_audit_log_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='audit_log_entries'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "audit_log_entries table should exist");
    }

    #[tokio::test]
    async fn test_migrations_idempotent() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Running twice should not error
        run_migrations(&pool).await.unwrap();

        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='users'")
                .fetch_one(&pool)
                .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_migrations_create_invitation_codes_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='invitation_codes'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "invitation_codes table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_endpoint_models_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='endpoint_models'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "endpoint_models table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_endpoint_health_checks_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='endpoint_health_checks'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "endpoint_health_checks table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_model_download_tasks_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='model_download_tasks'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "model_download_tasks table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_endpoint_daily_stats_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='endpoint_daily_stats'",
        )
        .fetch_one(&pool)
        .await;
        assert!(result.is_ok(), "endpoint_daily_stats table should exist");
    }

    #[tokio::test]
    async fn test_migrations_create_models_table() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let result =
            sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='models'")
                .fetch_one(&pool)
                .await;
        assert!(result.is_ok(), "models table should exist");
    }

    #[tokio::test]
    async fn test_run_migrations_recovers_from_pre_v440_checksum_for_migration_005() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let current_checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 5")
                .fetch_one(&pool)
                .await
                .expect("should read current checksum");
        let legacy_checksum: [u8; 48] = [
            0xbb, 0x58, 0x31, 0x50, 0x93, 0xaf, 0x8c, 0xc7, 0x44, 0xed, 0x00, 0xf7, 0xdd, 0xe3,
            0xc4, 0xd5, 0xd2, 0xca, 0xdb, 0xf4, 0xa8, 0x92, 0x20, 0x0e, 0x4f, 0x39, 0xbf, 0xdf,
            0xd3, 0x34, 0x61, 0xfa, 0x3e, 0x7f, 0x72, 0xeb, 0x9a, 0xd3, 0x33, 0xc6, 0x05, 0xb2,
            0xc3, 0xe7, 0x78, 0xd0, 0x2d, 0xee,
        ];
        assert_ne!(current_checksum, legacy_checksum);

        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 5")
            .bind(legacy_checksum.as_slice())
            .execute(&pool)
            .await
            .expect("should overwrite checksum with legacy value");

        run_migrations(&pool)
            .await
            .expect("run_migrations should reconcile legacy checksum before sqlx migrate");

        let reconciled_checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 5")
                .fetch_one(&pool)
                .await
                .expect("should read reconciled checksum");
        assert_eq!(reconciled_checksum, current_checksum);
    }

    #[tokio::test]
    async fn test_run_migrations_recovers_from_v440_checksum_for_migration_005() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let current_checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 5")
                .fetch_one(&pool)
                .await
                .expect("should read current checksum");
        let v440_checksum: [u8; 48] = [
            0x5b, 0x77, 0x47, 0x63, 0xce, 0xd7, 0xd8, 0xbc, 0x14, 0xe9, 0x6b, 0x88, 0x1c, 0x33,
            0x90, 0x73, 0x5a, 0xe9, 0x92, 0x74, 0x46, 0xbd, 0x0e, 0x82, 0xc4, 0x2a, 0xe5, 0xe5,
            0x8d, 0x0b, 0xcf, 0x50, 0x43, 0xb4, 0xbf, 0x00, 0xa2, 0x8e, 0x3a, 0x95, 0x89, 0xa8,
            0x1c, 0x08, 0x9c, 0x26, 0xcc, 0xa0,
        ];
        assert_ne!(current_checksum, v440_checksum);

        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 5")
            .bind(v440_checksum.as_slice())
            .execute(&pool)
            .await
            .expect("should overwrite checksum with v4.4.0 value");

        run_migrations(&pool)
            .await
            .expect("run_migrations should reconcile v4.4.0 checksum before sqlx migrate");

        let reconciled_checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 5")
                .fetch_one(&pool)
                .await
                .expect("should read reconciled checksum");
        assert_eq!(reconciled_checksum, current_checksum);
    }

    #[tokio::test]
    async fn test_reconcile_migration_checksums_updates_all_known_legacy_checksums() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
            r#"
CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
            "#,
        )
        .execute(&pool)
        .await
        .expect("should create _sqlx_migrations table");

        let insert_sql = "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?1, ?2, ?3, ?4, ?5)";
        let old_entries: &[(i64, &[u8; 48])] = &[
            (5, &MIGRATION_005_OLD_CHECKSUM),
            (6, &MIGRATION_006_OLD_CHECKSUM),
            (7, &MIGRATION_007_OLD_CHECKSUM),
            (8, &MIGRATION_008_OLD_CHECKSUM),
            (10, &MIGRATION_010_OLD_CHECKSUM),
            (11, &MIGRATION_011_OLD_CHECKSUM),
            (14, &MIGRATION_014_OLD_CHECKSUM),
        ];
        for (version, checksum) in old_entries {
            sqlx::query(insert_sql)
                .bind(*version)
                .bind("test")
                .bind(true)
                .bind(checksum.as_slice())
                .bind(0_i64)
                .execute(&pool)
                .await
                .unwrap_or_else(|_| panic!("should insert migration row for version {version}"));
        }

        reconcile_migration_checksums(&pool)
            .await
            .expect("reconcile should succeed");

        let expected: &[(i64, &[u8; 48])] = &[
            (5, &MIGRATION_005_NEW_CHECKSUM),
            (6, &MIGRATION_006_NEW_CHECKSUM),
            (7, &MIGRATION_007_NEW_CHECKSUM),
            (8, &MIGRATION_008_NEW_CHECKSUM),
            (10, &MIGRATION_010_NEW_CHECKSUM),
            (11, &MIGRATION_011_NEW_CHECKSUM),
            (14, &MIGRATION_014_NEW_CHECKSUM),
        ];
        for (version, expected_checksum) in expected {
            let checksum: Vec<u8> =
                sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
                    .bind(*version)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| panic!("should read checksum for version {version}"));
            assert_eq!(
                checksum,
                expected_checksum.as_slice(),
                "checksum mismatch for migration version {version}"
            );
        }
    }
}
