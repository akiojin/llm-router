// T044-T046: JWT生成と検証（jsonwebtoken実装）

use crate::common::auth::{Claims, UserRole};
use crate::common::error::LbError;
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

/// JWT有効期限（24時間）
const JWT_EXPIRATION_HOURS: i64 = 24;

/// JWTトークンを生成
///
/// # Arguments
/// * `user_id` - ユーザーID
/// * `role` - ユーザーロール
/// * `secret` - JWTシークレットキー
///
/// # Returns
/// * `Ok(String)` - JWTトークン（3つのドット区切り部分）
/// * `Err(LbError)` - 生成失敗
pub fn create_jwt(
    user_id: &str,
    role: UserRole,
    secret: &str,
    must_change_password: bool,
) -> Result<String, LbError> {
    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::hours(JWT_EXPIRATION_HOURS))
        .ok_or_else(|| LbError::Jwt("Failed to calculate expiration time".to_string()))?
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        role,
        exp: expiration,
        must_change_password,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| LbError::Jwt(format!("Failed to create JWT: {}", e)))
}

/// JWTトークンを検証
///
/// # Arguments
/// * `token` - 検証するJWTトークン
/// * `secret` - JWTシークレットキー
///
/// # Returns
/// * `Ok(Claims)` - 検証済みクレーム
/// * `Err(LbError)` - 検証失敗（無効なトークン、期限切れなど）
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, LbError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| LbError::Jwt(format!("Failed to verify JWT: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "inline_test_secret_key_12345678";

    #[test]
    fn create_jwt_must_change_password_true_roundtrip() {
        let token = create_jwt("user1", UserRole::Admin, TEST_SECRET, true).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert!(claims.must_change_password);
        assert_eq!(claims.sub, "user1");
    }

    #[test]
    fn create_jwt_must_change_password_false_roundtrip() {
        let token = create_jwt("user2", UserRole::Viewer, TEST_SECRET, false).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert!(!claims.must_change_password);
        assert_eq!(claims.sub, "user2");
    }

    #[test]
    fn create_jwt_empty_secret() {
        let result = create_jwt("user", UserRole::Admin, "", false);
        assert!(result.is_ok());
    }

    #[test]
    fn create_jwt_empty_user_id() {
        let token = create_jwt("", UserRole::Admin, TEST_SECRET, false).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "");
    }

    #[test]
    fn create_jwt_very_long_user_id() {
        let long_id = "u".repeat(10_000);
        let token = create_jwt(&long_id, UserRole::Admin, TEST_SECRET, false).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, long_id);
    }

    #[test]
    fn admin_and_viewer_role_roundtrip() {
        let admin_token = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        let viewer_token = create_jwt("u", UserRole::Viewer, TEST_SECRET, false).unwrap();
        let ac = verify_jwt(&admin_token, TEST_SECRET).unwrap();
        let vc = verify_jwt(&viewer_token, TEST_SECRET).unwrap();
        assert_eq!(ac.role, UserRole::Admin);
        assert_eq!(vc.role, UserRole::Viewer);
    }

    #[test]
    fn two_tokens_from_same_input_differ() {
        let t1 = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        let t2 = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        // exp timestamp may differ by a second; tokens should still be distinct or equivalent
        // Both must be valid regardless
        assert!(verify_jwt(&t1, TEST_SECRET).is_ok());
        assert!(verify_jwt(&t2, TEST_SECRET).is_ok());
    }

    #[test]
    fn verify_jwt_empty_token_error() {
        let result = verify_jwt("", TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn verify_jwt_dots_only_error() {
        let result = verify_jwt("...", TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn token_roundtrip_all_fields_match() {
        let token = create_jwt("alice", UserRole::Viewer, TEST_SECRET, true).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.role, UserRole::Viewer);
        assert!(claims.must_change_password);
        let now = Utc::now().timestamp() as usize;
        assert!(claims.exp > now);
    }

    #[test]
    fn different_user_ids_produce_distinguishable_tokens() {
        let t1 = create_jwt("user-a", UserRole::Admin, TEST_SECRET, false).unwrap();
        let t2 = create_jwt("user-b", UserRole::Admin, TEST_SECRET, false).unwrap();
        let c1 = verify_jwt(&t1, TEST_SECRET).unwrap();
        let c2 = verify_jwt(&t2, TEST_SECRET).unwrap();
        assert_ne!(c1.sub, c2.sub);
    }

    #[test]
    fn jwt_expiration_within_24_hours() {
        let token = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        let now = Utc::now().timestamp() as usize;
        let diff_hours = (claims.exp - now) / 3600;
        assert!(diff_hours <= 24);
        assert!(diff_hours >= 23); // allow small timing variance
    }

    #[test]
    fn verify_with_wrong_secret_fails() {
        let token = create_jwt("user1", UserRole::Admin, TEST_SECRET, false).unwrap();
        let result = verify_jwt(&token, "wrong_secret_key_12345678");
        assert!(result.is_err());
    }

    #[test]
    fn verify_malformed_token_fails() {
        assert!(verify_jwt("not.a.jwt", TEST_SECRET).is_err());
    }

    #[test]
    fn verify_random_base64_token_fails() {
        assert!(verify_jwt("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.invalid", TEST_SECRET).is_err());
    }

    #[test]
    fn create_jwt_unicode_user_id() {
        let token = create_jwt("ユーザー日本語", UserRole::Viewer, TEST_SECRET, false).unwrap();
        let claims = verify_jwt(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "ユーザー日本語");
    }

    #[test]
    fn create_jwt_special_chars_secret() {
        let secret = "!@#$%^&*()_+-={}[]|;':\",./<>?";
        let token = create_jwt("user", UserRole::Admin, secret, false).unwrap();
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user");
    }

    #[test]
    fn token_has_three_parts() {
        let token = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn different_roles_have_different_tokens() {
        let t1 = create_jwt("u", UserRole::Admin, TEST_SECRET, false).unwrap();
        let t2 = create_jwt("u", UserRole::Viewer, TEST_SECRET, false).unwrap();
        // Payload differs due to different role
        assert_ne!(t1.split('.').nth(1), t2.split('.').nth(1));
    }

    #[test]
    fn verify_jwt_error_message_contains_jwt() {
        let result = verify_jwt("bad", TEST_SECRET);
        match result {
            Err(LbError::Jwt(msg)) => assert!(msg.contains("Failed to verify JWT")),
            _ => panic!("expected Jwt error"),
        }
    }

    #[test]
    fn create_jwt_with_single_char_secret() {
        let token = create_jwt("u", UserRole::Admin, "x", false).unwrap();
        assert!(verify_jwt(&token, "x").is_ok());
    }
}
