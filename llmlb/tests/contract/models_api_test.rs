//! モデル管理API契約テスト
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::{ApiKeyPermission, UserRole};
use llmlb::db::models::ModelStorage;
use llmlb::registry::models::ModelInfo;
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestApp {
    app: Router,
    db_pool: sqlx::SqlitePool,
    admin_key: String,
    node_key: String,
}

// Node主導キャッシュのため、registry manifest に外部ソースURLが含まれること
#[tokio::test]
#[serial]
async fn registry_manifest_includes_origin_urls() {
    let mock = MockServer::start().await;
    std::env::set_var("HF_BASE_URL", mock.uri());

    let model_name = "openai/gpt-oss-7b";
    let repo = "openai/gpt-oss-7b";
    let filename = "model.Q4_K_M.gguf";

    Mock::given(method("GET"))
        .and(path("/api/models/openai/gpt-oss-7b"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "model.Q4_K_M.gguf"}
            ]
        })))
        .mount(&mock)
        .await;

    let TestApp {
        app,
        node_key,
        db_pool,
        ..
    } = build_app().await;

    let mut model = ModelInfo::new(model_name.to_string(), 0, repo.to_string(), 0, vec![]);
    model.repo = Some(repo.to_string());
    model.filename = Some(filename.to_string());
    let storage = ModelStorage::new(db_pool.clone());
    storage.save_model(&model).await.unwrap();

    let encoded = model_name.replace("/", "%2F");
    let response = app
        .oneshot(
            node_request(&node_key)
                .method("GET")
                .uri(format!("/api/models/registry/{}/manifest.json", encoded))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let manifest_format = v.get("format").and_then(|f| f.as_str());
    assert_eq!(manifest_format, Some("gguf"));
    let quantization = v.get("quantization").and_then(|q| q.as_str());
    assert_eq!(quantization, Some("Q4_K_M"));
    let files = v.get("files").and_then(|f| f.as_array()).unwrap();
    let entry = files
        .iter()
        .find(|f| f.get("name").and_then(|n| n.as_str()) == Some("model.gguf"))
        .expect("model.gguf not found in manifest");
    let url = entry.get("url").and_then(|u| u.as_str()).unwrap();
    let expected_url = format!("{}/{}/resolve/main/{}", mock.uri(), repo, filename);
    assert_eq!(url, expected_url);
}

async fn build_app() -> TestApp {
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

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

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin_user = llmlb::db::users::create(
        &state.db_pool,
        "admin",
        &password_hash,
        UserRole::Admin,
        false,
    )
    .await
    .expect("create admin user");
    let admin_key = llmlb::db::api_keys::create(
        &state.db_pool,
        "admin-key",
        admin_user.id,
        None,
        ApiKeyPermission::all(),
    )
    .await
    .expect("create admin api key")
    .key;

    let node_key = llmlb::db::api_keys::create(
        &state.db_pool,
        "node-key",
        admin_user.id,
        None,
        vec![ApiKeyPermission::RegistryRead],
    )
    .await
    .expect("create node api key")
    .key;

    let db_pool = state.db_pool.clone();
    let app = api::create_app(state);
    TestApp {
        app,
        db_pool,
        admin_key,
        node_key,
    }
}

fn admin_request(admin_key: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", admin_key))
}

fn node_request(node_key: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", node_key))
}

/// モデル配布APIは廃止（ノードが /api/models/registry/:model/manifest.json から自律取得）
#[tokio::test]
#[serial]
async fn test_distribute_models_endpoint_is_removed() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let request_body = json!({
        "model_name": "gpt-oss-20b",
        "target": "specific",
        "node_ids": []
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/distribute")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        matches!(
            response.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "distribute models endpoint should be removed"
    );
}

/// ノードモデル取得APIは廃止
#[tokio::test]
#[serial]
async fn test_node_models_endpoint_is_removed() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri(format!("/api/runtimes/{}/models", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "node models endpoint should be removed"
    );
}

/// ノードへのモデルpull指示APIは廃止（ノードが自律的に取得）
#[tokio::test]
#[serial]
async fn test_pull_model_to_node_endpoint_is_removed() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let request_body = json!({
        "model_name": "gpt-oss-3b"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri(format!("/api/runtimes/{}/models/pull", Uuid::new_v4()))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "node model pull endpoint should be removed"
    );
}

/// ダウンロードタスクAPIは廃止（モデル同期はエンドポイント側でオンデマンドに実行）
#[tokio::test]
#[serial]
async fn test_tasks_endpoint_is_removed() {
    let TestApp { app, admin_key, .. } = build_app().await;

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/api/tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "tasks endpoint should be removed"
    );
}

