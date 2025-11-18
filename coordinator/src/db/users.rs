// T050-T052: ユーザーCRUD操作

use chrono::{DateTime, Utc};
use ollama_coordinator_common::auth::{User, UserRole};
use ollama_coordinator_common::error::CoordinatorError;
use sqlx::SqlitePool;
use uuid::Uuid;

/// ユーザーを作成
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `username` - ユーザー名
/// * `password_hash` - bcryptハッシュ化されたパスワード
/// * `role` - ユーザーロール
///
/// # Returns
/// * `Ok(User)` - 作成されたユーザー
/// * `Err(CoordinatorError)` - 作成失敗（ユーザー名重複など）
pub async fn create(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    role: UserRole,
) -> Result<User, CoordinatorError> {
    create_with_id(pool, Uuid::new_v4(), username, password_hash, role).await
}

/// ユーザーを特定のIDで作成（テスト用）
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - ユーザーID
/// * `username` - ユーザー名
/// * `password_hash` - bcryptハッシュ化されたパスワード
/// * `role` - ユーザーロール
///
/// # Returns
/// * `Ok(User)` - 作成されたユーザー
/// * `Err(CoordinatorError)` - 作成失敗（ユーザー名重複など）
pub async fn create_with_id(
    pool: &SqlitePool,
    id: Uuid,
    username: &str,
    password_hash: &str,
    role: UserRole,
) -> Result<User, CoordinatorError> {
    let created_at = Utc::now();

    let role_str = match role {
        UserRole::Admin => "admin",
        UserRole::Viewer => "viewer",
    };

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, created_at, last_login)
         VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind(id.to_string())
    .bind(username)
    .bind(password_hash)
    .bind(role_str)
    .bind(created_at.to_rfc3339())
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            CoordinatorError::Database(format!("Username '{}' already exists", username))
        } else {
            CoordinatorError::Database(format!("Failed to create user: {}", e))
        }
    })?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        role,
        created_at,
        last_login: None,
    })
}

/// ユーザー名でユーザーを検索
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `username` - ユーザー名
///
/// # Returns
/// * `Ok(Some(User))` - ユーザーが見つかった
/// * `Ok(None)` - ユーザーが見つからなかった
/// * `Err(CoordinatorError)` - 検索失敗
pub async fn find_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<User>, CoordinatorError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login FROM users WHERE username = ?"
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to find user: {}", e)))?;

    Ok(row.map(|r| r.into_user()))
}

/// すべてのユーザーを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(Vec<User>)` - ユーザー一覧
/// * `Err(CoordinatorError)` - 取得失敗
pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, CoordinatorError> {
    let rows = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login FROM users ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to list users: {}", e)))?;

    Ok(rows.into_iter().map(|r| r.into_user()).collect())
}

/// ユーザーを更新
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - ユーザーID
/// * `username` - 新しいユーザー名（Noneの場合は変更なし）
/// * `password_hash` - 新しいパスワードハッシュ（Noneの場合は変更なし）
/// * `role` - 新しいロール（Noneの場合は変更なし）
///
/// # Returns
/// * `Ok(User)` - 更新されたユーザー
/// * `Err(CoordinatorError)` - 更新失敗
pub async fn update(
    pool: &SqlitePool,
    id: Uuid,
    username: Option<&str>,
    password_hash: Option<&str>,
    role: Option<UserRole>,
) -> Result<User, CoordinatorError> {
    // 現在のユーザー情報を取得
    let current = find_by_id(pool, id)
        .await?
        .ok_or_else(|| CoordinatorError::Database(format!("User not found: {}", id)))?;

    let new_username = username.unwrap_or(&current.username);
    let new_password_hash = password_hash.unwrap_or(&current.password_hash);
    let new_role = role.unwrap_or(current.role);
    let role_str = match new_role {
        UserRole::Admin => "admin",
        UserRole::Viewer => "viewer",
    };

    sqlx::query("UPDATE users SET username = ?, password_hash = ?, role = ? WHERE id = ?")
        .bind(new_username)
        .bind(new_password_hash)
        .bind(role_str)
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to update user: {}", e)))?;

    Ok(User {
        id,
        username: new_username.to_string(),
        password_hash: new_password_hash.to_string(),
        role: new_role,
        created_at: current.created_at,
        last_login: current.last_login,
    })
}

