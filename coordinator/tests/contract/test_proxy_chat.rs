//! Contract Test: Ollama Chat APIプロキシ (POST /api/chat)
//!
//! このテストはRED状態であることが期待されます（T036-T039で実装後にGREENになる）

use serde_json::json;

#[tokio::test]
async fn test_proxy_chat_success() {
    // Arrange: 有効なチャットリクエスト
    let request_body = json!({
        "model": "llama2",
        "messages": [
            {"role": "user", "content": "Hello, world!"}
        ],
        "stream": false
    });

    // Act: POST /api/chat
    // let response = server.post("/api/chat")
    //     .json(&request_body)
    //     .await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // Assert: レスポンススキーマ検証
    // let body: serde_json::Value = response.json();
    // assert!(body["message"].is_object());
    // assert!(body["done"].is_boolean());

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: Ollama Chat APIプロキシが未実装");
}

#[tokio::test]
async fn test_proxy_chat_no_agents_available() {
    // Arrange: エージェントが登録されていない状態
    let request_body = json!({
        "model": "llama2",
        "messages": [
            {"role": "user", "content": "Hello, world!"}
        ]
    });

    // Act: POST /api/chat
    // let response = server.post("/api/chat")
    //     .json(&request_body)
    //     .await;

    // Assert: 503 Service Unavailable
    // assert_eq!(response.status(), 503);

    // Assert: エラーメッセージ
    // let body: serde_json::Value = response.json();
    // assert!(body["error"].as_str().unwrap().contains("利用可能なエージェントがありません"));

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: Ollama Chat APIプロキシが未実装");
}
