//! モデル情報表示統合テスト
//!
//! TDD RED: モデル一覧とエンドポイント別インストール状況の表示
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use crate::support::{ollama::spawn_mock_ollama, xllm::spawn_mock_xllm};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::{ApiKeyPermission, UserRole};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn build_app() -> (Router, String, sqlx::SqlitePool) {
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
        audit_log_storage: std::sync::Arc::new(llmlb::db::audit_log::AuditLogStorage::new(
            db_pool.clone(),
        )),
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

    (api::create_app(state), admin_key, db_pool)
}

/// T018: /api/models/available は廃止され、/api/models に統合
/// NOTE: HuggingFaceカタログ参照は廃止。登録済みモデル一覧は /api/models で取得
#[tokio::test]
async fn test_available_models_endpoint_is_removed() {
    let (app, admin_key, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models/available")
                .header("authorization", format!("Bearer {}", admin_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // エンドポイントは削除済み
    // NOTE: 405 (Method Not Allowed) は /api/models/*model_name (DELETE用) にマッチするため
    assert!(
        response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::METHOD_NOT_ALLOWED,
        "/api/models/available GET endpoint should be removed (got {})",
        response.status()
    );
}

// NOTE: T019 (test_list_installed_models_on_endpoint) はSPEC-e8e9326e（NodeRegistry廃止）により削除。
// EndpointRegistryベースのモデル同期テストは llmlb/tests/contract/endpoints_sync_test.rs で実装済み。

/// T020: 複数エンドポイントのロード済みモデルの反映テスト
#[tokio::test]
async fn test_model_matrix_view_multiple_endpoints() {
    let (app, admin_key, _db_pool) = build_app().await;

    // モックサーバーを2つ起動
    let xllm = spawn_mock_xllm().await;
    let ollama = spawn_mock_ollama().await;

    // 2つのエンドポイントを登録
    for (name, base_url) in [
        ("xllm-matrix", format!("http://{}", xllm.addr())),
        ("ollama-matrix", format!("http://{}", ollama.addr())),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/endpoints")
                    .header("authorization", format!("Bearer {}", admin_key))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "name": name,
                            "base_url": base_url,
                            "health_check_interval_secs": 60,
                            "inference_timeout_secs": 30
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    // エンドポイント一覧で2件登録されていることを確認
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", admin_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let endpoints = body["endpoints"].as_array().unwrap();
    assert!(
        endpoints.len() >= 2,
        "expected at least two endpoints, got {}",
        endpoints.len()
    );
}

/// T021: /v1/models は対応モデル一覧を返す（APIキー認証必須）
#[tokio::test]
async fn test_v1_models_returns_fixed_list() {
    // テスト用のDBを作成
    let db_pool = crate::support::lb::create_test_db_pool().await;

    // テストユーザーとAPIキーを作成
    let password_hash = llmlb::auth::password::hash_password("testpassword").unwrap();
    let test_user = llmlb::db::users::create(
        &db_pool,
        "test-admin",
        &password_hash,
        llmlb::common::auth::UserRole::Admin,
        false,
    )
    .await
    .expect("Failed to create test user");
    let api_key = llmlb::db::api_keys::create(
        &db_pool,
        "test-key",
        test_user.id,
        None,
        vec![ApiKeyPermission::OpenaiModelsRead],
    )
    .await
    .expect("Failed to create test API key");

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

    let app = api::create_app(state);

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

    // ローカルモデルのみをフィルタ（クラウドプロバイダープレフィックスを除外）
    // SPEC-996e37bfでクラウドモデルが追加されたため、ローカルモデルのみを検証
    let cloud_prefixes = ["openai:", "google:", "anthropic:"];
    let local_ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .filter(|id| !cloud_prefixes.iter().any(|prefix| id.starts_with(prefix)))
        .collect();

    let expected: Vec<String> = vec![];

    assert_eq!(
        local_ids.len(),
        expected.len(),
        "should return only downloaded local models (cloud models are filtered out)"
    );
}
