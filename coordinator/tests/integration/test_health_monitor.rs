//! Integration Test: ヘルスモニター
//!
//! 定期ヘルスチェック → タイムアウト検知 → 振り分け対象から除外
//! このテストはRED状態であることが期待されます（T033-T049で実装後にGREENになる）

#[tokio::test]
async fn test_health_monitor_detects_timeout() {
    // Arrange: Coordinatorサーバー起動（ヘルスモニター有効）、エージェント登録
    // let coordinator = start_test_coordinator_with_health_monitor().await;
    // let agent_id = register_test_agent(&coordinator).await;

    // Act: 60秒以上ヘルスチェックを送信しない
    // tokio::time::sleep(Duration::from_secs(61)).await;

    // Assert: エージェントが自動的にオフラインになった
    // let agents = coordinator.list_agents().await;
    // assert_eq!(agents[0].status, "offline");

    // TODO: T033-T049で実装後にアンコメント
    panic!("RED: ヘルスモニターが未実装");
}

#[tokio::test]
async fn test_offline_agent_excluded_from_balancing() {
    // Arrange: Coordinatorサーバー起動、2台のエージェント登録（1台はオフライン）
    // let coordinator = start_test_coordinator().await;
    // let agent1 = register_test_agent(&coordinator).await; // オンライン
    // let agent2 = register_test_agent(&coordinator).await; // オフライン
    // simulate_agent_offline(&coordinator, agent2).await;

    // Act: 5個のリクエストを送信
    // for _ in 0..5 {
    //     let request = json!({
    //         "model": "llama2",
    //         "messages": [{"role": "user", "content": "Hello"}]
    //     });
    //     coordinator.post("/api/chat", request).await;
    // }

    // Assert: オンラインのagent1のみが処理した
    // let metrics = coordinator.get_agent_metrics().await;
    // assert_eq!(metrics[&agent1].total_requests, 5);
    // assert_eq!(metrics[&agent2].total_requests, 0);

    // TODO: T033-T049で実装後にアンコメント
    panic!("RED: オフラインエージェント除外が未実装");
}
