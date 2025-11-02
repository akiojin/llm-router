//! Contract Test: Ollama Chat APIプロキシ (POST /api/chat)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! 実装は既に完了しており、api::proxy::testsで十分にカバーされています。

use serde_json::json;

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::proxy::tests"]
async fn test_proxy_chat_success() {
    // Arrange: 有効なチャットリクエスト
    let _request_body = json!({
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
#[ignore = "SPEC-32e2b31a archived - covered by api::proxy::tests"]
async fn test_proxy_chat_no_agents_available() {
    // Arrange: エージェントが登録されていない状態
    let _request_body = json!({
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
