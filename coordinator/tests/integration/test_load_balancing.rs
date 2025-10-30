//! Integration Test: ロードバランシング
//!
//! 複数リクエスト → 複数エージェントに均等分散
//! このテストはRED状態であることが期待されます（T038-T047で実装後にGREENになる）

use serde_json::json;

#[tokio::test]
async fn test_round_robin_load_balancing() {
    // Arrange: Coordinatorサーバー起動、3台のエージェント登録
    // let coordinator = start_test_coordinator().await;
    // let agent1 = register_test_agent(&coordinator).await;
    // let agent2 = register_test_agent(&coordinator).await;
    // let agent3 = register_test_agent(&coordinator).await;

    // Act: 9つのリクエストを送信
    // for _ in 0..9 {
    //     let request = json!({
    //         "model": "llama2",
    //         "messages": [{"role": "user", "content": "Hello"}]
    //     });
    //     coordinator.post("/api/chat", request).await;
    // }

    // Assert: 各エージェントが3リクエストずつ処理した
    // let metrics = coordinator.get_agent_metrics().await;
    // assert_eq!(metrics[&agent1].total_requests, 3);
    // assert_eq!(metrics[&agent2].total_requests, 3);
    // assert_eq!(metrics[&agent3].total_requests, 3);

    // TODO: T038-T047で実装後にアンコメント
    panic!("RED: ロードバランシングが未実装");
}

#[tokio::test]
async fn test_load_based_balancing() {
    // Arrange: Coordinatorサーバー起動、2台のエージェント登録
    // let coordinator = start_test_coordinator().await;
    // let agent1 = register_test_agent(&coordinator).await; // CPU: 90%
    // let agent2 = register_test_agent(&coordinator).await; // CPU: 10%
    // simulate_high_load(&coordinator, agent1, 90.0).await;

    // Act: 10個のリクエストを送信
    // for _ in 0..10 {
    //     let request = json!({
    //         "model": "llama2",
    //         "messages": [{"role": "user", "content": "Hello"}]
    //     });
    //     coordinator.post("/api/chat", request).await;
    // }

    // Assert: 低負荷のagent2が優先的に処理した
    // let metrics = coordinator.get_agent_metrics().await;
    // assert!(metrics[&agent2].total_requests > metrics[&agent1].total_requests);

    // TODO: T038-T047で実装後にアンコメント
    panic!("RED: 負荷ベースロードバランシングが未実装");
}
