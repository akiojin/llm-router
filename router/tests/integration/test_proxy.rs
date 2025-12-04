//! Integration Test: LLM runtimeプロキシ
//!
//! リクエスト振り分け → LLM runtime転送 → レスポンス返却
//! このテストはRED状態であることが期待されます（T036-T039で実装後にGREENになる）

use serde_json::json;

#[tokio::test]
async fn test_proxy_request_to_single_agent() {
    // Arrange: Coordinatorサーバー起動、1台のノード登録、モックLLM runtime起動
    // let coordinator = start_test_coordinator().await;
    // let mock_runtime = start_mock_runtime().await;
    // register_test_agent(&coordinator, mock_runtime.url()).await;

    // Act: チャットリクエスト送信
    // let request = json!({
    //     "model": "llama2",
    //     "messages": [{"role": "user", "content": "Hello"}]
    // });
    // let response = coordinator.post("/api/chat", request).await;

    // Assert: 正常にレスポンスが返された
    // assert_eq!(response.status(), 200);
    // let body: serde_json::Value = response.json();
    // assert!(body["message"].is_object());

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: LLM runtimeプロキシが未実装");
}

#[tokio::test]
async fn test_proxy_no_agents_returns_503() {
    // Arrange: Coordinatorサーバー起動（ノード未登録）
    // let coordinator = start_test_coordinator().await;

    // Act: チャットリクエスト送信
    // let request = json!({
    //     "model": "llama2",
    //     "messages": [{"role": "user", "content": "Hello"}]
    // });
    // let response = coordinator.post("/api/chat", request).await;

    // Assert: 503 Service Unavailable
    // assert_eq!(response.status(), 503);

    // TODO: T036-T039で実装後にアンコメント
    panic!("RED: LLM runtimeプロキシが未実装");
}
