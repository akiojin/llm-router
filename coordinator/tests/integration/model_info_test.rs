//! モデル情報表示統合テスト
//!
//! TDD RED: モデル一覧とエージェント別インストール状況の表示

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

/// T018: Ollamaライブラリから利用可能なモデル一覧を取得
#[tokio::test]
async fn test_list_available_models_from_ollama_library() {
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

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Available models endpoint should return 200 OK"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // modelsフィールドが配列であることを検証
    assert!(
        result.get("models").is_some(),
        "Response must have 'models' field"
    );
    let models = result["models"]
        .as_array()
        .expect("'models' must be an array");

    // 事前定義モデルが含まれることを検証
    let model_names: Vec<String> = models
        .iter()
        .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
        .collect();

    assert!(
        model_names.contains(&"gpt-oss:20b".to_string()),
        "Should include gpt-oss:20b"
    );
    assert!(
        model_names.contains(&"gpt-oss:7b".to_string()),
        "Should include gpt-oss:7b"
    );
    assert!(
        model_names.contains(&"gpt-oss:3b".to_string()),
        "Should include gpt-oss:3b"
    );
    assert!(
        model_names.contains(&"gpt-oss:1b".to_string()),
        "Should include gpt-oss:1b"
    );

    // 各モデルに必要な情報が含まれることを検証
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
    }
}

/// T019: 特定エージェントのインストール済みモデル一覧を取得
#[tokio::test]
async fn test_list_installed_models_on_agent() {
    let app = build_app();

    // テスト用エージェントを登録
    let register_payload = json!({
        "machine_name": "model-info-agent",
        "ip_address": "192.168.1.230",
        "ollama_version": "0.1.42",
        "ollama_port": 11434,
        "gpu_available": true,
        "gpu_devices": [
            {"model": "NVIDIA RTX 4090", "count": 1}
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

    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let agent_id = agent["agent_id"]
        .as_str()
        .expect("Agent must have 'agent_id' field");

    // エージェントのインストール済みモデル一覧を取得
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

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Agent models endpoint should return 200 OK"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let models: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // 配列であることを検証
    assert!(
        models.is_array(),
        "Response must be an array of installed models"
    );

    // 各モデルに必要な情報が含まれることを検証（空でない場合）
    if let Some(model_array) = models.as_array() {
        for model in model_array {
            assert!(model.get("name").is_some(), "Model must have 'name'");
            assert!(model.get("size").is_some(), "Model must have 'size'");
            assert!(
                model.get("installed_at").is_some(),
                "Model must have 'installed_at'"
            );
        }
    }
}

/// T020: 全エージェントのモデルマトリックス表示
#[tokio::test]
async fn test_model_matrix_view_multiple_agents() {
    let app = build_app();

    // 複数のエージェントを登録
    let mut agent_ids = Vec::new();
    for i in 0..3 {
        let register_payload = json!({
            "machine_name": format!("matrix-agent-{}", i),
            "ip_address": format!("192.168.1.{}", 240 + i),
            "ollama_version": "0.1.42",
            "ollama_port": 11434,
            "gpu_available": true,
            "gpu_devices": [
                {"model": "NVIDIA RTX 3090", "count": 1}
            ]
        });

        let response = app
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        agent_ids.push(
            agent["agent_id"]
                .as_str()
                .expect("Agent must have 'agent_id'")
                .to_string(),
        );
    }

    // 各エージェントのモデル一覧を取得
    for agent_id in &agent_ids {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/agents/{}/models", agent_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Each agent should have accessible model list"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let models: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(
            models.is_array(),
            "Each agent's model list must be an array"
        );
    }

    // 利用可能なモデル一覧も取得できることを確認
    let available_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/available")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        available_response.status(),
        StatusCode::OK,
        "Available models should be accessible for matrix view"
    );

    let body = to_bytes(available_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let available: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        available.get("models").is_some(),
        "Available models must have 'models' field"
    );
    assert!(
        available["models"].is_array(),
        "Available models must be an array"
    );
}
