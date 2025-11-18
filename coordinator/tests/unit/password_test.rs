// T027-T028: パスワードハッシュ化と検証のユニットテスト（RED）

#[cfg(test)]
mod password_tests {
    use ollama_coordinator_coordinator::auth::password::{hash_password, verify_password};

    #[test]
    fn test_hash_password_creates_valid_hash() {
        // Given: 平文パスワード
        let password = "secure_password123";

        // When: パスワードをハッシュ化
        let hash = hash_password(password).expect("Failed to hash password");

        // Then: bcryptハッシュ形式（$2b$で始まる）
        assert!(hash.starts_with("$2b$"));
        assert!(hash.len() > 50); // bcryptハッシュは通常60文字
    }

    #[test]
    fn test_hash_password_produces_different_hashes() {
        // Given: 同じパスワード
        let password = "same_password";

        // When: 2回ハッシュ化
        let hash1 = hash_password(password).expect("Failed to hash password");
        let hash2 = hash_password(password).expect("Failed to hash password");

        // Then: ソルトのため、異なるハッシュが生成される
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_verify_password_with_correct_password() {
        // Given: ハッシュ化されたパスワード
        let password = "correct_password";
        let hash = hash_password(password).expect("Failed to hash password");

        // When: 正しいパスワードで検証
        let is_valid = verify_password(password, &hash).expect("Failed to verify password");

        // Then: 検証成功
        assert!(is_valid);
    }

    #[test]
    fn test_verify_password_with_incorrect_password() {
        // Given: ハッシュ化されたパスワード
        let password = "correct_password";
        let hash = hash_password(password).expect("Failed to hash password");

        // When: 間違ったパスワードで検証
        let is_valid = verify_password("wrong_password", &hash).expect("Failed to verify password");

        // Then: 検証失敗
        assert!(!is_valid);
    }

    #[test]
    fn test_verify_password_with_empty_password() {
        // Given: 空パスワードのハッシュ
        let password = "";
        let hash = hash_password(password).expect("Failed to hash password");

        // When: 空パスワードで検証
        let is_valid = verify_password("", &hash).expect("Failed to verify password");

        // Then: 検証成功（空パスワードも有効）
        assert!(is_valid);
    }
}
