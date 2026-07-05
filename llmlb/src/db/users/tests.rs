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