/// 最終ログイン日時を更新
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - ユーザーID
///
/// # Returns
/// * `Ok(())` - 更新成功
/// * `Err(CoordinatorError)` - 更新失敗
pub async fn update_last_login(pool: &SqlitePool, id: Uuid) -> Result<(), CoordinatorError> {
    let now = Utc::now();

    sqlx::query("UPDATE users SET last_login = ? WHERE id = ?")
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to update last login: {}", e)))?;

    Ok(())
}

/// ユーザーを削除
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - ユーザーID
///
/// # Returns
/// * `Ok(())` - 削除成功
/// * `Err(CoordinatorError)` - 削除失敗
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), CoordinatorError> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to delete user: {}", e)))?;

    Ok(())
}

/// IDでユーザーを検索
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `id` - ユーザーID
///
/// # Returns
/// * `Ok(Some(User))` - ユーザーが見つかった
/// * `Ok(None)` - ユーザーが見つからなかった
/// * `Err(CoordinatorError)` - 検索失敗
pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<User>, CoordinatorError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login FROM users WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| CoordinatorError::Database(format!("Failed to find user: {}", e)))?;

    Ok(row.map(|r| r.into_user()))
}

/// 初回起動チェック（ユーザーが0人かどうか）
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(true)` - ユーザーが0人（初回起動）
/// * `Ok(false)` - ユーザーが存在する
/// * `Err(CoordinatorError)` - チェック失敗
pub async fn is_first_boot(pool: &SqlitePool) -> Result<bool, CoordinatorError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to check first boot: {}", e)))?;

    Ok(count == 0)
}

/// 最後の管理者チェック（削除前の検証用）
///
/// # Arguments
/// * `pool` - データベース接続プール
/// * `user_id` - 削除しようとしているユーザーID
///
/// # Returns
/// * `Ok(true)` - このユーザーは最後の管理者（削除不可）
/// * `Ok(false)` - このユーザーを削除しても他に管理者がいる
/// * `Err(CoordinatorError)` - チェック失敗
pub async fn is_last_admin(pool: &SqlitePool, user_id: Uuid) -> Result<bool, CoordinatorError> {
    // 対象ユーザーが管理者かチェック
    let user = find_by_id(pool, user_id)
        .await?
        .ok_or_else(|| CoordinatorError::Database(format!("User not found: {}", user_id)))?;

    if user.role != UserRole::Admin {
        // 管理者でなければ最後の管理者ではない
        return Ok(false);
    }

    // 管理者の総数をカウント
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await
        .map_err(|e| CoordinatorError::Database(format!("Failed to count admins: {}", e)))?;

    // 管理者が1人だけの場合、このユーザーは最後の管理者
    Ok(admin_count == 1)
}

// SQLiteからの行取得用の内部型
#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role: String,
    created_at: String,
    last_login: Option<String>,
}

impl UserRow {
    fn into_user(self) -> User {
        let id = Uuid::parse_str(&self.id).unwrap();
        let role = match self.role.as_str() {
            "admin" => UserRole::Admin,
            "viewer" => UserRole::Viewer,
            _ => UserRole::Viewer, // デフォルト
        };
        let created_at = DateTime::parse_from_rfc3339(&self.created_at)
            .unwrap()
            .with_timezone(&Utc);
        let last_login = self.last_login.as_ref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        User {
            id,
            username: self.username,
            password_hash: self.password_hash,
            role,
            created_at,
            last_login,
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
    async fn test_create_and_find_user() {
        let pool = setup_test_db().await;

        let user = create(&pool, "testuser", "hash123", UserRole::Admin)
            .await
            .expect("Failed to create user");

        assert_eq!(user.username, "testuser");
        assert_eq!(user.role, UserRole::Admin);

        let found = find_by_username(&pool, "testuser")
            .await
            .expect("Failed to find user");
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "testuser");
    }

    #[tokio::test]
    async fn test_is_first_boot() {
        let pool = setup_test_db().await;

        assert!(is_first_boot(&pool).await.unwrap());

        create(&pool, "firstuser", "hash", UserRole::Admin)
            .await
            .unwrap();

        assert!(!is_first_boot(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_last_admin() {
        let pool = setup_test_db().await;

        let admin = create(&pool, "admin", "hash", UserRole::Admin)
            .await
            .unwrap();

        assert!(is_last_admin(&pool, admin.id).await.unwrap());

        let _admin2 = create(&pool, "admin2", "hash", UserRole::Admin)
            .await
            .unwrap();

        assert!(!is_last_admin(&pool, admin.id).await.unwrap());
    }
}
