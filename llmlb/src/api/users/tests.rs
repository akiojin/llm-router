use super::*;
use chrono::Utc;

// --- CreateUserRequest deserialization ---

#[test]
fn create_user_request_deserializes_admin() {
    let json = r#"{"username":"alice","role":"admin"}"#;
    let req: CreateUserRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.username, "alice");
    assert_eq!(req.role, UserRole::Admin);
}

#[test]
fn create_user_request_deserializes_viewer() {
    let json = r#"{"username":"bob","role":"viewer"}"#;
    let req: CreateUserRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.username, "bob");
    assert_eq!(req.role, UserRole::Viewer);
}

#[test]
fn create_user_request_missing_username_fails() {
    let json = r#"{"role":"admin"}"#;
    let result = serde_json::from_str::<CreateUserRequest>(json);
    assert!(result.is_err());
}

#[test]
fn create_user_request_missing_role_fails() {
    let json = r#"{"username":"alice"}"#;
    let result = serde_json::from_str::<CreateUserRequest>(json);
    assert!(result.is_err());
}

#[test]
fn create_user_request_invalid_role_fails() {
    let json = r#"{"username":"alice","role":"superuser"}"#;
    let result = serde_json::from_str::<CreateUserRequest>(json);
    assert!(result.is_err());
}

#[test]
fn create_user_request_empty_username() {
    let json = r#"{"username":"","role":"admin"}"#;
    let req: CreateUserRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.username, "");
}

// --- UpdateUserRequest deserialization ---

#[test]
fn update_user_request_all_fields() {
    let json = r#"{"username":"new_name","password":"new_pass","role":"viewer"}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.username, Some("new_name".to_string()));
    assert_eq!(req.password, Some("new_pass".to_string()));
    assert_eq!(req.role, Some(UserRole::Viewer));
}

#[test]
fn update_user_request_empty_body() {
    let json = r#"{}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert!(req.username.is_none());
    assert!(req.password.is_none());
    assert!(req.role.is_none());
}

#[test]
fn update_user_request_username_only() {
    let json = r#"{"username":"updated"}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.username, Some("updated".to_string()));
    assert!(req.password.is_none());
    assert!(req.role.is_none());
}

#[test]
fn update_user_request_password_only() {
    let json = r#"{"password":"secret123"}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert!(req.username.is_none());
    assert_eq!(req.password, Some("secret123".to_string()));
}

#[test]
fn update_user_request_role_only() {
    let json = r#"{"role":"admin"}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert!(req.username.is_none());
    assert!(req.password.is_none());
    assert_eq!(req.role, Some(UserRole::Admin));
}

#[test]
fn update_user_request_null_fields() {
    let json = r#"{"username":null,"password":null,"role":null}"#;
    let req: UpdateUserRequest = serde_json::from_str(json).unwrap();
    assert!(req.username.is_none());
    assert!(req.password.is_none());
    assert!(req.role.is_none());
}

// --- UserResponse serialization ---

#[test]
fn user_response_serialization() {
    let resp = UserResponse {
        id: "123".to_string(),
        username: "alice".to_string(),
        role: "admin".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        last_login: Some("2025-06-01T12:00:00Z".to_string()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"id\":\"123\""));
    assert!(json.contains("\"username\":\"alice\""));
    assert!(json.contains("\"role\":\"admin\""));
    assert!(json.contains("\"last_login\":\"2025-06-01T12:00:00Z\""));
}

#[test]
fn user_response_no_last_login() {
    let resp = UserResponse {
        id: "456".to_string(),
        username: "bob".to_string(),
        role: "viewer".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        last_login: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"last_login\":null"));
}

// --- From<User> for UserResponse ---

#[test]
fn user_response_from_admin_user() {
    let user = User {
        id: Uuid::new_v4(),
        username: "admin_user".to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::Admin,
        created_at: Utc::now(),
        last_login: Some(Utc::now()),
        must_change_password: false,
        password_changed_at: 0,
    };
    let resp = UserResponse::from(user.clone());
    assert_eq!(resp.id, user.id.to_string());
    assert_eq!(resp.username, "admin_user");
    assert_eq!(resp.role, "admin");
    assert!(resp.last_login.is_some());
}

#[test]
fn user_response_from_viewer_user() {
    let user = User {
        id: Uuid::new_v4(),
        username: "viewer_user".to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::Viewer,
        created_at: Utc::now(),
        last_login: None,
        must_change_password: true,
        password_changed_at: 0,
    };
    let resp = UserResponse::from(user.clone());
    assert_eq!(resp.username, "viewer_user");
    assert_eq!(resp.role, "viewer");
    assert!(resp.last_login.is_none());
}

#[test]
fn user_response_created_at_is_rfc3339() {
    let user = User {
        id: Uuid::new_v4(),
        username: "test".to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::Admin,
        created_at: Utc::now(),
        last_login: None,
        must_change_password: false,
        password_changed_at: 0,
    };
    let resp = UserResponse::from(user);
    // RFC 3339 timestamps contain "T" and "+" or "Z"
    assert!(resp.created_at.contains('T'));
}

#[test]
fn user_response_last_login_rfc3339() {
    let now = Utc::now();
    let user = User {
        id: Uuid::new_v4(),
        username: "test".to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::Admin,
        created_at: now,
        last_login: Some(now),
        must_change_password: false,
        password_changed_at: 0,
    };
    let resp = UserResponse::from(user);
    let ll = resp.last_login.unwrap();
    assert!(ll.contains('T'));
}

// --- CreateUserResponse serialization ---

#[test]
fn create_user_response_serialization() {
    let resp = CreateUserResponse {
        user: UserResponse {
            id: "1".to_string(),
            username: "alice".to_string(),
            role: "admin".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            last_login: None,
        },
        generated_password: "random_password_123".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"generated_password\":\"random_password_123\""));
    assert!(json.contains("\"username\":\"alice\""));
}

// --- ListUsersResponse serialization ---

#[test]
fn list_users_response_empty() {
    let resp = ListUsersResponse { users: vec![] };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"users\":[]"));
}

#[test]
fn list_users_response_multiple_users() {
    let resp = ListUsersResponse {
        users: vec![
            UserResponse {
                id: "1".to_string(),
                username: "alice".to_string(),
                role: "admin".to_string(),
                created_at: "2025-01-01T00:00:00Z".to_string(),
                last_login: None,
            },
            UserResponse {
                id: "2".to_string(),
                username: "bob".to_string(),
                role: "viewer".to_string(),
                created_at: "2025-06-01T00:00:00Z".to_string(),
                last_login: Some("2025-06-15T00:00:00Z".to_string()),
            },
        ],
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("alice"));
    assert!(json.contains("bob"));
}

// --- UserRole serde in context of requests ---

#[test]
fn user_role_admin_lowercase_in_json() {
    let json = serde_json::to_string(&UserRole::Admin).unwrap();
    assert_eq!(json, "\"admin\"");
}

#[test]
fn user_role_viewer_lowercase_in_json() {
    let json = serde_json::to_string(&UserRole::Viewer).unwrap();
    assert_eq!(json, "\"viewer\"");
}

#[test]
fn user_role_unknown_value_fails() {
    let result = serde_json::from_str::<UserRole>("\"moderator\"");
    assert!(result.is_err());
}
