//! Contract Test: エージェント登録 (POST /api/agents/register)
//!
//! このテストはRED状態であることが期待されます（T027-T031で実装後にGREENになる）

use serde_json::json;

#[tokio::test]
async fn test_agent_registration_success() {
    // Arrange: テストサーバー起動（TODO: T027でAxumサーバー実装後に有効化）
    // let app = coordinator::api::create_app().await;
    // let server = axum_test::TestServer::new(app).unwrap();

    let request_body = json!({
        "machine_name": "test-machine",
        "ip_address": "192.168.1.100",
        "ollama_version": "0.1.0",
        "ollama_port": 11434
    });

    // Act: POST /api/agents/register
    // let response = server.post("/api/agents/register")
    //     .json(&request_body)
    //     .await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // Assert: レスポンススキーマ検証
    // let body: serde_json::Value = response.json();
    // assert!(body["agent_id"].is_string());
    // assert!(body["status"].is_string());
    // assert!(["registered", "updated"].contains(&body["status"].as_str().unwrap()));

    // TODO: T027-T031で実装後にアンコメント
    panic!("RED: エージェント登録APIが未実装");
}

#[tokio::test]
async fn test_agent_registration_invalid_request() {
    // Arrange: 不正なリクエスト
    let request_body = json!({
        "machine_name": "test-machine"
        // ip_address, ollama_version, ollama_portが欠けている
    });

    // Act: POST /api/agents/register
    // let response = server.post("/api/agents/register")
    //     .json(&request_body)
    //     .await;

    // Assert: 400 Bad Request
    // assert_eq!(response.status(), 400);

    // TODO: T027-T031で実装後にアンコメント
    panic!("RED: エージェント登録APIが未実装");
}
