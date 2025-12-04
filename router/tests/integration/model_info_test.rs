//! モデル情報表示統合テスト
//!
//! TDD RED: モデル一覧とノード別インストール状況の表示

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llm_router::{api, balancer::LoadManager, registry::NodeRegistry, AppState};
use serde_json::json;
use tower::ServiceExt;

async fn build_app() -> Router {
    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = llm_router::tasks::DownloadTaskManager::new();
    let db_pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run migrations");
    let jwt_secret = "test-secret".to_string();
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

/// T018: LLM runtimeライブラリから利用可能なモデル一覧を取得
#[tokio::test]
async fn test_list_available_models_from_runtime_library() {
    let app = build_app().await;

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

    assert!(model_names.contains(&"gpt-oss:20b".to_string()));
    assert!(model_names.contains(&"gpt-oss:120b".to_string()));
    assert!(model_names.contains(&"gpt-oss-safeguard:20b".to_string()));
    assert!(model_names.contains(&"qwen3-coder:30b".to_string()));

    // 各モデルに必要な情報が含まれることを検証
    for model in models {
        assert!(model.get("name").is_some(), "Model must have 'name'");
        assert!(model.get("size_gb").is_some(), "Model must have 'size_gb'");
        assert!(
            model.get("description").is_some(),
            "Model must have 'description'"
        );
        assert!(
            model.get("required_memory_gb").is_some(),
            "Model must have 'required_memory_gb'"
        );
        assert!(model.get("tags").is_some(), "Model must have 'tags'");
    }
}

/// T019: 特定ノードのインストール済みモデル一覧を取得
#[tokio::test]
async fn test_list_installed_models_on_agent() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let app = build_app().await;

    // テスト用ノードを登録
    let register_payload = json!({
        "machine_name": "model-info-agent",
        "ip_address": "192.168.1.230",
        "runtime_version": "0.1.42",
        "runtime_port": 11434,
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
                .uri("/api/nodes")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::CREATED);

    let body = to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let node_id = agent["node_id"]
        .as_str()
        .expect("Node must have 'node_id' field");

    // ノードのインストール済みモデル一覧を取得
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/nodes/{}/models", node_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Node models endpoint should return 200 OK"
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

/// T020: 全ノードのモデルマトリックス表示
#[tokio::test]
async fn test_model_matrix_view_multiple_agents() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    let app = build_app().await;

    // 複数のノードを登録
    let mut node_ids = Vec::new();
    for i in 0..3 {
        let register_payload = json!({
            "machine_name": format!("matrix-agent-{}", i),
            "ip_address": format!("192.168.1.{}", 240 + i),
            "runtime_version": "0.1.42",
            "runtime_port": 11434,
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
                    .uri("/api/nodes")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&register_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let agent: serde_json::Value = serde_json::from_slice(&body).unwrap();
        node_ids.push(
            agent["node_id"]
                .as_str()
                .expect("Node must have 'node_id'")
                .to_string(),
        );
    }

    // 各ノードのモデル一覧を取得
    for node_id in &node_ids {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/nodes/{}/models", node_id))
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

/// T021: /v1/models は対応モデル5件のみを返す（APIキー認証必須）
#[tokio::test]
async fn test_v1_models_returns_fixed_list() {
    // テスト用のDBを作成
    let db_pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run migrations");

    // テストユーザーとAPIキーを作成
    let test_user = llm_router::db::users::create(
        &db_pool,
        "test-admin",
        "testpassword",
        llm_router_common::auth::UserRole::Admin,
    )
    .await
    .expect("Failed to create test user");
    let api_key = llm_router::db::api_keys::create(&db_pool, "test-key", test_user.id, None)
        .await
        .expect("Failed to create test API key");

    let registry = NodeRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history =
        std::sync::Arc::new(llm_router::db::request_history::RequestHistoryStorage::new().unwrap());
    let task_manager = llm_router::tasks::DownloadTaskManager::new();
    let jwt_secret = "test-secret".to_string();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
        db_pool,
        jwt_secret,
    };

    let app = api::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("Authorization", format!("Bearer {}", api_key.key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let data = json["data"]
        .as_array()
        .expect("data must be an array of models");
    let ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let expected = vec![
        "gpt-oss:20b",
        "gpt-oss:120b",
        "gpt-oss-safeguard:20b",
        "qwen3-coder:30b",
    ];

    assert_eq!(ids.len(), expected.len(), "should return exactly 4 models");
    for id in expected {
        assert!(ids.contains(&id.to_string()), "missing {id}");
    }
}
