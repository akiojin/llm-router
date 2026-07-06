
use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn test_logout_returns_no_content() {
    let response = logout(HeaderMap::new()).await.into_response();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[test]
fn test_login_request_deserialize() {
    let json = r#"{"username": "admin", "password": "secret"}"#;
    let request: LoginRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.username, "admin");
    assert_eq!(request.password, "secret");
}

#[test]
fn test_login_response_serialize() {
    let response = LoginResponse {
        token: "jwt_token".to_string(),
        expires_in: 86400,
        user: UserInfo {
            id: "user-id".to_string(),
            username: "admin".to_string(),
            role: "admin".to_string(),
            must_change_password: false,
        },
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("jwt_token"));
    assert!(json.contains("86400"));
    assert!(json.contains("admin"));
}

#[test]
fn test_me_response_serialize() {
    let response = MeResponse {
        user_id: "user-123".to_string(),
        username: "testuser".to_string(),
        role: "user".to_string(),
        must_change_password: false,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("user-123"));
    assert!(json.contains("testuser"));
    assert!(json.contains("user"));
}

#[test]
fn test_user_info_serialize() {
    let info = UserInfo {
        id: "id-456".to_string(),
        username: "bob".to_string(),
        role: "admin".to_string(),
        must_change_password: false,
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("id-456"));
    assert!(json.contains("bob"));
    assert!(json.contains("admin"));
}

// =========================================================================
// logout tests (additional)
// =========================================================================

#[tokio::test]
async fn test_logout_clears_cookies() {
    let response = logout(HeaderMap::new()).await.into_response();
    let set_cookies: Vec<&str> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert!(
        set_cookies.len() >= 2,
        "logout should set at least 2 Set-Cookie headers"
    );
    let has_jwt_clear = set_cookies
        .iter()
        .any(|c| c.contains(crate::auth::DASHBOARD_JWT_COOKIE) && c.contains("Max-Age=0"));
    let has_csrf_clear = set_cookies
        .iter()
        .any(|c| c.contains(crate::auth::DASHBOARD_CSRF_COOKIE) && c.contains("Max-Age=0"));
    assert!(has_jwt_clear, "should clear JWT cookie");
    assert!(has_csrf_clear, "should clear CSRF cookie");
}

#[tokio::test]
async fn test_logout_sets_secure_flag_behind_https() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-proto", "https".parse().unwrap());
    let response = logout(headers).await.into_response();
    let set_cookies: Vec<&str> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert!(
        set_cookies.iter().all(|c| c.contains("Secure")),
        "all cookies should have Secure flag behind HTTPS"
    );
}

#[tokio::test]
async fn test_logout_no_secure_flag_for_http() {
    let response = logout(HeaderMap::new()).await.into_response();
    let set_cookies: Vec<&str> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert!(
        set_cookies.iter().all(|c| !c.contains("Secure")),
        "cookies should NOT have Secure flag for HTTP"
    );
}

// =========================================================================
// LoginRequest deserialization tests
// =========================================================================

#[test]
fn test_login_request_missing_field_fails() {
    let json = r#"{"username": "admin"}"#;
    let result = serde_json::from_str::<LoginRequest>(json);
    assert!(result.is_err());
}

#[test]
fn test_login_request_empty_strings() {
    let json = r#"{"username": "", "password": ""}"#;
    let request: LoginRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.username, "");
    assert_eq!(request.password, "");
}

// =========================================================================
// ChangePasswordRequest deserialization tests
// =========================================================================

#[test]
fn test_change_password_request_deserialize() {
    let json = r#"{"new_password": "newpass123"}"#;
    let request: ChangePasswordRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.new_password, "newpass123");
}

#[test]
fn test_change_password_request_missing_field_fails() {
    let json = r#"{}"#;
    let result = serde_json::from_str::<ChangePasswordRequest>(json);
    assert!(result.is_err());
}

// =========================================================================
// RegisterRequest deserialization tests
// =========================================================================

#[test]
fn test_register_request_deserialize() {
    let json = r#"{"invitation_code": "ABC123", "username": "newuser", "password": "secret"}"#;
    let request: RegisterRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.invitation_code, "ABC123");
    assert_eq!(request.username, "newuser");
    assert_eq!(request.password, "secret");
}

#[test]
fn test_register_request_missing_field_fails() {
    let json = r#"{"username": "newuser", "password": "secret"}"#;
    let result = serde_json::from_str::<RegisterRequest>(json);
    assert!(result.is_err());
}

// =========================================================================
// RegisterResponse serialization tests
// =========================================================================

#[test]
fn test_register_response_serialize() {
    let response = RegisterResponse {
        id: "user-id-1".to_string(),
        username: "testuser".to_string(),
        role: "viewer".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("user-id-1"));
    assert!(json.contains("testuser"));
    assert!(json.contains("viewer"));
    assert!(json.contains("2024-01-01T00:00:00Z"));
}

// =========================================================================
// LoginResponse serialization tests (additional)
// =========================================================================

#[test]
fn test_login_response_contains_all_fields() {
    let response = LoginResponse {
        token: "eyJhbGciOiJIUzI1NiJ9.payload.sig".to_string(),
        expires_in: 3600,
        user: UserInfo {
            id: "uuid-1".to_string(),
            username: "alice".to_string(),
            role: "viewer".to_string(),
            must_change_password: true,
        },
    };
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("token").is_some());
    assert!(parsed.get("expires_in").is_some());
    assert!(parsed.get("user").is_some());
    let user = parsed.get("user").unwrap();
    assert_eq!(user["must_change_password"], true);
}

// =========================================================================
// MeResponse serialization tests (additional)
// =========================================================================

#[test]
fn test_me_response_must_change_password_true() {
    let response = MeResponse {
        user_id: "uid".to_string(),
        username: "bob".to_string(),
        role: "admin".to_string(),
        must_change_password: true,
    };
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["must_change_password"], true);
}

#[test]
fn test_me_response_all_fields_present() {
    let response = MeResponse {
        user_id: "uid-abc".to_string(),
        username: "charlie".to_string(),
        role: "viewer".to_string(),
        must_change_password: false,
    };
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("user_id").is_some());
    assert!(parsed.get("username").is_some());
    assert!(parsed.get("role").is_some());
    assert!(parsed.get("must_change_password").is_some());
}

// =========================================================================
// UserInfo serialization tests (additional)
// =========================================================================

#[test]
fn test_user_info_must_change_password_true() {
    let info = UserInfo {
        id: "id-1".to_string(),
        username: "alice".to_string(),
        role: "admin".to_string(),
        must_change_password: true,
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["must_change_password"], true);
}

#[test]
fn test_user_info_all_fields_present() {
    let info = UserInfo {
        id: "id-2".to_string(),
        username: "user".to_string(),
        role: "viewer".to_string(),
        must_change_password: false,
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("id").is_some());
    assert!(parsed.get("username").is_some());
    assert!(parsed.get("role").is_some());
    assert!(parsed.get("must_change_password").is_some());
}
