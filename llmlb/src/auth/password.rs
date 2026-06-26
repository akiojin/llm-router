// T042-T043: パスワードハッシュ化と検証（bcrypt実装）

use crate::common::error::LbError;
use bcrypt::{hash, verify};

/// パスワードハッシュ化のコスト（12推奨、200-300ms）
const HASH_COST: u32 = 12;

/// パスワードをbcryptでハッシュ化
///
/// # Arguments
/// * `password` - ハッシュ化するパスワード
///
/// # Returns
/// * `Ok(String)` - bcryptハッシュ文字列（$2b$で始まる）
/// * `Err(LbError)` - ハッシュ化失敗
pub fn hash_password(password: &str) -> Result<String, LbError> {
    hash(password, HASH_COST)
        .map_err(|e| LbError::PasswordHash(format!("Failed to hash password: {}", e)))
}

/// パスワードを検証
///
/// # Arguments
/// * `password` - 検証する平文パスワード
/// * `hash` - bcryptハッシュ文字列
///
/// # Returns
/// * `Ok(true)` - パスワード一致
/// * `Ok(false)` - パスワード不一致
/// * `Err(LbError)` - 検証失敗
pub fn verify_password(password: &str, hash: &str) -> Result<bool, LbError> {
    verify(password, hash)
        .map_err(|e| LbError::PasswordHash(format!("Failed to verify password: {}", e)))
}

