//! 画像API統合テスト
//!
//! NOTE: SPEC-e8e9326e（NodeRegistry廃止）に伴い、ノードルーティングテストは削除。
//! EndpointRegistryベースのテストは load balancer/tests/contract/ で実装済み。
//!
//! 削除されたテスト:
//! - test_image_gen_node_routing_selects_stable_diffusion_runtime
//! - test_multi_runtime_node_handles_llm_and_image
//! - test_no_image_capable_node_returns_503

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::json;
use serial_test::serial;
use std::sync::Arc;
use tower::ServiceExt;

async fn build_app() -> Router {
    let db_pool = crate::support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = "test-secret".to_string();
    let http_client = reqwest::Client::new();
    let inference_gate = llmlb::inference_gate::InferenceGate::default();
    let shutdown = llmlb::shutdown::ShutdownController::default();
    let update_manager = llmlb::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to create update manager");
    let state = AppState {
        load_manager,
        request_history,
        db_pool: db_pool.clone(),
        jwt_secret,
        http_client,
        event_bus: llmlb::events::create_shared_event_bus(),
        endpoint_registry,
        inference_gate,
        shutdown,
        update_manager,
        audit_log_writer: llmlb::audit::writer::AuditLogWriter::new(
            llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone()),
            llmlb::audit::writer::AuditLogWriterConfig::default(),
        ),
        audit_log_storage: std::sync::Arc::new(llmlb::db::audit_log::AuditLogStorage::new(db_pool)),
        audit_archive_pool: None,
    };

    api::create_app(state)
}

/// IMG004: 画像生成APIルート存在テスト
///
/// /v1/images/generations, /v1/images/edits, /v1/images/variationsルートが存在する
#[tokio::test]
async fn test_image_api_routes_exist() {
    let app = build_app().await;
    // /v1/images/generations (POST)
    let gen_response = app
        .clone()
        .oneshot(crate::support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/images/generations")
                .header("content-type", "application/json")
                .header("x-api-key", "sk_debug")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "model": "stable-diffusion-xl",
                        "prompt": "test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    // 404でないことを確認（503 Service Unavailableは許容）
    assert_ne!(
        gen_response.status(),
        StatusCode::NOT_FOUND,
        "/v1/images/generations route should exist"
    );

    // /v1/images/edits (POST) - multipartなので空ボディでもルートは存在確認
    let edits_response = app
        .clone()
        .oneshot(crate::support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/images/edits")
                .header("x-api-key", "sk_debug")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_ne!(
        edits_response.status(),
        StatusCode::NOT_FOUND,
        "/v1/images/edits route should exist"
    );

    // /v1/images/variations (POST) - multipartなので空ボディでもルートは存在確認
    let variations_response = app
        .clone()
        .oneshot(crate::support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/images/variations")
                .header("x-api-key", "sk_debug")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_ne!(
        variations_response.status(),
        StatusCode::NOT_FOUND,
        "/v1/images/variations route should exist"
    );
}

/// IMG005: 認証なし画像生成リクエストテスト
///
/// 認証ヘッダーなしで401を返す
#[tokio::test]
#[serial]
async fn test_image_generation_without_auth_returns_401() {
    let app = build_app().await;

    let image_request = json!({
        "model": "stable-diffusion-xl",
        "prompt": "A white cat"
    });

    let response = app
        .clone()
        .oneshot(crate::support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/images/generations")
                .header("content-type", "application/json")
                // No Authorization header
                .body(Body::from(serde_json::to_vec(&image_request).unwrap()))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Should return 401 when no auth header is provided"
    );
}
