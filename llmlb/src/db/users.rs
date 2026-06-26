// T050-T052: ユーザーCRUD操作

use crate::common::auth::{User, UserRole};
use crate::common::error::LbError;
use chrono::{DateTime, Utc};
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
/// * `Err(LbError)` - 作成失敗（ユーザー名重複など）
pub async fn create(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    role: UserRole,
    must_change_password: bool,
) -> Result<User, LbError> {
    create_with_id(
        pool,
        Uuid::new_v4(),
        username,
        password_hash,
        role,
        must_change_password,
    )
    .await
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
/// * `Err(LbError)` - 作成失敗（ユーザー名重複など）
pub async fn create_with_id(
    pool: &SqlitePool,
    id: Uuid,
    username: &str,
    password_hash: &str,
    role: UserRole,
    must_change_password: bool,
) -> Result<User, LbError> {
    let created_at = Utc::now();

    let role_str = match role {
        UserRole::Admin => "admin",
        UserRole::Viewer => "viewer",
    };

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, created_at, last_login, must_change_password)
         VALUES (?, ?, ?, ?, ?, NULL, ?)",
    )
    .bind(id.to_string())
    .bind(username)
    .bind(password_hash)
    .bind(role_str)
    .bind(created_at.to_rfc3339())
    .bind(must_change_password as i32)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            LbError::Database(format!("Username '{}' already exists", username))
        } else {
            LbError::Database(format!("Failed to create user: {}", e))
        }
    })?;

    Ok(User {
        id,
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        role,
        created_at,
        last_login: None,
        must_change_password,
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
/// * `Err(LbError)` - 検索失敗
pub async fn find_by_username(pool: &SqlitePool, username: &str) -> Result<Option<User>, LbError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login, must_change_password FROM users WHERE username = ?"
    )
    .bind(username)
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find user: {}", e)))?;

    Ok(row.map(|r| r.into_user()))
}

/// すべてのユーザーを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(Vec<User>)` - ユーザー一覧
/// * `Err(LbError)` - 取得失敗
pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, LbError> {
    let rows = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login, must_change_password FROM users ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to list users: {}", e)))?;

    Ok(rows.into_iter().map(|r| r.into_user()).collect())
}

