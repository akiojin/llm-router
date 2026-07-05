use super::*;
use crate::common::auth::UserRole;
use crate::db::users;

async fn setup_test_db() -> SqlitePool {
    crate::db::test_utils::test_db_pool().await
}

#[tokio::test]
async fn test_generate_invitation_code() {
    let code = generate_invitation_code();
    assert!(code.starts_with("inv_"));
    assert_eq!(code.len(), 4 + 16); // "inv_" + 16文字
}

#[tokio::test]
async fn test_create_and_find_invitation() {
    let pool = setup_test_db().await;

    // テスト用ユーザーを作成
    let user = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    // 招待コードを作成
    let invitation = create(&pool, user.id, None).await.unwrap();

    assert!(invitation.code.starts_with("inv_"));

    // 平文コードで検索
    let found = find_by_code(&pool, &invitation.code).await.unwrap();
    assert!(found.is_some());

    let found = found.unwrap();
    assert_eq!(found.id, invitation.id);
    assert_eq!(found.status, InvitationStatus::Active);
    assert_eq!(found.created_by, user.id);
}

#[tokio::test]
async fn test_mark_as_used() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // 新しいユーザーを作成して招待コードを使用
    let new_user = users::create(&pool, "newuser", "hash", UserRole::Viewer, false)
        .await
        .unwrap();

    mark_as_used(&pool, invitation.id, new_user.id)
        .await
        .unwrap();

    // 再度検索してステータスを確認
    let found = find_by_code(&pool, &invitation.code)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, InvitationStatus::Used);
    assert_eq!(found.used_by, Some(new_user.id));
    assert!(found.used_at.is_some());
}

#[tokio::test]
async fn test_cannot_use_twice() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    let user1 = users::create(&pool, "user1", "hash", UserRole::Viewer, false)
        .await
        .unwrap();
    let user2 = users::create(&pool, "user2", "hash", UserRole::Viewer, false)
        .await
        .unwrap();

    // 1回目は成功
    mark_as_used(&pool, invitation.id, user1.id).await.unwrap();

    // 2回目は失敗
    let result = mark_as_used(&pool, invitation.id, user2.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_revoke_invitation() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // 無効化
    let revoked = revoke(&pool, invitation.id).await.unwrap();
    assert!(revoked);

    // ステータス確認
    let found = find_by_code(&pool, &invitation.code)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, InvitationStatus::Revoked);

    // 無効化済みは使用不可
    let user = users::create(&pool, "user", "hash", UserRole::Viewer, false)
        .await
        .unwrap();
    let result = mark_as_used(&pool, invitation.id, user.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_validate_expired_invitation() {
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now() - Duration::hours(100),
        expires_at: Utc::now() - Duration::hours(1), // 1時間前に期限切れ
        status: InvitationStatus::Active,
        used_by: None,
        used_at: None,
    };

    let result = validate_invitation(&invitation);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expired"));
}

#[tokio::test]
async fn test_list_invitations() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    create(&pool, admin.id, None).await.unwrap();
    create(&pool, admin.id, Some(24)).await.unwrap();

    let invitations = list(&pool).await.unwrap();
    assert_eq!(invitations.len(), 2);
}

// --- Additional tests ---

#[test]
fn test_invitation_status_display() {
    assert_eq!(InvitationStatus::Active.to_string(), "active");
    assert_eq!(InvitationStatus::Used.to_string(), "used");
    assert_eq!(InvitationStatus::Revoked.to_string(), "revoked");
}

#[test]
fn test_invitation_status_from_str() {
    assert_eq!(
        "active".parse::<InvitationStatus>().unwrap(),
        InvitationStatus::Active
    );
    assert_eq!(
        "used".parse::<InvitationStatus>().unwrap(),
        InvitationStatus::Used
    );
    assert_eq!(
        "revoked".parse::<InvitationStatus>().unwrap(),
        InvitationStatus::Revoked
    );
    assert!("invalid".parse::<InvitationStatus>().is_err());
}

#[test]
fn test_hash_with_sha256_deterministic() {
    let h1 = hash_with_sha256("test_code");
    let h2 = hash_with_sha256("test_code");
    assert_eq!(h1, h2);
    // Different input produces different hash
    let h3 = hash_with_sha256("other_code");
    assert_ne!(h1, h3);
}

