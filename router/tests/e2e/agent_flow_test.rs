//! エージェントフローE2Eテスト
//!
//! T093: 完全なエージェントフロー（登録 → トークン使用 → ヘルスチェック）

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use chrono::Utc;
use llm_router::{
    api, balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager, AppState,
};
use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
use serde_json::json;
use std::net::IpAddr;
use tower::ServiceExt;
use uuid::Uuid;

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
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
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // Step 1: エージェント登録
    let register_request = RegisterRequest {
        machine_name: "test-agent".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100)),
        runtime_version: "0.1.0".to_string(),
        runtime_port: 11434,
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
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // エージェント登録
    let register_request = RegisterRequest {
        machine_name: "test-agent-2".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 101)),
        runtime_version: "0.1.0".to_string(),
        runtime_port: 11434,
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

    // 2回目の登録でも新しいトークンが返される（プロトコル変更により、更新時もトークンを再生成）
    assert!(
        second_data["agent_token"].is_string(),
        "Re-registration should return a new token"
    );
}

#[tokio::test]
async fn test_list_nodes() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // ノードを登録
    let register_request = RegisterRequest {
        machine_name: "list-test-node".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 200)),
        runtime_version: "0.1.0".to_string(),
        runtime_port: 11434,
        gpu_available: true,
        gpu_devices: vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: Some(8192),
        }],
        gpu_count: Some(1),
        gpu_model: Some("Test GPU".to_string()),
    };

    let _register_response = app
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

    // GET /api/nodes でノード一覧を取得
    let list_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        list_response.status(),
        StatusCode::OK,
        "GET /api/nodes should return OK"
    );

    let body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let nodes: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(nodes.is_array(), "Response should be an array");
    let nodes_array = nodes.as_array().unwrap();
    assert!(
        !nodes_array.is_empty(),
        "Should have at least one registered node"
    );

    // ノードの構造を検証
    let node = &nodes_array[0];
    assert!(node.get("id").is_some(), "Node must have 'id' field");
    assert!(
        node.get("machine_name").is_some(),
        "Node must have 'machine_name' field"
    );
}

#[tokio::test]
async fn test_node_metrics_update() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // ノードを登録
    let register_request = RegisterRequest {
        machine_name: "metrics-test-node".to_string(),
        ip_address: IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 201)),
        runtime_version: "0.1.0".to_string(),
        runtime_port: 11434,
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

    let body = axum::body::to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let register_data: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let node_id = register_data["node_id"].as_str().unwrap();
    let agent_token = register_data["agent_token"].as_str().unwrap();

    // POST /api/nodes/:node_id/metrics でメトリクスを更新
    let metrics_request = json!({
        "node_id": node_id,
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "active_requests": 3,
        "avg_response_time_ms": 250.5,
        "timestamp": Utc::now().to_rfc3339()
    });

    let metrics_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/nodes/{}/metrics", node_id))
                .header("content-type", "application/json")
                .header("x-agent-token", agent_token)
                .body(Body::from(serde_json::to_vec(&metrics_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        metrics_response.status() == StatusCode::OK
            || metrics_response.status() == StatusCode::NO_CONTENT,
        "POST /api/nodes/:id/metrics should return OK or NO_CONTENT"
    );
}

#[tokio::test]
async fn test_list_node_metrics() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/nodes/metrics でメトリクス一覧を取得
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/nodes/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/nodes/metrics should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // メトリクスはオブジェクトまたは配列
    assert!(
        metrics.is_object() || metrics.is_array(),
        "Response should be an object or array"
    );
}

#[tokio::test]
async fn test_metrics_summary() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let (app, _db_pool) = build_app().await;

    // GET /api/metrics/summary
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/metrics/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /api/metrics/summary should return OK"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let summary: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(summary.is_object(), "Response should be an object");
}
