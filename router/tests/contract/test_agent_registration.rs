//! Contract Test: ノード登録 (POST /api/nodes/register)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! 実装は既に完了しており、api::agent::testsで十分にカバーされています。

use serde_json::json;

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::agent::tests"]
async fn test_agent_registration_success() {
    // Arrange: テストサーバー起動（TODO: T027でAxumサーバー実装後に有効化）
    // let app = coordinator::api::create_app().await;
    // let server = axum_test::TestServer::new(app).unwrap();

    let _request_body = json!({
        "machine_name": "test-machine",
        "ip_address": "192.168.1.100",
        "runtime_version": "0.1.0",
        "runtime_port": 11434
    });

    // Act: POST /api/nodes/register
    // let response = server.post("/api/nodes/register")
    //     .json(&request_body)
    //     .await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // Assert: レスポンススキーマ検証
    // let body: serde_json::Value = response.json();
    // assert!(body["node_id"].is_string());
    // assert!(body["status"].is_string());
    // assert!(["registered", "updated"].contains(&body["status"].as_str().unwrap()));

    // TODO: T027-T031で実装後にアンコメント
    panic!("RED: ノード登録APIが未実装");
}

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::agent::tests"]
async fn test_agent_registration_invalid_request() {
    // Arrange: 不正なリクエスト
    let _request_body = json!({
        "machine_name": "test-machine"
        // ip_address, runtime_version, runtime_portが欠けている
    });

    // Act: POST /api/nodes/register
    // let response = server.post("/api/nodes/register")
    //     .json(&request_body)
    //     .await;

    // Assert: 400 Bad Request
    // assert_eq!(response.status(), 400);

    // TODO: T027-T031で実装後にアンコメント
    panic!("RED: ノード登録APIが未実装");
}