#[test]
fn test_validate_used_invitation() {
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(72),
        status: InvitationStatus::Used,
        used_by: Some(Uuid::new_v4()),
        used_at: Some(Utc::now()),
    };
    let result = validate_invitation(&invitation);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("used"));
}

#[test]
fn test_validate_revoked_invitation() {
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(72),
        status: InvitationStatus::Revoked,
        used_by: None,
        used_at: None,
    };
    let result = validate_invitation(&invitation);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("revoked"));
}

#[test]
fn test_validate_active_not_expired_invitation() {
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(72),
        status: InvitationStatus::Active,
        used_by: None,
        used_at: None,
    };
    assert!(validate_invitation(&invitation).is_ok());
}

#[tokio::test]
async fn test_delete_invitation() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // Delete the invitation
    delete(&pool, invitation.id).await.unwrap();

    // Should no longer be found
    let found = find_by_code(&pool, &invitation.code).await.unwrap();
    assert!(found.is_none());

    // List should be empty
    let all = list(&pool).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_find_by_hash_not_found() {
    let pool = setup_test_db().await;

    let found = find_by_hash(&pool, "nonexistent_hash").await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_revoke_already_used_invitation_fails() {
    let pool = setup_test_db().await;

    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // Use the invitation first
    let user = users::create(&pool, "user", "hash", UserRole::Viewer, false)
        .await
        .unwrap();
    mark_as_used(&pool, invitation.id, user.id).await.unwrap();

    // Revoking a used invitation should return false (no rows affected)
    let revoked = revoke(&pool, invitation.id).await.unwrap();
    assert!(!revoked);
}

// =====================================================================
// 追加テスト: generate_invitation_code
// =====================================================================

#[test]
fn test_generate_invitation_code_format() {
    let code = generate_invitation_code();
    assert!(code.starts_with("inv_"));
    assert_eq!(code.len(), 20); // "inv_" (4) + 16 chars

    // All characters after prefix are alphanumeric
    let suffix = &code[4..];
    assert!(suffix.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn test_generate_invitation_code_uniqueness() {
    let code1 = generate_invitation_code();
    let code2 = generate_invitation_code();
    assert_ne!(code1, code2);
}

// =====================================================================
// 追加テスト: hash_with_sha256
// =====================================================================

#[test]
fn test_hash_with_sha256_length() {
    let hash = hash_with_sha256("test");
    assert_eq!(hash.len(), 64);
}

#[test]
fn test_hash_with_sha256_hex_chars() {
    let hash = hash_with_sha256("test");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_hash_with_sha256_empty_input() {
    let hash = hash_with_sha256("");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

// =====================================================================
// 追加テスト: InvitationStatus serde
// =====================================================================

#[test]
fn test_invitation_status_serialize() {
    let json = serde_json::to_string(&InvitationStatus::Active).unwrap();
    assert_eq!(json, "\"active\"");

    let json = serde_json::to_string(&InvitationStatus::Used).unwrap();
    assert_eq!(json, "\"used\"");

    let json = serde_json::to_string(&InvitationStatus::Revoked).unwrap();
    assert_eq!(json, "\"revoked\"");
}

#[test]
fn test_invitation_status_deserialize() {
    let active: InvitationStatus = serde_json::from_str("\"active\"").unwrap();
    assert_eq!(active, InvitationStatus::Active);

    let used: InvitationStatus = serde_json::from_str("\"used\"").unwrap();
    assert_eq!(used, InvitationStatus::Used);

    let revoked: InvitationStatus = serde_json::from_str("\"revoked\"").unwrap();
    assert_eq!(revoked, InvitationStatus::Revoked);
}

#[test]
fn test_invitation_status_from_str_case_sensitive() {
    // Must be lowercase
    assert!("Active".parse::<InvitationStatus>().is_err());
    assert!("ACTIVE".parse::<InvitationStatus>().is_err());
    assert!("USED".parse::<InvitationStatus>().is_err());
}

// =====================================================================
// 追加テスト: validate_invitation edge cases
// =====================================================================

#[test]
fn test_validate_invitation_just_expired() {
    // Exactly at expiry boundary (already expired)
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now() - Duration::hours(73),
        expires_at: Utc::now() - Duration::seconds(1),
        status: InvitationStatus::Active,
        used_by: None,
        used_at: None,
    };
    let result = validate_invitation(&invitation);
    assert!(result.is_err());
}

#[test]
fn test_validate_invitation_far_future_expiry() {
    let invitation = InvitationCode {
        id: Uuid::new_v4(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::days(365),
        status: InvitationStatus::Active,
        used_by: None,
        used_at: None,
    };
    assert!(validate_invitation(&invitation).is_ok());
}

// =====================================================================
// 追加テスト: InvitationCodeRow -> InvitationCode conversion
// =====================================================================

#[test]
fn test_invitation_code_row_conversion() {
    let id = Uuid::new_v4();
    let created_by = Uuid::new_v4();
    let now = Utc::now();
    let row = InvitationCodeRow {
        id: id.to_string(),
        code_hash: "hash123".to_string(),
        created_by: created_by.to_string(),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::hours(72)).to_rfc3339(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };

    let invitation = row.try_into_invitation_code().unwrap();
    assert_eq!(invitation.id, id);
    assert_eq!(invitation.code_hash, "hash123");
    assert_eq!(invitation.created_by, created_by);
    assert_eq!(invitation.status, InvitationStatus::Active);
    assert!(invitation.used_by.is_none());
    assert!(invitation.used_at.is_none());
}

#[test]
fn test_invitation_code_row_with_used_fields() {
    let id = Uuid::new_v4();
    let created_by = Uuid::new_v4();
    let used_by = Uuid::new_v4();
    let now = Utc::now();
    let row = InvitationCodeRow {
        id: id.to_string(),
        code_hash: "hash".to_string(),
        created_by: created_by.to_string(),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::hours(72)).to_rfc3339(),
        status: "used".to_string(),
        used_by: Some(used_by.to_string()),
        used_at: Some(now.to_rfc3339()),
    };

    let invitation = row.try_into_invitation_code().unwrap();
    assert_eq!(invitation.status, InvitationStatus::Used);
    assert_eq!(invitation.used_by, Some(used_by));
    assert!(invitation.used_at.is_some());
}

#[test]
fn test_invitation_code_row_invalid_uuid() {
    let now = Utc::now();
    let row = InvitationCodeRow {
        id: "not-a-uuid".to_string(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4().to_string(),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::hours(72)).to_rfc3339(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };

    let result = row.try_into_invitation_code();
    assert!(result.is_err());
}

#[test]
fn test_invitation_code_row_invalid_created_at() {
    let row = InvitationCodeRow {
        id: Uuid::new_v4().to_string(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4().to_string(),
        created_at: "not-a-date".to_string(),
        expires_at: Utc::now().to_rfc3339(),
        status: "active".to_string(),
        used_by: None,
        used_at: None,
    };

    let result = row.try_into_invitation_code();
    assert!(result.is_err());
}

#[test]
fn test_invitation_code_row_invalid_status() {
    let now = Utc::now();
    let row = InvitationCodeRow {
        id: Uuid::new_v4().to_string(),
        code_hash: "hash".to_string(),
        created_by: Uuid::new_v4().to_string(),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::hours(72)).to_rfc3339(),
        status: "unknown_status".to_string(),
        used_by: None,
        used_at: None,
    };

    let result = row.try_into_invitation_code();
    assert!(result.is_err());
}

// =====================================================================
// 追加テスト: DB操作 - create with custom expiry
// =====================================================================

#[tokio::test]
async fn test_create_with_custom_expiry() {
    let pool = setup_test_db().await;
    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, Some(1)).await.unwrap();

    // Should expire in ~1 hour
    let diff = invitation.expires_at - invitation.created_at;
    assert_eq!(diff.num_hours(), 1);
}

#[tokio::test]
async fn test_create_with_default_expiry() {
    let pool = setup_test_db().await;
    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // Default: 72 hours
    let diff = invitation.expires_at - invitation.created_at;
    assert_eq!(diff.num_hours(), 72);
}

// =====================================================================
// 追加テスト: DB操作 - revoke nonexistent
// =====================================================================

#[tokio::test]
async fn test_revoke_nonexistent_invitation() {
    let pool = setup_test_db().await;

    let revoked = revoke(&pool, Uuid::new_v4()).await.unwrap();
    assert!(!revoked);
}

// =====================================================================
// 追加テスト: DB操作 - mark_as_used nonexistent
// =====================================================================

#[tokio::test]
async fn test_mark_as_used_nonexistent_invitation() {
    let pool = setup_test_db().await;

    let result = mark_as_used(&pool, Uuid::new_v4(), Uuid::new_v4()).await;
    assert!(result.is_err());
}

// =====================================================================
// 追加テスト: DB操作 - delete nonexistent (no-op)
// =====================================================================

#[tokio::test]
async fn test_delete_nonexistent_invitation_is_noop() {
    let pool = setup_test_db().await;
    delete(&pool, Uuid::new_v4()).await.unwrap();
}

// =====================================================================
// 追加テスト: DB操作 - list empty
// =====================================================================

#[tokio::test]
async fn test_list_empty_db() {
    let pool = setup_test_db().await;
    let invitations = list(&pool).await.unwrap();
    assert!(invitations.is_empty());
}

// =====================================================================
// 追加テスト: DB操作 - find_by_code not found
// =====================================================================

#[tokio::test]
async fn test_find_by_code_not_found() {
    let pool = setup_test_db().await;
    let found = find_by_code(&pool, "nonexistent_code").await.unwrap();
    assert!(found.is_none());
}

// =====================================================================
// 追加テスト: DB操作 - revoke already revoked
// =====================================================================

#[tokio::test]
async fn test_revoke_already_revoked_invitation() {
    let pool = setup_test_db().await;
    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();

    // Revoke first time
    let revoked = revoke(&pool, invitation.id).await.unwrap();
    assert!(revoked);

    // Revoke second time should return false
    let revoked_again = revoke(&pool, invitation.id).await.unwrap();
    assert!(!revoked_again);
}

// =====================================================================
// 追加テスト: DB操作 - mark_as_used on revoked invitation
// =====================================================================

#[tokio::test]
async fn test_mark_as_used_revoked_invitation() {
    let pool = setup_test_db().await;
    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let invitation = create(&pool, admin.id, None).await.unwrap();
    revoke(&pool, invitation.id).await.unwrap();

    let user = users::create(&pool, "user", "hash", UserRole::Viewer, false)
        .await
        .unwrap();
    let result = mark_as_used(&pool, invitation.id, user.id).await;
    assert!(result.is_err());
}

// =====================================================================
// 追加テスト: DB操作 - create multiple from same admin
// =====================================================================

#[tokio::test]
async fn test_create_multiple_invitations_same_admin() {
    let pool = setup_test_db().await;
    let admin = users::create(&pool, "admin", "hash", UserRole::Admin, false)
        .await
        .unwrap();

    let inv1 = create(&pool, admin.id, None).await.unwrap();
    let inv2 = create(&pool, admin.id, None).await.unwrap();
    let inv3 = create(&pool, admin.id, Some(24)).await.unwrap();

    assert_ne!(inv1.id, inv2.id);
    assert_ne!(inv2.id, inv3.id);

    let all = list(&pool).await.unwrap();
    assert_eq!(all.len(), 3);

    // All should have same created_by
    for inv in &all {
        assert_eq!(inv.created_by, admin.id);
    }
}

// =====================================================================
// 追加テスト: InvitationCodeWithPlaintext serialize
// =====================================================================

#[test]
fn test_invitation_code_with_plaintext_serialize() {
    let inv = InvitationCodeWithPlaintext {
        id: Uuid::new_v4(),
        code: "inv_test1234567890ab".to_string(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::hours(72),
    };
    let json = serde_json::to_value(&inv).unwrap();
    assert!(json["id"].is_string());
    assert_eq!(json["code"], "inv_test1234567890ab");
    assert!(json["created_at"].is_string());
    assert!(json["expires_at"].is_string());
}
