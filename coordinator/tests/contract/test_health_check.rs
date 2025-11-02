//! Contract Test: ヘルスチェック (POST /api/health)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! 実装は既に完了しており、api::health::testsで十分にカバーされています。

use serde_json::json;

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::health::tests"]
async fn test_health_check_success() {
    // Arrange: 有効なヘルスチェックリクエスト
    let _request_body = json!({
        "agent_id": "550e8400-e29b-41d4-a716-446655440000",
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "active_requests": 3
    });

    // Act: POST /api/health
    // let response = server.post("/api/health")
    //     .json(&request_body)
    //     .await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // TODO: T032-T035で実装後にアンコメント
    panic!("RED: ヘルスチェックAPIが未実装");
}

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::health::tests"]
async fn test_health_check_invalid_agent_id() {
    // Arrange: 不正なagent_id形式
    let _request_body = json!({
        "agent_id": "invalid-uuid",
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "active_requests": 3
    });

    // Act: POST /api/health
    // let response = server.post("/api/health")
    //     .json(&request_body)
    //     .await;

    // Assert: 400 Bad Request
    // assert_eq!(response.status(), 400);

    // TODO: T032-T035で実装後にアンコメント
    panic!("RED: ヘルスチェックAPIが未実装");
}
