//! 初回起動時の管理者アカウント作成
//!
//! 環境変数または対話式で管理者を作成

use crate::auth::password::{hash_password, validate_password};
use crate::common::auth::UserRole;
use crate::common::error::LbError;
use crate::config::get_env_with_fallback;
use crate::db;
use std::io::{self, Write};

/// 環境変数から管理者を作成
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Environment Variables
/// * `ADMIN_USERNAME` - 管理者ユーザー名（省略時: "admin"）
/// * `ADMIN_PASSWORD` - 管理者パスワード（必須）
///
/// # Returns
/// * `Ok(Some(username))` - 管理者作成成功（ユーザー名を返す）
/// * `Ok(None)` - ADMIN_PASSWORDが未設定（作成しない）
/// * `Err(LbError)` - 作成失敗
pub async fn create_admin_from_env(pool: &sqlx::SqlitePool) -> Result<Option<String>, LbError> {
    // ADMIN_PASSWORDが設定されていなければスキップ
    let password = match get_env_with_fallback("LLMLB_ADMIN_PASSWORD", "ADMIN_PASSWORD") {
        Some(p) if !p.is_empty() => p,
        _ => {
            tracing::debug!("LLMLB_ADMIN_PASSWORD not set, skipping admin creation from env");
            return Ok(None);
        }
    };

    // ADMIN_USERNAMEが設定されていなければデフォルト値を使用
    let username = get_env_with_fallback("LLMLB_ADMIN_USERNAME", "ADMIN_USERNAME")
        .unwrap_or_else(|| "admin".to_string());

    // パスワード要件を検証（弱い管理者パスワードでの起動を防ぐ）
    validate_password(&password)?;

    // パスワードをハッシュ化
    let password_hash = hash_password(&password)?;

    // 管理者を作成
    match db::users::create(pool, &username, &password_hash, UserRole::Admin, false).await {
        Ok(user) => {
            tracing::info!("Created admin user from env: username={}", username);
            Ok(Some(user.username))
        }
        Err(LbError::Database(ref e)) if e.contains("UNIQUE constraint failed") => {
            tracing::warn!("Admin user {} already exists, skipping creation", username);
            Ok(Some(username))
        }
        Err(e) => {
            tracing::error!("Failed to create admin user from env: {}", e);
            Err(e)
        }
    }
}

