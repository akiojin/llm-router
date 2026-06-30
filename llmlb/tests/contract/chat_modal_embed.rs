//! Contract test: dashboard (React SPA) serves correctly
//!
//! The dashboard is now a React single-page application.
//! This test verifies the React app shell is served correctly.
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{body::to_bytes, http::Request, Router};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
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

#[tokio::test]
async fn dashboard_serves_react_app() {
    let app = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/dashboard")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 512 * 1024).await.unwrap();
    let html = String::from_utf8(bytes.to_vec()).expect("dashboard html should be utf-8");

    // React app mount point
    assert!(
        html.contains("id=\"root\""),
        "React mount point (id=root) not found"
    );

    // Should have script tags for the React bundle
    assert!(
        html.contains("<script") && html.contains("</script>"),
        "Script tags not found"
    );

    // Should have the dashboard title
    assert!(
        html.contains("<title>") && html.contains("Dashboard"),
        "Dashboard title not found"
    );
}

// NOTE: dashboard_links_to_playground テストは廃止
// Playground機能はダッシュボード内のエンドポイント別Playgroundに移行 (#playground/:endpointId)
// 旧 /playground ルートは削除されました
