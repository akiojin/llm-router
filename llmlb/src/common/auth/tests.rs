use super::*;
use std::collections::HashSet;

// --- UserRole ---

#[test]
fn user_role_admin_serde_roundtrip() {
    let json = serde_json::to_string(&UserRole::Admin).unwrap();
    assert_eq!(json, r#""admin""#);
    let back: UserRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, UserRole::Admin);
}

#[test]
fn user_role_viewer_serde_roundtrip() {
    let json = serde_json::to_string(&UserRole::Viewer).unwrap();
    assert_eq!(json, r#""viewer""#);
    let back: UserRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, UserRole::Viewer);
}

#[test]
fn user_role_invalid_string_fails() {
    let result = serde_json::from_str::<UserRole>(r#""superuser""#);
    assert!(result.is_err());
}

// --- ApiKeyPermission ---

#[test]
fn api_key_permission_openai_inference_serde_roundtrip() {
    let json = serde_json::to_string(&ApiKeyPermission::OpenaiInference).unwrap();
    assert_eq!(json, r#""openai.inference""#);
    let back: ApiKeyPermission = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ApiKeyPermission::OpenaiInference);
}

#[test]
fn api_key_permission_endpoints_manage_serde_roundtrip() {
    let json = serde_json::to_string(&ApiKeyPermission::EndpointsManage).unwrap();
    assert_eq!(json, r#""endpoints.manage""#);
    let back: ApiKeyPermission = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ApiKeyPermission::EndpointsManage);
}

#[test]
fn api_key_permission_all_contains_11_variants() {
    let all = ApiKeyPermission::all();
    assert_eq!(all.len(), 11);
}

#[test]
fn api_key_permission_all_no_duplicates() {
    let all = ApiKeyPermission::all();
    let set: HashSet<ApiKeyPermission> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len());
}

// --- ApiKey ---

#[test]
fn api_key_multiple_permissions() {
    let key = ApiKey {
        id: Uuid::new_v4(),
        key_hash: "hash".to_string(),
        key_prefix: Some("sk-".to_string()),
        name: "test-key".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: None,
        permissions: vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::EndpointsRead,
            ApiKeyPermission::MetricsRead,
        ],
    };
    assert_eq!(key.permissions.len(), 3);
    assert!(key.permissions.contains(&ApiKeyPermission::OpenaiInference));
    assert!(key.permissions.contains(&ApiKeyPermission::EndpointsRead));
    assert!(key.permissions.contains(&ApiKeyPermission::MetricsRead));
}

// --- Claims ---

#[test]
fn claims_serde_roundtrip() {
    let claims = Claims {
        sub: "user-123".to_string(),
        role: UserRole::Admin,
        exp: 1_700_000_000,
        must_change_password: true,
        password_changed_at: 0,
        username: None,
    };
    let json = serde_json::to_string(&claims).unwrap();
    let back: Claims = serde_json::from_str(&json).unwrap();
    assert_eq!(back, claims);
}

#[test]
fn claims_must_change_password_defaults_to_false() {
    let json = r#"{"sub":"u1","role":"viewer","exp":100}"#;
    let claims: Claims = serde_json::from_str(json).unwrap();
    assert!(!claims.must_change_password);
}

// --- User ---

#[test]
fn user_struct_basic() {
    let user = User {
        id: Uuid::new_v4(),
        username: "alice".to_string(),
        password_hash: "bcrypt_hash".to_string(),
        role: UserRole::Viewer,
        created_at: Utc::now(),
        last_login: None,
        must_change_password: false,
        password_changed_at: 0,
    };
    assert_eq!(user.username, "alice");
    assert_eq!(user.role, UserRole::Viewer);
    assert!(user.last_login.is_none());
}

// --- ApiKeyWithPlaintext ---

#[test]
fn api_key_with_plaintext_serialization() {
    let key = ApiKeyWithPlaintext {
        id: Uuid::new_v4(),
        key: "sk-plain-key".to_string(),
        key_prefix: "sk-pl".to_string(),
        name: "my-key".to_string(),
        created_at: Utc::now(),
        expires_at: None,
        permissions: vec![ApiKeyPermission::OpenaiInference],
    };
    let json = serde_json::to_string(&key).unwrap();
    assert!(json.contains("sk-plain-key"));
    assert!(json.contains("my-key"));
}

// --- RuntimeToken / RuntimeTokenWithPlaintext ---

#[test]
fn runtime_token_serde_roundtrip() {
    let token = RuntimeToken {
        runtime_id: Uuid::new_v4(),
        token_hash: "sha256hash".to_string(),
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&token).unwrap();
    let back: RuntimeToken = serde_json::from_str(&json).unwrap();
    assert_eq!(back.runtime_id, token.runtime_id);
    assert_eq!(back.token_hash, token.token_hash);
}

#[test]
fn runtime_token_with_plaintext_serialization() {
    let token = RuntimeTokenWithPlaintext {
        runtime_id: Uuid::new_v4(),
        token: "rt-plaintext".to_string(),
        created_at: Utc::now(),
    };
    let json = serde_json::to_string(&token).unwrap();
    assert!(json.contains("rt-plaintext"));
}

// --- Additional permission tests ---

#[test]
fn api_key_permission_all_serde_roundtrip() {
    for perm in ApiKeyPermission::all() {
        let json = serde_json::to_string(&perm).unwrap();
        let back: ApiKeyPermission = serde_json::from_str(&json).unwrap();
        assert_eq!(back, perm);
    }
}

