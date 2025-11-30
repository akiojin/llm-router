//! 初回起動時の管理者アカウント作成
//!
//! 環境変数または対話式で管理者を作成

use crate::auth::password::hash_password;
use crate::config::get_env_with_fallback;
use crate::db;
use llm_router_common::auth::UserRole;
use llm_router_common::error::RouterError;
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
/// * `Err(RouterError)` - 作成失敗
pub async fn create_admin_from_env(pool: &sqlx::SqlitePool) -> Result<Option<String>, RouterError> {
    // ADMIN_PASSWORDが設定されていなければスキップ
    let password = match get_env_with_fallback("LLM_ROUTER_ADMIN_PASSWORD", "ADMIN_PASSWORD") {
        Some(p) if !p.is_empty() => p,
        _ => {
            tracing::debug!("LLM_ROUTER_ADMIN_PASSWORD not set, skipping admin creation from env");
            return Ok(None);
        }
    };

    // ADMIN_USERNAMEが設定されていなければデフォルト値を使用
    let username = get_env_with_fallback("LLM_ROUTER_ADMIN_USERNAME", "ADMIN_USERNAME")
        .unwrap_or_else(|| "admin".to_string());

    // パスワードをハッシュ化
    let password_hash = hash_password(&password)?;

    // 管理者を作成
    match db::users::create(pool, &username, &password_hash, UserRole::Admin).await {
        Ok(user) => {
            tracing::info!("Created admin user from env: username={}", username);
            Ok(Some(user.username))
        }
        Err(RouterError::Database(ref e)) if e.contains("UNIQUE constraint failed") => {
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
/// * `Err(RouterError)` - 作成失敗
pub async fn create_admin_interactive(pool: &sqlx::SqlitePool) -> Result<String, RouterError> {
    println!("\n=== Initial Setup: Create Admin User ===");

    // ユーザー名を入力
    print!("Enter admin username (default: admin): ");
    io::stdout().flush().unwrap();
    let mut username = String::new();
    io::stdin()
        .read_line(&mut username)
        .map_err(|e| RouterError::Internal(format!("Failed to read username: {}", e)))?;
    let username = username.trim();
    let username = if username.is_empty() {
        "admin"
    } else {
        username
    };

    // パスワードを入力（マスク表示）
    let password = rpassword::prompt_password("Enter admin password: ")
        .map_err(|e| RouterError::Internal(format!("Failed to read password: {}", e)))?;
    let password = password.trim();

    // パスワードが空でないことを確認
    if password.is_empty() {
        return Err(RouterError::Internal(
            "Password cannot be empty".to_string(),
        ));
    }

    // パスワードをハッシュ化
    let password_hash = hash_password(password)?;

    // 管理者を作成
    match db::users::create(pool, username, &password_hash, UserRole::Admin).await {
        Ok(user) => {
            println!("✓ Admin user '{}' created successfully", user.username);
            tracing::info!(
                "Created admin user interactively: username={}",
                user.username
            );
            Ok(user.username)
        }
        Err(RouterError::Database(ref e)) if e.contains("UNIQUE constraint failed") => {
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

/// 初回起動時の管理者作成処理
///
/// 1. データベースにユーザーが存在するかチェック
/// 2. ユーザーが存在しない場合:
///    a. 環境変数（ADMIN_PASSWORD）が設定されていれば環境変数から作成
///    b. 環境変数が未設定なら対話式で作成
/// 3. ユーザーが既に存在する場合はスキップ
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(())` - 処理成功
/// * `Err(RouterError)` - 処理失敗
pub async fn ensure_admin_exists(pool: &sqlx::SqlitePool) -> Result<(), RouterError> {
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
        std::env::set_var("ADMIN_PASSWORD", "testpass123");

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
        std::env::set_var("ADMIN_PASSWORD", "testpass123");

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
        std::env::set_var("ADMIN_PASSWORD", "firstpass123");

        let result = ensure_admin_exists(&pool).await;
        assert!(result.is_ok());

        // ユーザーが作成されたことを確認
        let user = db::users::find_by_username(&pool, "firstadmin")
            .await
            .unwrap();
        assert!(user.is_some());

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
        db::users::create(&pool, "existing", &hash, UserRole::Admin)
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
