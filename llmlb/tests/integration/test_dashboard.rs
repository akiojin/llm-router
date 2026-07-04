//! Integration Test: ダッシュボード
//!
//! WebSocket接続 → リアルタイム更新 → ノード状態変化の受信
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。
//! NOTE: AUTH_DISABLED廃止に伴い、JWT認証を使用するよう更新済み。

use axum::Router;
use futures::StreamExt;
use llmlb::common::auth::UserRole;
use llmlb::{
    api, auth::jwt::create_jwt, balancer::LoadManager, registry::endpoints::EndpointRegistry,
    AppState,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

async fn build_test_app() -> (AppState, Router) {
    let temp_dir = std::env::temp_dir().join(format!(
        "dashboard-ws-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

    std::env::set_var("HOME", &temp_dir);
    std::env::set_var("USERPROFILE", &temp_dir);

    let db_pool = crate::support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    llmlb::api::models::clear_registered_models(&db_pool)
        .await
        .expect("clear registered models");
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

    let app = api::create_app(state.clone());
    (state, app)
}

/// WebSocket 接続リクエストを Authorization ヘッダー付きで生成する。
///
/// クエリパラメータ経由のトークン受理は廃止されたため、ダッシュボード WS は
/// `Authorization: Bearer` ヘッダー（または Cookie）で認証する。
fn ws_request_with_token(
    addr: std::net::SocketAddr,
    secret: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let token =
        create_jwt("test-admin", UserRole::Admin, secret, false, 0, None).expect("create test jwt");
    let mut request = format!("ws://{}/ws/dashboard", addr)
        .into_client_request()
        .expect("build ws client request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", token)
            .parse()
            .expect("valid authorization header"),
    );
    request
}

#[tokio::test]
async fn test_dashboard_websocket_connection() {
    // Arrange: Router server startup
    let (state, app) = build_test_app().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Act: WebSocket connection
    let request = ws_request_with_token(addr, &state.jwt_secret);
    let (ws_stream, _) = connect_async(request)
        .await
        .expect("Failed to connect to WebSocket");

    let (mut _write, mut read) = ws_stream.split();

    // Assert: Should receive connection confirmation
    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for message")
        .expect("No message received")
        .expect("Message error");

    if let Message::Text(text) = msg {
        let json: serde_json::Value = serde_json::from_str(&text).expect("Invalid JSON");
        assert_eq!(json["type"], "connected");
    } else {
        panic!("Expected text message, got {:?}", msg);
    }
}

#[tokio::test]
async fn test_dashboard_receives_node_registration_event() {
    // Arrange: Router server startup
    let (state, app) = build_test_app().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect WebSocket
    let request = ws_request_with_token(addr, &state.jwt_secret);
    let (ws_stream, _) = connect_async(request)
        .await
        .expect("Failed to connect to WebSocket");

    let (_write, mut read) = ws_stream.split();

    // Skip the initial "connected" message
    let _ = read.next().await;

    // Act: Register a node by publishing event directly (since we can't go through HTTP in this test)
    let endpoint_id = uuid::Uuid::new_v4();
    state
        .event_bus
        .publish(llmlb::events::DashboardEvent::EndpointRegistered {
            endpoint_id,
            endpoint_name: "test-node".to_string(),
            ip_address: "127.0.0.1".to_string(),
            status: llmlb::types::endpoint::EndpointStatus::Online,
        });

    // Assert: WebSocket client should receive node registration event
    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for message")
        .expect("No message received")
        .expect("Message error");

    if let Message::Text(text) = msg {
        let json: serde_json::Value = serde_json::from_str(&text).expect("Invalid JSON");
        assert_eq!(json["type"], "NodeRegistered");
        assert_eq!(json["data"]["runtime_id"], endpoint_id.to_string());
        assert_eq!(json["data"]["machine_name"], "test-node");
    } else {
        panic!("Expected text message, got {:?}", msg);
    }
}

#[tokio::test]
async fn test_dashboard_receives_node_status_change() {
    // Arrange: Router server startup
    let (state, app) = build_test_app().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect WebSocket
    let request = ws_request_with_token(addr, &state.jwt_secret);
    let (ws_stream, _) = connect_async(request)
        .await
        .expect("Failed to connect to WebSocket");

    let (_write, mut read) = ws_stream.split();

    // Skip the initial "connected" message
    let _ = read.next().await;

    // Act: Publish a node status change event
    let endpoint_id = uuid::Uuid::new_v4();
    state
        .event_bus
        .publish(llmlb::events::DashboardEvent::EndpointStatusChanged {
            endpoint_id,
            old_status: llmlb::types::endpoint::EndpointStatus::Online,
            new_status: llmlb::types::endpoint::EndpointStatus::Offline,
        });

    // Assert: WebSocket client should receive status change event
    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(5), read.next())
        .await
        .expect("Timeout waiting for message")
        .expect("No message received")
        .expect("Message error");

    if let Message::Text(text) = msg {
        let json: serde_json::Value = serde_json::from_str(&text).expect("Invalid JSON");
        assert_eq!(json["type"], "EndpointStatusChanged");
        assert_eq!(json["data"]["runtime_id"], endpoint_id.to_string());
        assert_eq!(json["data"]["new_status"], "offline");
    } else {
        panic!("Expected text message, got {:?}", msg);
    }
}
