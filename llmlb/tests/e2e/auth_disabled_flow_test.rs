//! デバッグAPIキー認証のE2Eテスト
//!
//! デバッグビルドでsk_debug APIキーを使用してアクセスできることを確認する
//!
//! NOTE: AUTH_DISABLEDは廃止されました。デバッグビルドではsk_debug APIキーで認証します。

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use std::sync::Arc;
use tower::ServiceExt;

use crate::support;

async fn build_app() -> Router {
    let db_pool = support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
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

/// デバッグビルドではsk_debug APIキーでエンドポイント一覧にアクセスできることを確認
#[cfg(debug_assertions)]
#[tokio::test]
async fn debug_api_key_allows_endpoints_access() {
    let app = build_app().await;

    let endpoints_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/endpoints")
                .header("x-api-key", "sk_debug")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        endpoints_response.status(),
        StatusCode::OK,
        "sk_debug should allow /api/endpoints in debug build"
    );
}

/// デバッグビルドではsk_debug APIキーでダッシュボード静的アセットにアクセスできることを確認
/// （ダッシュボード静的ファイルは認証不要）
#[tokio::test]
async fn dashboard_static_is_accessible_without_auth() {
    let app = build_app().await;

    let dashboard_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/dashboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        dashboard_response.status(),
        StatusCode::OK,
        "/dashboard static asset should be accessible without auth"
    );
}