/// 任意の管理者ユーザーIDを取得
///
/// # Arguments
/// * `pool` - データベース接続プール
///
/// # Returns
/// * `Ok(Some(Uuid))` - 管理者ユーザーIDが見つかった
/// * `Ok(None)` - 管理者ユーザーが存在しない
/// * `Err(LbError)` - 取得失敗
pub async fn find_any_admin_id(pool: &SqlitePool) -> Result<Option<Uuid>, LbError> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM users WHERE role = 'admin' ORDER BY created_at ASC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find admin user: {}", e)))?;

    match id {
        Some(raw) => Uuid::parse_str(&raw)
            .map(Some)
            .map_err(|e| LbError::Database(format!("Invalid admin user id: {}", e))),
        None => Ok(None),
    }
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
/// * `Err(LbError)` - 更新失敗
pub async fn update(
    pool: &SqlitePool,
    id: Uuid,
    username: Option<&str>,
    password_hash: Option<&str>,
    role: Option<UserRole>,
) -> Result<User, LbError> {
    // 現在のユーザー情報を取得
    let current = find_by_id(pool, id)
        .await?
        .ok_or_else(|| LbError::Database(format!("User not found: {}", id)))?;

    let new_username = username.unwrap_or(&current.username);
    let new_password_hash = password_hash.unwrap_or(&current.password_hash);
    let new_role = role.unwrap_or(current.role);
    let role_str = match new_role {
        UserRole::Admin => "admin",
        UserRole::Viewer => "viewer",
    };
    // パスワードが変更される場合は must_change_password フラグを有効化
    let new_must_change_password = if password_hash.is_some() {
        true
    } else {
        current.must_change_password
    };

    sqlx::query("UPDATE users SET username = ?, password_hash = ?, role = ?, must_change_password = ? WHERE id = ?")
        .bind(new_username)
        .bind(new_password_hash)
        .bind(role_str)
        .bind(new_must_change_password)
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to update user: {}", e)))?;

    Ok(User {
        id,
        username: new_username.to_string(),
        password_hash: new_password_hash.to_string(),
        role: new_role,
        created_at: current.created_at,
        last_login: current.last_login,
        must_change_password: new_must_change_password,
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
/// * `Err(LbError)` - 更新失敗
pub async fn update_last_login(pool: &SqlitePool, id: Uuid) -> Result<(), LbError> {
    let now = Utc::now();

    sqlx::query("UPDATE users SET last_login = ? WHERE id = ?")
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to update last login: {}", e)))?;

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
/// * `Err(LbError)` - 削除失敗
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), LbError> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to delete user: {}", e)))?;

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
/// * `Err(LbError)` - 検索失敗
pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<User>, LbError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, created_at, last_login, must_change_password FROM users WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|e| LbError::Database(format!("Failed to find user: {}", e)))?;

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
/// * `Err(LbError)` - チェック失敗
pub async fn is_first_boot(pool: &SqlitePool) -> Result<bool, LbError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to check first boot: {}", e)))?;

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
/// * `Err(LbError)` - チェック失敗
pub async fn is_last_admin(pool: &SqlitePool, user_id: Uuid) -> Result<bool, LbError> {
    // 対象ユーザーが管理者かチェック
    let user = find_by_id(pool, user_id)
        .await?
        .ok_or_else(|| LbError::Database(format!("User not found: {}", user_id)))?;

    if user.role != UserRole::Admin {
        // 管理者でなければ最後の管理者ではない
        return Ok(false);
    }

    // 管理者の総数をカウント
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to count admins: {}", e)))?;

    // 管理者が1人だけの場合、このユーザーは最後の管理者
    Ok(admin_count == 1)
}

/// must_change_password フラグをクリア
pub async fn clear_must_change_password(pool: &SqlitePool, id: Uuid) -> Result<(), LbError> {
    sqlx::query("UPDATE users SET must_change_password = 0 WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|e| LbError::Database(format!("Failed to clear must_change_password: {}", e)))?;
    Ok(())
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
    must_change_password: i32,
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
            must_change_password: self.must_change_password != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_create_and_find_user() {
        let pool = setup_test_db().await;

        let user = create(&pool, "testuser", "hash123", UserRole::Admin, false)
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

        create(&pool, "firstuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        assert!(!is_first_boot(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_last_admin() {
        let pool = setup_test_db().await;

        let admin = create(&pool, "admin", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        assert!(is_last_admin(&pool, admin.id).await.unwrap());

        let _admin2 = create(&pool, "admin2", "hash", UserRole::Admin, false)
            .await
            .unwrap();

        assert!(!is_last_admin(&pool, admin.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_find_by_id() {
        let pool = setup_test_db().await;
        let user = create(&pool, "findme", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        let found = find_by_id(&pool, user.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "findme");
    }

    #[tokio::test]
    async fn test_find_by_id_nonexistent() {
        let pool = setup_test_db().await;
        let found = find_by_id(&pool, Uuid::new_v4()).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_by_username_nonexistent() {
        let pool = setup_test_db().await;
        let found = find_by_username(&pool, "ghost").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_duplicate_username_error() {
        let pool = setup_test_db().await;
        create(&pool, "dup", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        let result = create(&pool, "dup", "hash2", UserRole::Viewer, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_user() {
        let pool = setup_test_db().await;
        let user = create(&pool, "todelete", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        delete(&pool, user.id).await.unwrap();
        let found = find_by_id(&pool, user.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_update_user_username() {
        let pool = setup_test_db().await;
        let user = create(&pool, "original", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        let updated = update(&pool, user.id, Some("renamed"), None, None)
            .await
            .unwrap();
        assert_eq!(updated.username, "renamed");
    }

    #[tokio::test]
    async fn test_update_user_role() {
        let pool = setup_test_db().await;
        let user = create(&pool, "roleuser", "hash", UserRole::Viewer, false)
            .await
            .unwrap();

        let updated = update(&pool, user.id, None, None, Some(UserRole::Admin))
            .await
            .unwrap();
        assert_eq!(updated.role, UserRole::Admin);
    }

    #[tokio::test]
    async fn test_update_last_login() {
        let pool = setup_test_db().await;
        let user = create(&pool, "loginuser", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        assert!(user.last_login.is_none());

        update_last_login(&pool, user.id).await.unwrap();
        let found = find_by_id(&pool, user.id).await.unwrap().unwrap();
        assert!(found.last_login.is_some());
    }

    #[tokio::test]
    async fn test_list_users() {
        let pool = setup_test_db().await;
        assert!(list(&pool).await.unwrap().is_empty());

        create(&pool, "u1", "h", UserRole::Admin, false)
            .await
            .unwrap();
        create(&pool, "u2", "h", UserRole::Viewer, false)
            .await
            .unwrap();

        let users = list(&pool).await.unwrap();
        assert_eq!(users.len(), 2);
    }

    #[tokio::test]
    async fn test_find_any_admin_id() {
        let pool = setup_test_db().await;

        assert!(find_any_admin_id(&pool).await.unwrap().is_none());

        let admin = create(&pool, "adm", "h", UserRole::Admin, false)
            .await
            .unwrap();
        let found = find_any_admin_id(&pool).await.unwrap();
        assert_eq!(found, Some(admin.id));
    }

    #[tokio::test]
    async fn test_clear_must_change_password() {
        let pool = setup_test_db().await;
        let user = create(&pool, "chgpw", "h", UserRole::Admin, true)
            .await
            .unwrap();
        assert!(user.must_change_password);

        clear_must_change_password(&pool, user.id).await.unwrap();
        let found = find_by_id(&pool, user.id).await.unwrap().unwrap();
        assert!(!found.must_change_password);
    }

    #[tokio::test]
    async fn test_is_last_admin_viewer_not_admin() {
        let pool = setup_test_db().await;
        let viewer = create(&pool, "v", "h", UserRole::Viewer, false)
            .await
            .unwrap();
        assert!(!is_last_admin(&pool, viewer.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_create_with_id() {
        let pool = setup_test_db().await;
        let fixed_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let user = create_with_id(
            &pool,
            fixed_id,
            "fixed_id_user",
            "hash",
            UserRole::Admin,
            false,
        )
        .await
        .unwrap();
        assert_eq!(user.id, fixed_id);
        assert_eq!(user.username, "fixed_id_user");

        let found = find_by_id(&pool, fixed_id).await.unwrap().unwrap();
        assert_eq!(found.id, fixed_id);
    }

    #[tokio::test]
    async fn test_update_nonexistent_user_returns_error() {
        let pool = setup_test_db().await;
        let result = update(&pool, Uuid::new_v4(), Some("new_name"), None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_password_hash() {
        let pool = setup_test_db().await;
        let user = create(&pool, "pwuser", "old_hash", UserRole::Viewer, false)
            .await
            .unwrap();
        let updated = update(&pool, user.id, None, Some("new_hash"), None)
            .await
            .unwrap();
        assert_eq!(updated.password_hash, "new_hash");

        let found = find_by_id(&pool, user.id).await.unwrap().unwrap();
        assert_eq!(found.password_hash, "new_hash");
    }

    #[tokio::test]
    async fn test_is_last_admin_nonexistent_user() {
        let pool = setup_test_db().await;
        let result = is_last_admin(&pool, Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_must_change_password_false() {
        let pool = setup_test_db().await;
        let user = create(&pool, "no_change", "hash", UserRole::Viewer, false)
            .await
            .unwrap();
        assert!(!user.must_change_password);
        let found = find_by_id(&pool, user.id).await.unwrap().unwrap();
        assert!(!found.must_change_password);
    }

    #[tokio::test]
    async fn test_update_preserves_created_at_and_last_login() {
        let pool = setup_test_db().await;
        let user = create(&pool, "preserve_fields", "hash", UserRole::Admin, false)
            .await
            .unwrap();
        update_last_login(&pool, user.id).await.unwrap();

        let before = find_by_id(&pool, user.id).await.unwrap().unwrap();
        let updated = update(&pool, user.id, Some("new_name"), None, None)
            .await
            .unwrap();

        assert_eq!(updated.created_at, before.created_at);
        assert_eq!(updated.last_login, before.last_login);
    }
}
