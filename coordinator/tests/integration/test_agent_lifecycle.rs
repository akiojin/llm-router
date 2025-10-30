//! Integration Test: エージェントライフサイクル
//!
//! エージェント登録 → ヘルスチェック → オフライン検知
//! このテストはRED状態であることが期待されます（T027-T049で実装後にGREENになる）

#[tokio::test]
async fn test_agent_registers_and_sends_heartbeat() {
    // Arrange: Coordinatorサーバー起動
    // let coordinator = start_test_coordinator().await;

    // Act: エージェント登録
    // let agent_id = register_test_agent(&coordinator).await;

    // Assert: エージェントが登録された
    // let agents = coordinator.list_agents().await;
    // assert_eq!(agents.len(), 1);
    // assert_eq!(agents[0].id, agent_id);
    // assert_eq!(agents[0].status, "online");

    // Act: ヘルスチェック送信
    // send_heartbeat(&coordinator, agent_id).await;

    // Assert: last_seenが更新された
    // let agents = coordinator.list_agents().await;
    // assert!(agents[0].last_seen > initial_last_seen);

    // TODO: T027-T049で実装後にアンコメント
    panic!("RED: エージェントライフサイクルが未実装");
}

#[tokio::test]
async fn test_agent_timeout_detection() {
    // Arrange: Coordinatorサーバー起動、エージェント登録
    // let coordinator = start_test_coordinator().await;
    // let agent_id = register_test_agent(&coordinator).await;

    // Act: 60秒以上ヘルスチェックを送信しない（タイムアウトシミュレーション）
    // tokio::time::sleep(Duration::from_secs(61)).await;

    // Assert: エージェントがオフラインになった
    // let agents = coordinator.list_agents().await;
    // assert_eq!(agents[0].status, "offline");

    // TODO: T027-T049で実装後にアンコメント
    panic!("RED: エージェントタイムアウト検知が未実装");
}

#[tokio::test]
async fn test_agent_auto_reconnect() {
    // Arrange: Coordinatorサーバー起動、エージェント登録後にオフライン
    // let coordinator = start_test_coordinator().await;
    // let agent_id = register_test_agent(&coordinator).await;
    // simulate_agent_offline(&coordinator, agent_id).await;

    // Act: エージェント再登録
    // let new_agent_id = register_test_agent(&coordinator).await;

    // Assert: エージェントがオンラインに戻った
    // assert_eq!(agent_id, new_agent_id); // 同じIDで再登録
    // let agents = coordinator.list_agents().await;
    // assert_eq!(agents[0].status, "online");

    // TODO: T027-T049で実装後にアンコメント
    panic!("RED: エージェント自動再接続が未実装");
}