/// POST /api/models/register - 正常系と重複/404異常系
#[tokio::test]
#[serial]
async fn test_register_model_contract() {
    let mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/models/test/repo"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "model.gguf"}
            ]
        })))
        .mount(&mock)
        .await;

    std::env::set_var("HF_BASE_URL", mock.uri());

    let TestApp { app, admin_key, .. } = build_app().await;

    // 正常登録
    let payload = json!({
        "repo": "test/repo",
        "filename": "model.gguf",
        "display_name": "Test Model",
        "chat_template": "test template"
    });

    let response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    // SPEC-6cd7f960 FR-6: /v1/models はオンラインエンドポイントの実行可能モデルのみ返す
    // 登録しただけでエンドポイントがない場合は含まれない
    let models_res = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models_res.status(), StatusCode::OK);
    let body = to_bytes(models_res.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = body["data"]
        .as_array()
        .expect("'data' must be an array on /v1/models");
    assert!(
        !data.iter().any(|m| m["id"] == "test/repo"),
        "/v1/models should NOT include registered model without online endpoints (FR-6)"
    );

    // 重複登録は400
    let dup = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::BAD_REQUEST);

    // 異常系: 指定したファイルがsiblingsに存在しない
    Mock::given(method("GET"))
        .and(path("/api/models/missing/repo"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "other.gguf"}
            ]
        })))
        .mount(&mock)
        .await;

    let missing_payload = json!({
        "repo": "missing/repo",
        "filename": "absent.gguf",
        "chat_template": "test template"
    });

    let missing = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&missing_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);

    // repoのみ指定で safetensors の場合は登録できる（config/tokenizer必須）
    Mock::given(method("GET"))
        .and(path("/api/models/safetensors-repo"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "tokenizer.json"},
                {"rfilename": "model.safetensors"}
            ]
        })))
        .mount(&mock)
        .await;

    let safetensors_repo_only = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "repo": "safetensors-repo",
                        "chat_template": "test template"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(safetensors_repo_only.status(), StatusCode::CREATED);

    // shard URL指定時は index を優先して登録する
    Mock::given(method("GET"))
        .and(path("/api/models/sharded/with-index"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "tokenizer.json"},
                {"rfilename": "model-00001-of-00002.safetensors"},
                {"rfilename": "model-00002-of-00002.safetensors"},
                {"rfilename": "model.safetensors.index.json"}
            ]
        })))
        .mount(&mock)
        .await;

    let shard_payload = json!({
        "repo": format!("{}/sharded/with-index/resolve/main/model-00001-of-00002.safetensors", mock.uri()),
        "chat_template": "test template"
    });

    let shard_response = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&shard_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(shard_response.status(), StatusCode::CREATED);
    let shard_body = to_bytes(shard_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let shard_body: serde_json::Value = serde_json::from_slice(&shard_body).unwrap();
    assert_eq!(
        shard_body["filename"].as_str(),
        Some("model.safetensors.index.json")
    );
}

/// safetensors登録では config.json + tokenizer.json を必須とする
#[tokio::test]
#[serial]
async fn test_register_safetensors_requires_metadata_files() {
    let mock = MockServer::start().await;
    std::env::set_var("HF_BASE_URL", mock.uri());

    Mock::given(method("GET"))
        .and(path("/api/models/safetensors-missing-meta"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "model.safetensors"}
            ]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "repo": "safetensors-missing-meta",
        "chat_template": "test template"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "safetensors register should require config/tokenizer metadata"
    );
}

/// 複数の safetensors ファイルがある場合は index.json を要求する
#[tokio::test]
#[serial]
async fn test_register_safetensors_sharded_requires_index_file() {
    let mock = MockServer::start().await;
    std::env::set_var("HF_BASE_URL", mock.uri());

    Mock::given(method("GET"))
        .and(path("/api/models/sharded-safetensors"))
        .and(query_param("expand", "siblings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "siblings": [
                {"rfilename": "config.json"},
                {"rfilename": "tokenizer.json"},
                {"rfilename": "model-00001.safetensors"},
                {"rfilename": "model-00002.safetensors"}
            ]
        })))
        .mount(&mock)
        .await;

    let TestApp { app, admin_key, .. } = build_app().await;

    let payload = json!({
        "repo": "sharded-safetensors",
        "chat_template": "test template"
    });

    let response = app
        .oneshot(
            admin_request(&admin_key)
                .method("POST")
                .uri("/api/models/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "sharded safetensors repo should require .safetensors.index.json"
    );
}

/// 削除後に /v1/models から消える
#[tokio::test]
#[serial]
async fn test_delete_model_removes_from_list() {
    let TestApp {
        app,
        admin_key,
        db_pool,
        ..
    } = build_app().await;

    let model_name = "delete-me";

    let model = ModelInfo::new(model_name.to_string(), 0, "test".to_string(), 0, vec![]);
    let storage = ModelStorage::new(db_pool.clone());
    storage.save_model(&model).await.unwrap();

    // SPEC-6cd7f960 FR-6: /v1/models はオンラインエンドポイントの実行可能モデルのみ返す
    // 登録しただけでエンドポイントがない場合は含まれない
    let models_res = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models_res.status(), StatusCode::OK);
    let body = to_bytes(models_res.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        body["data"]
            .as_array()
            .map(|arr| !arr.iter().any(|m| m["id"] == model_name))
            .unwrap_or(true),
        "model should NOT appear in /v1/models without online endpoints (FR-6)"
    );

    let delete_res = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("DELETE")
                .uri(format!("/api/models/{}", model_name))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_res.status(), StatusCode::NO_CONTENT);

    let models_res = app
        .clone()
        .oneshot(
            admin_request(&admin_key)
                .method("GET")
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models_res.status(), StatusCode::OK);
    let body = to_bytes(models_res.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        body["data"]
            .as_array()
            .map(|arr| arr.iter().all(|m| m["id"] != model_name))
            .unwrap_or(false),
        "model should remain absent in /v1/models after delete"
    );
}
