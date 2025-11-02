//! Contract Test: Ollama Generate APIプロキシ (POST /api/generate)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! 実装は既に完了しており、api::proxy::testsで十分にカバーされています。

use serde_json::json;

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::proxy::tests"]
async fn test_proxy_generate_success() {
    // Arrange: 有効なGenerateリクエスト
    let _request_body = json!({
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
#[ignore = "SPEC-32e2b31a archived - covered by api::proxy::tests"]
async fn test_proxy_generate_missing_model() {
    // Arrange: modelパラメータが欠けている
    let _request_body = json!({
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
