//! TPS優先ルーティング選択のUnit Test
//!
//! SPEC #494 Phase 3:
//! - 高TPSエンドポイントの優先選択
//! - モデル別TPS選択
//! - TPS未計測エンドポイントの最低優先
//! - 同一TPS時のラウンドロビン
//! - offline/error/initializing エンドポイントの除外
//! - 非TPS対象リクエストでは他API種別のTPSを流用しない

use std::sync::Arc;

use llmlb::{
    balancer::LoadManager,
    common::protocol::TpsApiKind,
    db::migrations::run_migrations,
    registry::endpoints::EndpointRegistry,
    types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI},
};
use sqlx::SqlitePool;

async fn create_test_load_manager() -> LoadManager {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to connect test database");
    run_migrations(&pool)
        .await
        .expect("Failed to run test migrations");

    let registry = Arc::new(
        EndpointRegistry::new(pool)
            .await
            .expect("Failed to create endpoint registry"),
    );
    LoadManager::new(registry)
}

async fn add_online_endpoint(load_manager: &LoadManager, name: &str, models: &[&str]) -> Endpoint {
    let mut endpoint = Endpoint::new(
        name.to_string(),
        format!("http://{}.local:8080", name.to_lowercase()),
        EndpointType::Xllm,
    );
    endpoint.status = EndpointStatus::Online;

    load_manager
        .endpoint_registry()
        .add(endpoint.clone())
        .await
        .expect("Failed to add endpoint");

    for model_id in models {
        load_manager
            .endpoint_registry()
            .add_model(&EndpointModel {
                endpoint_id: endpoint.id,
                model_id: (*model_id).to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions, SupportedAPI::Responses],
                canonical_name: None,
            })
            .await
            .expect("Failed to add endpoint model");
    }

    endpoint
}

#[tokio::test]
async fn tps_routing_prefers_highest_aggregate_tps_without_model() {
    let load_manager = create_test_load_manager().await;
    let fast = add_online_endpoint(&load_manager, "Fast", &["model-a"]).await;
    let slow = add_online_endpoint(&load_manager, "Slow", &["model-b"]).await;

    load_manager
        .update_tps(
            fast.id,
            "model-a".to_string(),
            TpsApiKind::ChatCompletions,
            200,
            1_000,
        )
        .await;
    load_manager
        .update_tps(
            slow.id,
            "model-b".to_string(),
            TpsApiKind::ChatCompletions,
            50,
            1_000,
        )
        .await;

    let selected = load_manager
        .select_endpoint_by_tps_direct(None)
        .await
        .expect("endpoint should be selected");

    assert_eq!(selected.id, fast.id, "highest aggregate TPS should win");
}

#[tokio::test]
async fn tps_routing_prefers_highest_tps_for_model() {
    let load_manager = create_test_load_manager().await;
    let fast = add_online_endpoint(&load_manager, "Fast", &["shared-model"]).await;
    let slow = add_online_endpoint(&load_manager, "Slow", &["shared-model"]).await;

    load_manager
        .update_tps(
            fast.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            180,
            1_000,
        )
        .await;
    load_manager
        .update_tps(
            slow.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            60,
            1_000,
        )
        .await;

    let selected = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("endpoint should be selected");

    assert_eq!(selected.id, fast.id, "highest model TPS should win");
}

#[tokio::test]
async fn tps_routing_treats_unmeasured_endpoint_as_lowest_priority() {
    let load_manager = create_test_load_manager().await;
    let measured = add_online_endpoint(&load_manager, "Measured", &["shared-model"]).await;
    let unmeasured = add_online_endpoint(&load_manager, "Unmeasured", &["shared-model"]).await;

    load_manager
        .update_tps(
            measured.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            40,
            1_000,
        )
        .await;

    let selected = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("endpoint should be selected");

    assert_eq!(selected.id, measured.id);
    assert_ne!(selected.id, unmeasured.id);
}

#[tokio::test]
async fn tps_routing_all_unmeasured_uses_round_robin() {
    let load_manager = create_test_load_manager().await;
    let _first = add_online_endpoint(&load_manager, "First", &["shared-model"]).await;
    let _second = add_online_endpoint(&load_manager, "Second", &["shared-model"]).await;
    let _third = add_online_endpoint(&load_manager, "Third", &["shared-model"]).await;

    let selected_1 = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("first selection should succeed");
    let selected_2 = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("second selection should succeed");

    assert_ne!(
        selected_1.id, selected_2.id,
        "round-robin should cycle through endpoints when all TPS are equal"
    );
}

