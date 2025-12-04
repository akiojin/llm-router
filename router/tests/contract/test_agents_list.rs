//! Contract Test: ノード一覧取得 (GET /api/nodes)
//!
//! ⚠️ このテストはSPEC-32e2b31a（アーカイブ済み）の一部です。
//! 実装は既に完了しており、api::agent::testsで十分にカバーされています。

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::agent::tests"]
async fn test_agents_list_empty() {
    // Arrange: ノードが登録されていない状態

    // Act: GET /api/nodes
    // let response = server.get("/api/nodes").await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // Assert: 空の配列
    // let body: Vec<serde_json::Value> = response.json();
    // assert_eq!(body.len(), 0);

    // TODO: T040-T041で実装後にアンコメント
    panic!("RED: ノード一覧APIが未実装");
}

#[tokio::test]
#[ignore = "SPEC-32e2b31a archived - covered by api::agent::tests"]
async fn test_agents_list_with_agents() {
    // Arrange: 2台のノードを登録
    // (TODO: T027-T031でノード登録が実装されてから有効化)

    // Act: GET /api/nodes
    // let response = server.get("/api/nodes").await;

    // Assert: 200 OK
    // assert_eq!(response.status(), 200);

    // Assert: 2つのノードが返される
    // let body: Vec<serde_json::Value> = response.json();
    // assert_eq!(body.len(), 2);

    // Assert: スキーマ検証
    // for agent in body {
    //     assert!(agent["id"].is_string());
    //     assert!(agent["machine_name"].is_string());
    //     assert!(agent["ip_address"].is_string());
    //     assert!(agent["runtime_version"].is_string());
    //     assert!(agent["status"].is_string());
    //     assert!(agent["registered_at"].is_string());
    //     assert!(agent["last_seen"].is_string());
    // }

    // TODO: T040-T041で実装後にアンコメント
    panic!("RED: ノード一覧APIが未実装");
}
