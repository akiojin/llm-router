// T029-T031: JWT生成・検証・有効期限チェックのユニットテスト（RED）

#[cfg(test)]
mod jwt_tests {
    use ollama_coordinator_common::auth::UserRole;
    use ollama_coordinator_coordinator::auth::jwt::{create_jwt, verify_jwt};

    const TEST_SECRET: &str = "test_secret_key_for_jwt_testing_12345678";

    #[test]
    fn test_create_jwt_generates_valid_token() {
        // Given: ユーザーIDとロール
        let user_id = "user-123";
        let role = UserRole::Admin;

        // When: JWTトークンを生成
        let token = create_jwt(user_id, role, TEST_SECRET).expect("Failed to create JWT");

        // Then: JWT形式（3つのピリオド区切り部分）
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn test_verify_jwt_with_valid_token() {
        // Given: 生成されたJWTトークン
        let user_id = "user-456";
        let role = UserRole::Viewer;
        let token = create_jwt(user_id, role, TEST_SECRET).expect("Failed to create JWT");

        // When: トークンを検証
        let claims = verify_jwt(&token, TEST_SECRET).expect("Failed to verify JWT");

        // Then: クレームが正しい
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.role, role);
    }

    #[test]
    fn test_verify_jwt_with_invalid_secret() {
        // Given: 異なるシークレットで生成されたトークン
        let user_id = "user-789";
        let role = UserRole::Admin;
        let token = create_jwt(user_id, role, TEST_SECRET).expect("Failed to create JWT");

        // When: 間違ったシークレットで検証
        let result = verify_jwt(&token, "wrong_secret_key");

        // Then: 検証失敗
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_jwt_with_malformed_token() {
        // Given: 不正な形式のトークン
        let malformed_token = "invalid.token.format";

        // When: 検証を試みる
        let result = verify_jwt(malformed_token, TEST_SECRET);

        // Then: 検証失敗
        assert!(result.is_err());
    }

    #[test]
    fn test_jwt_expiration_not_expired() {
        // Given: 有効期限内のトークン（24時間）
        let user_id = "user-exp-1";
        let role = UserRole::Admin;
        let token = create_jwt(user_id, role, TEST_SECRET).expect("Failed to create JWT");

        // When: 即座に検証
        let claims = verify_jwt(&token, TEST_SECRET).expect("Failed to verify JWT");

        // Then: expフィールドが未来の日時
        let now = chrono::Utc::now().timestamp() as usize;
        assert!(claims.exp > now);
    }

    #[test]
    #[ignore] // 実際の有効期限テストは時間がかかるため、通常はスキップ
    fn test_jwt_expiration_expired() {
        // Given: 有効期限が1秒のトークン（テスト用）
        // 注: この実装では有効期限を変更できないため、このテストはスキップ
        // 実装時にcreate_jwt_with_expirationのような関数を追加する必要がある
    }

    #[test]
    fn test_create_jwt_for_different_roles() {
        // Given: 異なるロールのユーザー
        let user_id = "user-roles";

        // When: AdminとViewerのトークンを生成
        let admin_token =
            create_jwt(user_id, UserRole::Admin, TEST_SECRET).expect("Failed to create admin JWT");
        let viewer_token = create_jwt(user_id, UserRole::Viewer, TEST_SECRET)
            .expect("Failed to create viewer JWT");

        // Then: 両方とも有効だが、ロールが異なる
        let admin_claims =
            verify_jwt(&admin_token, TEST_SECRET).expect("Failed to verify admin JWT");
        let viewer_claims =
            verify_jwt(&viewer_token, TEST_SECRET).expect("Failed to verify viewer JWT");

        assert_eq!(admin_claims.role, UserRole::Admin);
        assert_eq!(viewer_claims.role, UserRole::Viewer);
    }
}