/// 対話式で管理者を作成
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(username)` - 作成された管理者のユーザー名
/// * `Err(LbError)` - 作成失敗
pub async fn create_admin_interactive(pool: &sqlx::SqlitePool) -> Result<String, LbError> {
    println!("\n=== Initial Setup: Create Admin User ===");

    // ユーザー名を入力
    print!("Enter admin username (default: admin): ");
    let _ = io::stdout().flush(); // エラー時は無視（対話的UIで回復不能）
    let mut username = String::new();
    io::stdin()
        .read_line(&mut username)
        .map_err(|e| LbError::Internal(format!("Failed to read username: {}", e)))?;
    let username = username.trim();
    let username = if username.is_empty() {
        "admin"
    } else {
        username
    };

    // パスワードを入力（マスク表示）
    let password = rpassword::prompt_password("Enter admin password: ")
        .map_err(|e| LbError::Internal(format!("Failed to read password: {}", e)))?;
    let password = password.trim();

    // パスワードが空でないことを確認
    if password.is_empty() {
        return Err(LbError::Internal("Password cannot be empty".to_string()));
    }

    // パスワード要件を検証（8文字以上・大文字・数字）
    validate_password(password)?;

    // パスワードをハッシュ化
    let password_hash = hash_password(password)?;

    // 管理者を作成
    match db::users::create(pool, username, &password_hash, UserRole::Admin, false).await {
        Ok(user) => {
            println!("✓ Admin user '{}' created successfully", user.username);
            tracing::info!(
                "Created admin user interactively: username={}",
                user.username
            );
            Ok(user.username)
        }
        Err(LbError::Database(ref e)) if e.contains("UNIQUE constraint failed") => {
            println!("✓ Admin user '{}' already exists", username);
            tracing::warn!("Admin user {} already exists, skipping creation", username);
            Ok(username.to_string())
        }
        Err(e) => {
            println!("✗ Failed to create admin user: {}", e);
            tracing::error!("Failed to create admin user interactively: {}", e);
            Err(e)
        }
    }
}

/// 開発モード用のadminユーザーを作成（nil UUIDを使用）
///
/// 開発モードでは `uuid::Uuid::nil()` を使ってログインするため、
/// FOREIGN KEY制約を満たすためにDBにもnil UUIDのadminユーザーが必要
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(())` - 処理成功
/// * `Err(LbError)` - 処理失敗
#[cfg(debug_assertions)]
async fn ensure_dev_admin_exists(pool: &sqlx::SqlitePool) -> Result<(), LbError> {
    use uuid::Uuid;

    let dev_user_id = Uuid::nil();

    // 既にnil UUIDのユーザーが存在するかチェック
    if db::users::find_by_id(pool, dev_user_id).await?.is_some() {
        tracing::debug!("Dev admin user already exists");
        return Ok(());
    }

    // 開発用のパスワードハッシュを生成
    let password_hash = hash_password("test")?;

    let fallback_username = format!("admin-dev-{}", &Uuid::new_v4().to_string()[..8]);
    let candidates = ["admin", "admin-dev", fallback_username.as_str()];

    for username in candidates {
        match db::users::create_with_id(
            pool,
            dev_user_id,
            username,
            &password_hash,
            UserRole::Admin,
            false,
        )
        .await
        {
            Ok(_) => {
                tracing::info!(
                    "Created dev admin user with nil UUID (username={})",
                    username
                );
                return Ok(());
            }
            Err(LbError::Database(ref e)) if e.contains("already exists") => {
                tracing::debug!("Dev admin username conflict: {}", username);
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(LbError::Database(
        "Failed to create dev admin user due to repeated username conflicts".to_string(),
    ))
}

/// 初回起動時の管理者作成処理
///
/// 1. データベースにユーザーが存在するかチェック
/// 2. ユーザーが存在しない場合:
///    a. 環境変数（ADMIN_PASSWORD）が設定されていれば環境変数から作成
///    b. 環境変数が未設定なら対話式で作成
/// 3. ユーザーが既に存在する場合はスキップ
/// 4. 開発モードでは常にnil UUIDのadminユーザーを確保
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(())` - 処理成功
/// * `Err(LbError)` - 処理失敗
pub async fn ensure_admin_exists(pool: &sqlx::SqlitePool) -> Result<(), LbError> {
    // 開発モードでは常にnil UUIDのadminユーザーを確保
    #[cfg(debug_assertions)]
    ensure_dev_admin_exists(pool).await?;

    // 初回起動かチェック
    let is_first_boot = db::users::is_first_boot(pool).await?;
    if !is_first_boot {
        tracing::debug!("Users already exist, skipping admin creation");
        return Ok(());
    }

    tracing::info!("First boot detected, creating admin user");

    // 環境変数から管理者を作成
    match create_admin_from_env(pool).await? {
        Some(username) => {
            tracing::info!("Admin user created from environment: {}", username);
            Ok(())
        }
        None => {
            // 環境変数が未設定なら対話式で作成
            tracing::info!("ADMIN_PASSWORD not set, prompting for admin credentials");
            create_admin_interactive(pool).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use serial_test::serial;

    async fn create_test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect(":memory:")
            .await
            .expect("Failed to create test database");
        migrations::run_migrations(&pool)
            .await
            .expect("Failed to run migrations");
        pool
    }

    #[tokio::test]
    #[serial]
    async fn test_create_admin_from_env_with_password() {
        let pool = create_test_pool().await;

        // 環境変数を設定
        std::env::set_var("ADMIN_USERNAME", "testadmin");
        std::env::set_var("ADMIN_PASSWORD", "Testpass123");

        let result = create_admin_from_env(&pool).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("testadmin".to_string()));

        // ユーザーが作成されたことを確認
        let user = db::users::find_by_username(&pool, "testadmin")
            .await
            .unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().role, UserRole::Admin);

        // クリーンアップ
        std::env::remove_var("ADMIN_USERNAME");
        std::env::remove_var("ADMIN_PASSWORD");
    }

    #[tokio::test]
    #[serial]
    async fn test_create_admin_from_env_without_password() {
        let pool = create_test_pool().await;

        // ADMIN_PASSWORDを削除
        std::env::remove_var("ADMIN_PASSWORD");

        let result = create_admin_from_env(&pool).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    #[serial]
    async fn test_create_admin_from_env_with_default_username() {
        let pool = create_test_pool().await;

        // ADMIN_USERNAMEを削除してデフォルト値を使用
        std::env::remove_var("ADMIN_USERNAME");
        std::env::set_var("ADMIN_PASSWORD", "Testpass123");

        let result = create_admin_from_env(&pool).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("admin".to_string()));

        // ユーザーが作成されたことを確認
        let user = db::users::find_by_username(&pool, "admin").await.unwrap();
        assert!(user.is_some());

        // クリーンアップ
        std::env::remove_var("ADMIN_PASSWORD");
    }

    #[tokio::test]
    #[serial]
    async fn test_ensure_admin_exists_first_boot() {
        let pool = create_test_pool().await;

        // 環境変数を設定
        std::env::set_var("ADMIN_USERNAME", "firstadmin");
        std::env::set_var("ADMIN_PASSWORD", "Firstpass123");

        let result = ensure_admin_exists(&pool).await;
        assert!(result.is_ok());

        // 開発モードではnil UUIDの"admin"が先に作成されるため、
        // is_first_bootがfalseになり環境変数のユーザーは作成されない
        #[cfg(debug_assertions)]
        {
            // 開発モード: nil UUIDのadminユーザーが作成される
            let dev_admin = db::users::find_by_id(&pool, uuid::Uuid::nil())
                .await
                .unwrap();
            assert!(dev_admin.is_some());
            assert_eq!(dev_admin.unwrap().username, "admin");
        }

        #[cfg(not(debug_assertions))]
        {
            // リリースモード: 環境変数で指定されたユーザーが作成される
            let user = db::users::find_by_username(&pool, "firstadmin")
                .await
                .unwrap();
            assert!(user.is_some());
        }

        // クリーンアップ
        std::env::remove_var("ADMIN_USERNAME");
        std::env::remove_var("ADMIN_PASSWORD");
    }

    #[tokio::test]
    #[serial]
    async fn test_ensure_admin_exists_not_first_boot() {
        let pool = create_test_pool().await;

        // ダミーユーザーを作成（初回起動でない状態）
        let hash = hash_password("dummy").unwrap();
        db::users::create(&pool, "existing", &hash, UserRole::Admin, false)
            .await
            .unwrap();

        // 環境変数を設定（使用されないはず）
        std::env::set_var("ADMIN_USERNAME", "shouldnotcreate");
        std::env::set_var("ADMIN_PASSWORD", "shouldnotcreate");

        let result = ensure_admin_exists(&pool).await;
        assert!(result.is_ok());

        // 新しいユーザーが作成されていないことを確認
        let user = db::users::find_by_username(&pool, "shouldnotcreate")
            .await
            .unwrap();
        assert!(user.is_none());

        // クリーンアップ
        std::env::remove_var("ADMIN_USERNAME");
        std::env::remove_var("ADMIN_PASSWORD");
    }
}