#[tokio::test]
async fn tps_routing_uses_round_robin_when_tps_is_tied() {
    let load_manager = create_test_load_manager().await;
    let first = add_online_endpoint(&load_manager, "First", &["shared-model"]).await;
    let second = add_online_endpoint(&load_manager, "Second", &["shared-model"]).await;

    for endpoint in [&first, &second] {
        load_manager
            .update_tps(
                endpoint.id,
                "shared-model".to_string(),
                TpsApiKind::ChatCompletions,
                100,
                1_000,
            )
            .await;
    }

    let selected_1 = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("first selection should succeed");
    let selected_2 = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("second selection should succeed");

    assert_ne!(
        selected_1.id, selected_2.id,
        "same TPS should be tie-broken by round-robin"
    );
}

#[tokio::test]
async fn tps_routing_excludes_offline_error_and_initializing_endpoints() {
    let load_manager = create_test_load_manager().await;
    let ready = add_online_endpoint(&load_manager, "Ready", &["shared-model"]).await;
    let initializing = add_online_endpoint(&load_manager, "Initializing", &["shared-model"]).await;
    let offline = add_online_endpoint(&load_manager, "Offline", &["shared-model"]).await;
    let error = add_online_endpoint(&load_manager, "Error", &["shared-model"]).await;

    for endpoint in [&ready, &initializing, &offline, &error] {
        load_manager
            .update_tps(
                endpoint.id,
                "shared-model".to_string(),
                TpsApiKind::ChatCompletions,
                200,
                1_000,
            )
            .await;
    }

    load_manager
        .upsert_initial_state(initializing.id, true, Some((0, 1)))
        .await;
    load_manager
        .endpoint_registry()
        .update_status(offline.id, EndpointStatus::Offline, None, Some("offline"))
        .await
        .expect("offline transition should succeed");
    load_manager
        .endpoint_registry()
        .update_status(error.id, EndpointStatus::Error, None, Some("error"))
        .await
        .expect("error transition should succeed");

    let selected = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("ready endpoint should remain selectable");

    assert_eq!(selected.id, ready.id);
}

#[tokio::test]
async fn tps_routing_ignores_other_api_kinds_when_request_kind_is_none() {
    let load_manager = create_test_load_manager().await;
    let first = add_online_endpoint(&load_manager, "First", &["shared-model"]).await;
    let second = add_online_endpoint(&load_manager, "Second", &["shared-model"]).await;

    load_manager
        .update_tps(
            first.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            400,
            100,
        )
        .await;

    let first_pick = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", None::<TpsApiKind>)
        .await
        .expect("first selection should succeed");
    let second_pick = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", None::<TpsApiKind>)
        .await
        .expect("second selection should succeed");

    assert_ne!(
        first_pick.id, second_pick.id,
        "non-TPS request kinds must not inherit chat/completions TPS bias"
    );
    assert!([first.id, second.id].contains(&first_pick.id));
    assert!([first.id, second.id].contains(&second_pick.id));
}

#[tokio::test]
async fn tps_routing_no_endpoints_returns_error() {
    let load_manager = create_test_load_manager().await;
    let result = load_manager.select_endpoint_by_tps_direct(None).await;
    assert!(
        result.is_err(),
        "should return error when no endpoints available"
    );
}

#[tokio::test]
async fn tps_routing_single_endpoint_is_selected() {
    let load_manager = create_test_load_manager().await;
    let only = add_online_endpoint(&load_manager, "Only", &["shared-model"]).await;

    load_manager
        .update_tps(
            only.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            500,
            10_000,
        )
        .await;

    let selected = load_manager
        .select_endpoint_by_tps_ready_for_model("shared-model", Some(TpsApiKind::ChatCompletions))
        .await
        .expect("selection should succeed");

    assert_eq!(selected.id, only.id);
}

#[tokio::test]
async fn tps_update_after_offline_does_not_recreate_entry() {
    // オフライン遷移で clear_tps_for_endpoint された後に、in-flight リクエスト完了の
    // 遅延 update_tps が TPS エントリを再生成しないことを検証する。
    let load_manager = create_test_load_manager().await;
    let endpoint = add_online_endpoint(&load_manager, "Target", &["shared-model"]).await;

    // オンライン中の計測は通常どおり反映される
    load_manager
        .update_tps(
            endpoint.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            100,
            1_000,
        )
        .await;
    assert_eq!(load_manager.get_model_tps(endpoint.id).await.len(), 1);

    // オフライン遷移 → TPSクリア（health checker の挙動を再現）
    load_manager
        .endpoint_registry()
        .update_status(endpoint.id, EndpointStatus::Offline, None, Some("offline"))
        .await
        .expect("offline transition should succeed");
    load_manager.clear_tps_for_endpoint(endpoint.id).await;

    // クリア後に届いた遅延計測は破棄され、エントリが再生成されない
    load_manager
        .update_tps(
            endpoint.id,
            "shared-model".to_string(),
            TpsApiKind::ChatCompletions,
            100,
            1_000,
        )
        .await;

    assert!(
        load_manager.get_model_tps(endpoint.id).await.is_empty(),
        "オフライン後の遅延 update_tps は TPS エントリを再生成してはならない"
    );
}
