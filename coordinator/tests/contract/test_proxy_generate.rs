//! Contract Test: Ollama Generate APIプロキシ (POST /api/generate)
//!
//! このテストはRED状態であることが期待されます（T036-T039で実装後にGREENになる）

use serde_json::json;

#[tokio::test]
async fn test_proxy_generate_success() {
    // Arrange: 有効なGenerateリクエスト
    let request_body = json!({
        "model": "llama2",
        "prompt": "Tell me a joke",
        "stream": false
    });

    // Act: POST /api/generate
    // let response = server.post("/api/generate")
    //     .json(&request_body)
    //     .await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: Ollama Generate APIプロキシが未実装");
}

#[tokio::test]
async fn test_proxy_generate_missing_model() {
    // Arrange: modelパラメータが欠けている
    let request_body = json!({
        "prompt": "Tell me a joke"
    });

    // Act: POST /api/generate
    // let response = server.post("/api/generate")
    //     .json(&request_body)
    //     .await;

    // Assert: 400 Bad Request
    // assert_eq!(response.status(), 400);

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: Ollama Generate APIプロキシが未実装");
}