/// パスワード要件を検証
///
/// # 要件
/// - 長さ: 8文字以上
/// - 大文字: 1文字以上
/// - 数字: 1文字以上
///
/// # Arguments
/// * `password` - 検証するパスワード
///
/// # Returns
/// * `Ok(())` - パスワード要件を満たす
/// * `Err(LbError)` - パスワード要件を満たさない（段階的エラーメッセージ付き）
pub fn validate_password(password: &str) -> Result<(), LbError> {
    // 長さチェック（8文字以上）
    if password.len() < 8 {
        return Err(LbError::Common(
            crate::common::error::CommonError::Validation("8文字以上必要です".to_string()),
        ));
    }

    // 大文字チェック（1文字以上）
    if !password.chars().any(|c| c.is_uppercase()) {
        return Err(LbError::Common(
            crate::common::error::CommonError::Validation(
                "大文字を1文字以上含めてください".to_string(),
            ),
        ));
    }

    // 数字チェック（1文字以上）
    if !password.chars().any(|c| c.is_numeric()) {
        return Err(LbError::Common(
            crate::common::error::CommonError::Validation(
                "数字を1文字以上含めてください".to_string(),
            ),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_password_hash_and_verify() {
        let pw = "\u{1F600}\u{65E5}\u{672C}\u{8A9E}\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
    }

    #[test]
    fn very_long_password_256_chars() {
        let pw = "A".repeat(256);
        let h = hash_password(&pw).unwrap();
        assert!(verify_password(&pw, &h).unwrap());
    }

    #[test]
    fn special_characters_password() {
        let pw = "!@#$%^&*()_+-=[]{}|;':\",./<>?`~";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
    }

    #[test]
    fn spaces_only_password() {
        let pw = "     ";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
    }

    #[test]
    fn newline_in_password() {
        let pw = "line1\nline2\r\nline3";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
    }

    #[test]
    fn same_password_verify_matches() {
        let pw = "consistent";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
        // second call also matches
        assert!(verify_password(pw, &h).unwrap());
    }

    #[test]
    fn invalid_hash_string_verify_error() {
        let result = verify_password("password", "not_a_valid_bcrypt_hash");
        assert!(result.is_err());
    }

    #[test]
    fn empty_password_hashes_and_verifies() {
        let h = hash_password("").unwrap();
        assert!(verify_password("", &h).unwrap());
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let h = hash_password("correct").unwrap();
        assert!(!verify_password("wrong", &h).unwrap());
    }

    #[test]
    fn hash_starts_with_bcrypt_prefix() {
        let h = hash_password("test").unwrap();
        assert!(h.starts_with("$2b$") || h.starts_with("$2a$") || h.starts_with("$2y$"));
    }

    #[test]
    fn same_password_produces_different_hashes() {
        let h1 = hash_password("same").unwrap();
        let h2 = hash_password("same").unwrap();
        assert_ne!(h1, h2); // bcrypt uses random salt
    }

    #[test]
    fn hash_has_expected_length() {
        let h = hash_password("test123").unwrap();
        assert_eq!(h.len(), 60); // bcrypt hash is always 60 chars
    }

    #[test]
    fn verify_error_returns_lb_error() {
        match verify_password("pw", "bad_hash") {
            Err(LbError::PasswordHash(msg)) => {
                assert!(msg.contains("Failed to verify password"));
            }
            _ => panic!("expected PasswordHash error"),
        }
    }

    #[test]
    fn null_byte_in_password() {
        let pw = "pass\0word";
        let h = hash_password(pw).unwrap();
        assert!(verify_password(pw, &h).unwrap());
    }

    // --- validate_password テストモジュール ---

    #[test]
    fn validate_password_fails_too_short() {
        // 長さ < 8文字でエラー返却
        let result = validate_password("Short1");
        assert!(
            result.is_err(),
            "Password shorter than 8 chars should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_no_uppercase() {
        // 大文字なしでエラー返却
        let result = validate_password("nouppercase123");
        assert!(
            result.is_err(),
            "Password without uppercase should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_no_digit() {
        // 数字なしでエラー返却
        let result = validate_password("NoDigitPassword");
        assert!(
            result.is_err(),
            "Password without digit should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_multiple_conditions() {
        // 複数条件不満足（短い + 大文字なし + 数字なし）
        let result = validate_password("short");
        assert!(
            result.is_err(),
            "Password failing multiple conditions should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_too_short_has_upper_and_digit() {
        // 長さのみ不満足（8文字未満だが大文字と数字を含む）
        let result = validate_password("Sh1rt");
        assert!(
            result.is_err(),
            "Password shorter than 8 chars should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_no_uppercase_meets_other_conditions() {
        // 大文字のみ不満足（8文字以上、数字ありだが大文字なし）
        let result = validate_password("lowercase1234");
        assert!(
            result.is_err(),
            "Password without uppercase should fail validation"
        );
    }

    #[test]
    fn validate_password_fails_no_digit_meets_other_conditions() {
        // 数字のみ不満足（8文字以上、大文字ありだが数字なし）
        let result = validate_password("NoDigitPasswordWithUppercase");
        assert!(
            result.is_err(),
            "Password without digit should fail validation"
        );
    }

    #[test]
    fn validate_password_succeeds_basic_valid() {
        // 有効なパスワード（8文字以上、大文字1以上、数字1以上）
        let result = validate_password("ValidPass123");
        assert!(result.is_ok(), "Valid password should pass validation");
    }

    #[test]
    fn validate_password_succeeds_strong_password() {
        // 強いパスワード（複雑な要件を満たす）
        let result = validate_password("MyP@ssw0rd!Secure");
        assert!(result.is_ok(), "Strong password should pass validation");
    }

    #[test]
    fn validate_password_succeeds_minimum_valid() {
        // 最小要件を満たすパスワード（8文字, 大文字1, 数字1）
        let result = validate_password("Minimal01");
        assert!(
            result.is_ok(),
            "Minimum valid password should pass validation"
        );
    }

    #[test]
    fn validate_password_succeeds_long_password() {
        // 長いパスワード（すべての要件を満たす）
        let result = validate_password("ThisIsAVeryLongPasswordWith123Numbers");
        assert!(result.is_ok(), "Long valid password should pass validation");
    }

    #[test]
    fn validate_password_succeeds_multiple_uppercase_digits() {
        // 複数の大文字と数字を含むパスワード
        let result = validate_password("MULTIPLEUpperCaseAnd123456Digits");
        assert!(
            result.is_ok(),
            "Password with multiple uppercase and digits should pass validation"
        );
    }
}
