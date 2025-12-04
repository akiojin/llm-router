//! エージェントトークン統合テスト
//!
//! T024-T026: エージェント登録時のトークン発行、ヘルスチェック成功/拒否

use crate::support;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llm_router_common::auth::UserRole;
use llm_router::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

async fn build_app() -> Router {
    // 認証を有効化（他のテストの影響を受けないように明示的に設定）
    std::env::remove_var("AUTH_DISABLED");

    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        llm_router::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = llm_router::tasks::DownloadTaskManager::new();
    let db_pool = support::router::create_test_db_pool().await;
    let jwt_secret = support::router::test_jwt_secret();

    // テスト用の管理者ユーザーを作成（エージェント管理に必要）
    let password_hash =
        llm_router::auth::password::hash_password("password123").unwrap();
    llm_router::db::users::create(
        &db_pool,
        "admin",
        &password_hash,
        UserRole::Admin,
    )
    .await
    .ok();

    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };

    api::create_router(state)
}

/// T024: エージェント登録時のトークン発行テスト
#[tokio::test]
async fn test_agent_registration_token_issuance() {
    let app = build_app().await;

    // Step 1: POST /api/agents でエージェントを登録
    let register_payload = json!({
        "machine_name": "test-agent",
        "ip_address": "192.168.1.100",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 3090", "count": 1}
        ]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "Agent registration should succeed"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Step 2: レスポンスに agent_token フィールドが含まれる
    assert!(
        agent.get("agent_token").is_some(),
        "Response must include agent_token field"
    );

    // Step 3: agent_token が `agt_` プレフィックスで始まる
    let agent_token = agent["agent_token"].as_str().unwrap();
    assert!(
        agent_token.starts_with("agt_"),
        "Agent token should start with agt_, got: {}",
        agent_token
    );

    // Step 4: agent_token がデータベースにハッシュ化されて保存される
    // （実際のハッシュ検証はユニットテストで行うため、ここではトークンが返されることのみ確認）
    assert!(!agent_token.is_empty(), "Agent token should not be empty");
}

/// T025: トークン付きヘルスチェック成功テスト
#[tokio::test]
async fn test_health_check_with_valid_token() {
    let app = build_app().await;

    // Step 1: エージェントを登録してトークンを取得
    let register_payload = json!({
        "machine_name": "health-check-agent",
        "ip_address": "192.168.1.101",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 3090", "count": 1}
        ]
    });

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_token = agent["agent_token"].as_str().unwrap();
    let agent_id = agent["agent_id"].as_str().unwrap();

    // Step 2: X-Agent-Tokenヘッダーでトークンを含めて POST /api/health にアクセス
    let health_payload = json!({
        "agent_id": agent_id,
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "gpu_usage": null,
        "gpu_memory_usage": null,
        "gpu_memory_total_mb": null,
        "gpu_memory_used_mb": null,
        "gpu_temperature": null,
        "gpu_model_name": null,
        "gpu_compute_capability": null,
        "gpu_capability_score": null,
        "active_requests": 3,
        "average_response_time_ms": 110.0,
        "loaded_models": ["gpt-oss:20b"]
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("x-agent-token", agent_token)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&health_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 3: 200 OK を受信
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Health check with valid agent token should succeed"
    );

    // Step 4: ヘルスチェック情報が記録される
    // （実際の記録検証はユニットテストで行うため、ここではステータスコードのみ確認）
}

/// T026: トークンなしヘルスチェック拒否テスト
#[tokio::test]
async fn test_health_check_without_token_rejected() {
    let app = build_app().await;

    // ダミーのagent_idを使用（トークンがないので実際には使われない）
    let dummy_agent_id = Uuid::new_v4();
    let health_payload = json!({
        "agent_id": dummy_agent_id.to_string(),
        "cpu_usage": 45.5,
        "memory_usage": 60.2,
        "gpu_usage": null,
        "gpu_memory_usage": null,
        "gpu_memory_total_mb": null,
        "gpu_memory_used_mb": null,
        "gpu_temperature": null,
        "gpu_model_name": null,
        "gpu_compute_capability": null,
        "gpu_capability_score": null,
        "active_requests": 3,
        "average_response_time_ms": 110.0,
        "loaded_models": ["gpt-oss:20b"]
    });

    // Step 1: X-Agent-Tokenヘッダーなしで POST /api/health にアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&health_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 2: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Health check without token should be unauthorized"
    );

    // Step 3: 無効なトークンでアクセス
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("x-agent-token", "agt_invalid_token_12345")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&health_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 4: 401 Unauthorized を受信
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Health check with invalid token should be unauthorized"
    );

    // Step 5: 削除されたエージェントのトークンでアクセス
    // まず、エージェントを登録してトークンを取得
    let register_payload = json!({
        "machine_name": "to-be-deleted-agent",
        "ip_address": "192.168.1.102",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 3090", "count": 1}
        ]
    });

    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agents")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_token = agent["agent_token"].as_str().unwrap().to_string();
    let agent_id = agent["agent_id"].as_str().unwrap();

    // JWT認証でログインして管理者トークンを取得
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let login_body = to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let jwt_token = login_data["token"].as_str().unwrap();

    // エージェントを削除
    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/agents/{}", agent_id))
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        delete_response.status(),
        StatusCode::NO_CONTENT,
        "Agent deletion should succeed"
    );

    // 削除されたエージェントのトークンでヘルスチェック
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/health")
                .header("x-agent-token", agent_token)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&health_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Step 6: 401 Unauthorized または 404 Not Found を受信
    // （削除されたエージェントは存在しないため404が返される場合もある）
    assert!(
        response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::NOT_FOUND,
        "Health check with deleted agent's token should be unauthorized or not found, got: {}",
        response.status()
    );
}