#[test]
fn api_key_permission_openai_models_read_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::OpenaiModelsRead).unwrap();
    assert_eq!(json, r#""openai.models.read""#);
}

#[test]
fn api_key_permission_endpoints_read_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::EndpointsRead).unwrap();
    assert_eq!(json, r#""endpoints.read""#);
}

#[test]
fn api_key_permission_api_keys_manage_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::ApiKeysManage).unwrap();
    assert_eq!(json, r#""api_keys.manage""#);
}

#[test]
fn api_key_permission_users_manage_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::UsersManage).unwrap();
    assert_eq!(json, r#""users.manage""#);
}

#[test]
fn api_key_permission_invitations_manage_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::InvitationsManage).unwrap();
    assert_eq!(json, r#""invitations.manage""#);
}

#[test]
fn api_key_permission_models_manage_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::ModelsManage).unwrap();
    assert_eq!(json, r#""models.manage""#);
}

#[test]
fn api_key_permission_registry_read_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::RegistryRead).unwrap();
    assert_eq!(json, r#""registry.read""#);
}

#[test]
fn api_key_permission_logs_read_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::LogsRead).unwrap();
    assert_eq!(json, r#""logs.read""#);
}

#[test]
fn api_key_permission_metrics_read_serde() {
    let json = serde_json::to_string(&ApiKeyPermission::MetricsRead).unwrap();
    assert_eq!(json, r#""metrics.read""#);
}

#[test]
fn api_key_permission_invalid_string() {
    let result = serde_json::from_str::<ApiKeyPermission>(r#""invalid.permission""#);
    assert!(result.is_err());
}

// --- User struct tests ---

#[test]
fn user_serde_roundtrip() {
    let user = User {
        id: Uuid::new_v4(),
        username: "bob".to_string(),
        password_hash: "hash123".to_string(),
        role: UserRole::Admin,
        created_at: Utc::now(),
        last_login: Some(Utc::now()),
        must_change_password: true,
        password_changed_at: 0,
    };
    let json = serde_json::to_string(&user).unwrap();
    let back: User = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, user.id);
    assert_eq!(back.username, user.username);
    assert_eq!(back.role, UserRole::Admin);
    assert!(back.must_change_password);
    assert!(back.last_login.is_some());
}

#[test]
fn user_admin_role() {
    let user = User {
        id: Uuid::new_v4(),
        username: "admin".to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::Admin,
        created_at: Utc::now(),
        last_login: None,
        must_change_password: false,
        password_changed_at: 0,
    };
    assert_eq!(user.role, UserRole::Admin);
}

// --- ApiKey struct tests ---

#[test]
fn api_key_serde_roundtrip() {
    let key = ApiKey {
        id: Uuid::new_v4(),
        key_hash: "sha256hash".to_string(),
        key_prefix: Some("sk_".to_string()),
        name: "test".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: Some(Utc::now()),
        permissions: ApiKeyPermission::all(),
    };
    let json = serde_json::to_string(&key).unwrap();
    let back: ApiKey = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, key.id);
    assert_eq!(back.name, "test");
    assert_eq!(back.permissions.len(), 11);
}

#[test]
fn api_key_empty_permissions() {
    let key = ApiKey {
        id: Uuid::new_v4(),
        key_hash: "hash".to_string(),
        key_prefix: None,
        name: "empty".to_string(),
        created_by: Uuid::new_v4(),
        created_at: Utc::now(),
        expires_at: None,
        permissions: vec![],
    };
    assert!(key.permissions.is_empty());
    assert!(key.key_prefix.is_none());
    assert!(key.expires_at.is_none());
}

// --- Claims edge cases ---

#[test]
fn claims_admin_with_must_change() {
    let claims = Claims {
        sub: "admin-id".to_string(),
        role: UserRole::Admin,
        exp: 0,
        must_change_password: true,
        password_changed_at: 0,
        username: None,
    };
    assert!(claims.must_change_password);
    assert_eq!(claims.exp, 0);
}

#[test]
fn claims_viewer_role() {
    let claims = Claims {
        sub: "viewer-id".to_string(),
        role: UserRole::Viewer,
        exp: usize::MAX,
        must_change_password: false,
        password_changed_at: 0,
        username: None,
    };
    assert_eq!(claims.role, UserRole::Viewer);
}

// --- Debug/Clone tests ---

#[test]
fn user_role_debug() {
    assert_eq!(format!("{:?}", UserRole::Admin), "Admin");
    assert_eq!(format!("{:?}", UserRole::Viewer), "Viewer");
}

#[test]
fn user_role_clone() {
    let role = UserRole::Admin;
    let cloned = role;
    assert_eq!(role, cloned);
}

#[test]
fn api_key_permission_clone() {
    let perm = ApiKeyPermission::OpenaiInference;
    let cloned = perm;
    assert_eq!(perm, cloned);
}

#[test]
fn api_key_with_plaintext_with_expiry() {
    let key = ApiKeyWithPlaintext {
        id: Uuid::new_v4(),
        key: "sk-long-key".to_string(),
        key_prefix: "sk-lo".to_string(),
        name: "with-expiry".to_string(),
        created_at: Utc::now(),
        expires_at: Some(Utc::now()),
        permissions: vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::LogsRead,
        ],
    };
    let json = serde_json::to_string(&key).unwrap();
    assert!(json.contains("expires_at"));
    assert!(json.contains("with-expiry"));
}
