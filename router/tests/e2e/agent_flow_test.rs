//! エージェントフローE2Eテスト
//!
//! T093: 完全なエージェントフロー（登録 → トークン使用 → ヘルスチェック）

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use ollama_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
use or_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use serde_json::json;
use std::net::IpAddr;
use tower::ServiceExt;
use uuid::Uuid;

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(or_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = DownloadTaskManager::new();
    let db_pool = support::router::create_test_db_pool().await;
    let jwt_secret = support::router::test_jwt_secret();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool: db_pool.clone(),
        jwt_secret,
    };

    (api::create_router(state), db_pool)
}

#[tokio::test]
async fn test_complete_agent_flow() {
    // ヘルスチェックをスキップ（E2Eテストでは実際のエージェントAPIがない）
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // Step 1: エージェント登録
    let register_request = RegisterRequest {
        machine_name: "test-agent".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100)),
        ollama_version: "0.1.0".to_string(),
        ollama_port: 11434,
        gpu_available: true,
        gpu_devices: vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: Some(8192),
        }],
        gpu_count: Some(1),
        gpu_model: Some("Test GPU".to_string()),
    };

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = register_response.status();
    let register_body = axum::body::to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();

    if status != StatusCode::CREATED {
        let error_text = String::from_utf8_lossy(&register_body);
        eprintln!("Registration failed with status: {:?}", status);
        eprintln!("Error body: {}", error_text);
    }
    assert_eq!(status, StatusCode::CREATED);

    let register_data: serde_json::Value = serde_json::from_slice(&register_body).unwrap();

    let agent_id = Uuid::parse_str(register_data["node_id"].as_str().unwrap()).unwrap();
    let agent_token = register_data["agent_token"].as_str().unwrap();

    assert!(!agent_token.is_empty(), "Agent token should be returned");

    // Step 2: トークンを使ってヘルスチェックを送信
    let heartbeat_request = json!({
        "node_id": agent_id.to_string(),
        "cpu_usage": 50.0,
        "memory_usage": 60.0,
        "gpu_usage": 40.0,
        "gpu_memory_usage": 50.0,
        "gpu_memory_total_mb": 8192,
        "gpu_memory_used_mb": 4096,
        "gpu_temperature": 65.0,
        "active_requests": 0,
        "loaded_models": []
    });

    let heartbeat_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("content-type", "application/json")
                .header("x-agent-token", agent_token)
                .body(Body::from(serde_json::to_vec(&heartbeat_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        heartbeat_response.status(),
        StatusCode::OK,
        "Heartbeat with valid token should succeed"
    );

    // Step 3: トークンなしでヘルスチェックを送信 → 失敗
    let unauthorized_heartbeat_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&heartbeat_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        unauthorized_heartbeat_response.status(),
        StatusCode::UNAUTHORIZED,
        "Heartbeat without token should fail"
    );

    // Step 4: 無効なトークンでヘルスチェックを送信 → 失敗
    let invalid_token_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("content-type", "application/json")
                .header("x-agent-token", "invalid-token-12345")
                .body(Body::from(serde_json::to_vec(&heartbeat_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        invalid_token_response.status(),
        StatusCode::UNAUTHORIZED,
        "Heartbeat with invalid token should fail"
    );
}

#[tokio::test]
async fn test_agent_token_persistence() {
    // ヘルスチェックをスキップ（E2Eテストでは実際のエージェントAPIがない）
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // エージェント登録
    let register_request = RegisterRequest {
        machine_name: "test-agent-2".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 101)),
        ollama_version: "0.1.0".to_string(),
        ollama_port: 11434,
        gpu_available: true,
        gpu_devices: vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: Some(8192),
        }],
        gpu_count: Some(1),
        gpu_model: Some("Test GPU".to_string()),
    };

    let first_register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(first_register_response.status(), StatusCode::CREATED);

    let first_body = axum::body::to_bytes(first_register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let first_data: serde_json::Value = serde_json::from_slice(&first_body).unwrap();

    let agent_id = first_data["node_id"].as_str().unwrap();
    let first_token = first_data["agent_token"].as_str().unwrap();

    assert!(
        !first_token.is_empty(),
        "First registration should return token"
    );

    // 同じエージェントを再度登録（更新）
    let second_register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .header("x-agent-token", first_token)
                .body(Body::from(serde_json::to_vec(&register_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        second_register_response.status(),
        StatusCode::OK,
        "Re-registration should return 200 OK (update)"
    );

    let second_body = axum::body::to_bytes(second_register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let second_data: serde_json::Value = serde_json::from_slice(&second_body).unwrap();

    let second_agent_id = second_data["node_id"].as_str().unwrap();

    // 同じエージェントIDが返される
    assert_eq!(
        agent_id, second_agent_id,
        "Re-registration should return same agent ID"
    );

    // 2回目の登録ではトークンは返されない（既存のトークンを使用）
    assert!(
        second_data["agent_token"].is_null(),
        "Re-registration should not return new token"
    );
}
