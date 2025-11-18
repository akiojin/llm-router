//! モデル管理API契約テスト
//!
//! TDD RED: これらのテストは実装前に失敗する必要があります

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

fn build_app() -> Router {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = ollama_coordinator_coordinator::tasks::DownloadTaskManager::new();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    };

    api::create_router(state)
}

/// T004: GET /api/models/available の契約テスト
#[tokio::test]
async fn test_get_available_models_contract() {
    let app = build_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/available")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // ステータスコードの検証
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK for GET /api/models/available"
    );

    // レスポンスボディの検証
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // スキーマ検証
    assert!(
        body.get("models").is_some(),
        "Response must have 'models' field"
    );
    assert!(body["models"].is_array(), "'models' field must be an array");

    // source フィールドが存在することを確認
    assert!(
        body.get("source").is_some(),
        "Response must have 'source' field"
    );
    let source = body["source"].as_str().expect("'source' must be a string");
    assert!(
        ["ollama_library", "agents"].contains(&source),
        "'source' must be 'ollama_library' or 'agents'"
    );

    // models配列の各要素の検証
    if let Some(models) = body["models"].as_array() {
        for model in models {
            assert!(model.get("name").is_some(), "Model must have 'name'");
            assert!(model.get("size").is_some(), "Model must have 'size'");
            assert!(
                model.get("description").is_some(),
                "Model must have 'description'"
            );
            assert!(
                model.get("required_memory").is_some(),
                "Model must have 'required_memory'"
            );
            assert!(model.get("tags").is_some(), "Model must have 'tags'");
            assert!(model["tags"].is_array(), "'tags' must be an array");
        }
    }
}

/// T005: POST /api/models/distribute の契約テスト
#[tokio::test]
async fn test_distribute_models_contract() {
    let app = build_app();

    // テスト用リクエスト
    let request_body = json!({
        "model_name": "gpt-oss:20b",
        "target": "all",
        "agent_ids": []
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // ステータスコードの検証
    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "Expected 202 ACCEPTED for POST /api/models/distribute"
    );

    // レスポンスボディの検証
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // スキーマ検証
    assert!(
        body.get("task_ids").is_some(),
        "Response must have 'task_ids' field"
    );
    assert!(
        body["task_ids"].is_array(),
        "'task_ids' field must be an array"
    );

    // task_ids配列の各要素がUUID文字列であることを確認
    if let Some(task_ids) = body["task_ids"].as_array() {
        for task_id in task_ids {
            let task_id_str = task_id.as_str().expect("task_id must be a string");
            Uuid::parse_str(task_id_str).expect("task_id must be a valid UUID");
        }
    }
}

/// T006: GET /api/agents/{agent_id}/models の契約テスト
#[tokio::test]
async fn test_get_agent_models_contract() {
    let app = build_app();

    // テスト用のエージェントを登録
    let register_payload = json!({
        "machine_name": "test-agent",
        "ip_address": "127.0.0.1",
        "ollama_version": "0.1.0",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "Test GPU", "count": 1}
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

    assert_eq!(register_response.status(), StatusCode::OK);

    // エージェントIDを取得
    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("Agent must have 'agent_id' field");

    // モデル一覧を取得
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/agents/{}/models", agent_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // ステータスコードの検証
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK for GET /api/agents/:id/models"
    );

    // レスポンスボディの検証（InstalledModelの配列）
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(body.is_array(), "Response must be an array");

    // 配列の各要素の検証
    if let Some(models) = body.as_array() {
        for model in models {
            assert!(model.get("name").is_some(), "Model must have 'name'");
            assert!(model.get("size").is_some(), "Model must have 'size'");
            assert!(
                model.get("installed_at").is_some(),
                "Model must have 'installed_at'"
            );
            // digestはオプション
        }
    }
}

/// T007: POST /api/agents/{agent_id}/models/pull の契約テスト
#[tokio::test]
async fn test_pull_model_contract() {
    let app = build_app();

    // テスト用のエージェントを登録
    let register_payload = json!({
        "machine_name": "test-agent",
        "ip_address": "127.0.0.1",
        "ollama_version": "0.1.0",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "Test GPU", "count": 1}
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

    assert_eq!(register_response.status(), StatusCode::OK);

    // エージェントIDを取得
    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("Agent must have 'agent_id' field");

    // モデルプル
    let request_body = json!({
        "model_name": "gpt-oss:3b"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/agents/{}/models/pull", agent_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // ステータスコードの検証
    assert_eq!(
        response.status(),
        StatusCode::ACCEPTED,
        "Expected 202 ACCEPTED for POST /api/agents/:id/models/pull"
    );

    // レスポンスボディの検証
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // スキーマ検証
    assert!(
        body.get("task_id").is_some(),
        "Response must have 'task_id' field"
    );
    let task_id_str = body["task_id"]
        .as_str()
        .expect("'task_id' must be a string");
    Uuid::parse_str(task_id_str).expect("'task_id' must be a valid UUID");
}

/// T008: GET /api/tasks/{task_id} の契約テスト
#[tokio::test]
async fn test_get_task_progress_contract() {
    let app = build_app();

    // テスト用のエージェントを登録
    let register_payload = json!({
        "machine_name": "test-agent",
        "ip_address": "127.0.0.1",
        "ollama_version": "0.1.0",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "Test GPU", "count": 1}
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

    assert_eq!(register_response.status(), StatusCode::OK);

    // エージェントIDを取得
    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("Agent must have 'agent_id' field");

    // モデルプルを開始してタスクIDを取得
    let request_body = json!({
        "model_name": "gpt-oss:3b"
    });

    let pull_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/agents/{}/models/pull", agent_id))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = to_bytes(pull_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let pull_result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let task_id = pull_result["task_id"]
        .as_str()
        .expect("Pull response must have 'task_id'");

    // タスク進捗を取得
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/tasks/{}", task_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // ステータスコードの検証
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK for GET /api/tasks/:id"
    );

    // レスポンスボディの検証（DownloadTask構造体）
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // スキーマ検証
    assert!(body.get("id").is_some(), "Task must have 'id'");
    assert!(body.get("agent_id").is_some(), "Task must have 'agent_id'");
    assert!(
        body.get("model_name").is_some(),
        "Task must have 'model_name'"
    );
    assert!(body.get("status").is_some(), "Task must have 'status'");
    assert!(body.get("progress").is_some(), "Task must have 'progress'");
    assert!(
        body.get("started_at").is_some(),
        "Task must have 'started_at'"
    );

    // statusフィールドの検証
    let status = body["status"].as_str().expect("'status' must be a string");
    assert!(
        ["pending", "in_progress", "completed", "failed"].contains(&status),
        "'status' must be one of: pending, in_progress, completed, failed"
    );

    // progressフィールドの検証（0.0-1.0の範囲）
    let progress = body["progress"]
        .as_f64()
        .expect("'progress' must be a number");
    assert!(
        (0.0..=1.0).contains(&progress),
        "'progress' must be between 0.0 and 1.0"
    );

    // UUIDの検証
    let id_str = body["id"].as_str().expect("'id' must be a string");
    Uuid::parse_str(id_str).expect("'id' must be a valid UUID");

    let agent_id_str = body["agent_id"]
        .as_str()
        .expect("'agent_id' must be a string");
    Uuid::parse_str(agent_id_str).expect("'agent_id' must be a valid UUID");
}
