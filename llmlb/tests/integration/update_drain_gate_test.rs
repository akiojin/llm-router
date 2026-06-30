//! Integration Test: self-update drain gate wiring
//!
//! When an update is being applied, new inference requests must be rejected with 503.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use std::sync::Arc;
use tower::ServiceExt;

use crate::support;

#[tokio::test]
async fn rejecting_gate_returns_503_with_retry_after_for_v1_inference() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("LLMLB_DATA_DIR", temp.path());

    let db_pool = support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = Arc::new(llmlb::db::request_history::RequestHistoryStorage::new(
        db_pool.clone(),
    ));
    let jwt_secret = support::lb::test_jwt_secret();

    let http_client = reqwest::Client::new();
    let inference_gate = llmlb::inference_gate::InferenceGate::default();
    let shutdown = llmlb::shutdown::ShutdownController::default();
    let update_manager = llmlb::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to create update manager");

    let gate_handle = inference_gate.clone();
    let app = api::create_app(AppState {
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
    });

    gate_handle.start_rejecting();

    let res = app
        .oneshot(support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        res.headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok()),
        Some("30")
    );
    assert_eq!(gate_handle.in_flight(), 0);
}
