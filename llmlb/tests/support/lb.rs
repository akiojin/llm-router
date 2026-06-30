use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::Router;
use llmlb::common::auth::UserRole;
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use reqwest::{Client, Response};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::http::{spawn_lb, TestServer};

/// 指定したDBプールからテスト用のRouterを構築する（複数ヘルパー共用）
#[allow(dead_code)]
pub async fn build_test_router(db_pool: SqlitePool) -> Router {
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = test_jwt_secret();

    // EndpointRegistryを初期化
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");

    // LoadManagerはEndpointRegistryを使用
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));

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

    api::create_app(state)
}

/// テスト用のload balancerを作成する（.oneshot()スタイルのテスト用）
#[allow(dead_code)]
pub async fn create_test_lb() -> (Router, SqlitePool) {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!("or-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

    std::env::set_var("LLM_CONVERT_FAKE", "1");

    let db_pool = create_test_db_pool().await;
    let app = build_test_router(db_pool.clone()).await;
    (app, db_pool)
}

/// US-013: 既定構成（認証必須化後）のテスト用 load balancer。
///
/// 旧 `api_key_required` 設定を一切付与しない素の構成で app を作る。
/// 認証必須化後は未認証リクエストが必ず 401 になることを検証するために使用する。
#[allow(dead_code)]
pub async fn create_test_lb_default_auth() -> (Router, SqlitePool) {
    let temp_dir = std::env::temp_dir().join(format!("or-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);
    std::env::set_var("LLM_CONVERT_FAKE", "1");

    let db_pool = create_test_db_pool_default_auth().await;
    let app = build_test_router(db_pool.clone()).await;
    (app, db_pool)
}

/// テスト用のSQLiteデータベースプールを作成する
pub async fn create_test_db_pool() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory SQLite pool");

    // マイグレーションを実行
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

/// US-013: 認証必須化後の既定構成のDBプール（`create_test_db_pool` と同一）。
#[allow(dead_code)]
pub async fn create_test_db_pool_default_auth() -> SqlitePool {
    create_test_db_pool().await
}

/// テスト用のJWT秘密鍵を生成する
pub fn test_jwt_secret() -> String {
    "test-jwt-secret-key-for-testing-only".to_string()
}

/// llmlbサーバーをテスト用に起動する
#[allow(dead_code)]
pub async fn spawn_test_lb() -> TestServer {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!("or-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);
    std::env::set_var("LLM_CONVERT_FAKE", "1");

    let db_pool = create_test_db_pool().await;
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = test_jwt_secret();

    // EndpointRegistryを初期化
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");

    // LoadManagerはEndpointRegistryを使用
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));

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
    spawn_lb(app).await
}

/// llmlbサーバーをテスト用に起動し、LoadManagerも返す
#[allow(dead_code)]
pub async fn spawn_test_lb_with_manager() -> (TestServer, LoadManager) {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!("or-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);
    std::env::set_var("LLM_CONVERT_FAKE", "1");

    let db_pool = create_test_db_pool().await;
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = test_jwt_secret();

    // EndpointRegistryを初期化
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");

    // LoadManagerはEndpointRegistryを使用
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));

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
        load_manager: load_manager.clone(),
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
    let server = spawn_lb(app).await;
    (server, load_manager)
}

/// 指定したllmlbにノードを登録する
#[allow(dead_code)]
pub async fn register_node(
    lb_addr: SocketAddr,
    node_addr: SocketAddr,
) -> reqwest::Result<Response> {
    let response = register_node_with_runtimes(lb_addr, node_addr, vec![]).await?;
    Ok(response)
}

/// SPEC-e8e9326e: POST /api/runtimes は廃止されました。
/// このヘルパー関数は後方互換性のために残されていますが、
/// 新しいテストは Endpoints API を使用してください。
///
/// 指定したllmlbにノードを登録する（ランタイムタイプ指定可能）
/// レスポンスのボディには {"runtime_id": "...", "token": "..."} 形式が含まれます
pub async fn register_node_with_runtimes(
    lb_addr: SocketAddr,
    node_addr: SocketAddr,
    supported_runtimes: Vec<&str>,
) -> reqwest::Result<Response> {
    use serde_json::json;

    // 1. 内部APIを使ってノードを登録するための仮想レスポンスを作成
    // POST /api/runtimes が廃止されたため、内部 /api/internal/test/register-runtime を使用
    let payload = json!({
        "machine_name": "stub-node",
        "ip_address": node_addr.ip().to_string(),
        "runtime_version": "0.0.0-test",
        "runtime_port": node_addr.port().saturating_sub(1),
        "gpu_available": true,
        "gpu_devices": [
            {"model": "Test GPU", "count": 1, "memory": 16_000_000_000u64}
        ],
        "supported_runtimes": supported_runtimes
    });

    // テスト専用の内部エンドポイントを使用
    Client::new()
        .post(format!(
            "http://{lb_addr}/api/internal/test/register-runtime"
        ))
        .header("authorization", "Bearer sk_debug")
        .json(&payload)
        .send()
        .await
}

/// 音声認識（ASR）対応エンドポイントを登録する
/// （SPEC-e8e9326e: EndpointRegistry経由）
#[allow(dead_code)]
pub async fn register_audio_transcription_endpoint(
    lb_addr: SocketAddr,
    stub_addr: SocketAddr,
) -> reqwest::Result<String> {
    register_endpoint_with_capabilities(
        lb_addr,
        stub_addr,
        "Audio Transcription Endpoint",
        &["audio_transcription"],
    )
    .await
}

/// 音声合成（TTS）対応エンドポイントを登録する
/// （SPEC-e8e9326e: EndpointRegistry経由）
#[allow(dead_code)]
pub async fn register_audio_speech_endpoint(
    lb_addr: SocketAddr,
    stub_addr: SocketAddr,
) -> reqwest::Result<String> {
    register_endpoint_with_capabilities(
        lb_addr,
        stub_addr,
        "Audio Speech Endpoint",
        &["audio_speech"],
    )
    .await
}

/// 画像生成対応エンドポイントを登録する
/// （SPEC-e8e9326e: EndpointRegistry経由）
#[allow(dead_code)]
pub async fn register_image_generation_endpoint(
    lb_addr: SocketAddr,
    stub_addr: SocketAddr,
) -> reqwest::Result<String> {
    register_endpoint_with_capabilities(
        lb_addr,
        stub_addr,
        "Image Generation Endpoint",
        &["image_generation"],
    )
    .await
}

/// 指定したcapabilitiesでエンドポイントを登録する
/// （SPEC-e8e9326e: EndpointRegistry経由）
#[allow(dead_code)]
pub async fn register_endpoint_with_capabilities(
    lb_addr: SocketAddr,
    stub_addr: SocketAddr,
    name: &str,
    capabilities: &[&str],
) -> reqwest::Result<String> {
    let client = Client::new();

    // 1. エンドポイントを作成
    let create_response = client
        .post(format!("http://{}/api/endpoints", lb_addr))
        .header("authorization", "Bearer sk_debug")
        .json(&json!({
            "name": format!("{} - {}", name, stub_addr),
            "base_url": format!("http://{}", stub_addr),
            "health_check_interval_secs": 30,
            "capabilities": capabilities
        }))
        .send()
        .await?;

    let create_body: Value = create_response.json().await.unwrap_or_default();
    let endpoint_id = create_body["id"].as_str().unwrap_or_default().to_string();

    // 2. エンドポイントをOnline状態にする
    let _ = client
        .post(format!(
            "http://{}/api/endpoints/{}/test",
            lb_addr, endpoint_id
        ))
        .header("authorization", "Bearer sk_debug")
        .send()
        .await?;

    Ok(endpoint_id)
}

/// Responses API対応エンドポイントを登録し、指定のモデルで利用可能にする
/// （SPEC-0f1de549: Open Responses API対応テスト用）
#[allow(dead_code)]
pub async fn register_responses_endpoint(
    lb_addr: SocketAddr,
    stub_addr: SocketAddr,
    model_id: &str,
) -> reqwest::Result<String> {
    let client = Client::new();

    // 1. エンドポイントを登録
    let create_response = client
        .post(format!("http://{}/api/endpoints", lb_addr))
        .header("authorization", "Bearer sk_debug")
        .json(&json!({
            "name": format!("Responses API Test Endpoint - {}", model_id),
            "base_url": format!("http://{}", stub_addr),
            "health_check_interval_secs": 30
        }))
        .send()
        .await?;

    let create_status = create_response.status();
    let create_body: Value = create_response.json().await.unwrap_or_default();
    let endpoint_id = create_body["id"].as_str().unwrap_or_default().to_string();

    if !create_status.is_success() || endpoint_id.is_empty() {
        eprintln!(
            "Failed to create endpoint: status={}, body={}",
            create_status, create_body
        );
    }

    // 2. エンドポイントをOnline状態にする（接続テストを実行）
    let test_response = client
        .post(format!(
            "http://{}/api/endpoints/{}/test",
            lb_addr, endpoint_id
        ))
        .header("authorization", "Bearer sk_debug")
        .send()
        .await?;

    let test_status = test_response.status();
    let test_body: Value = test_response.json().await.unwrap_or_default();
    if !test_status.is_success() {
        eprintln!(
            "Failed to test endpoint: status={}, body={}",
            test_status, test_body
        );
    }

    // 3. モデルを同期
    let sync_response = client
        .post(format!(
            "http://{}/api/endpoints/{}/sync",
            lb_addr, endpoint_id
        ))
        .header("authorization", "Bearer sk_debug")
        .send()
        .await?;

    let sync_status = sync_response.status();
    let sync_body: Value = sync_response.json().await.unwrap_or_default();
    if !sync_status.is_success() {
        eprintln!(
            "Failed to sync endpoint: status={}, body={}",
            sync_status, sync_body
        );
    }

    Ok(endpoint_id)
}

/// 指定したノードを管理者として承認する
#[allow(dead_code)]
pub async fn approve_node(lb_addr: SocketAddr, endpoint_id: &str) -> reqwest::Result<Response> {
    let client = Client::new();
    let login_response = client
        .post(format!("http://{}/api/auth/login", lb_addr))
        .json(&json!({
            "username": "admin",
            "password": "test"
        }))
        .send()
        .await?;

    let login_data: Value = login_response.json().await.unwrap_or_default();
    let token = login_data["token"].as_str().unwrap_or_default();

    client
        .post(format!(
            "http://{}/api/runtimes/{}/approve",
            lb_addr, endpoint_id
        ))
        .header("authorization", format!("Bearer {}", token))
        .send()
        .await
}

/// 登録レスポンスからノードを承認し、HTTPステータスとボディを返す
#[allow(dead_code)]
pub async fn approve_node_from_register_response(
    lb_addr: SocketAddr,
    register_response: Response,
) -> reqwest::Result<(reqwest::StatusCode, Value)> {
    let status = register_response.status();
    let body: Value = register_response.json().await.unwrap_or_default();

    if let Some(runtime_id) = body.get("runtime_id").and_then(|v| v.as_str()) {
        let _ = approve_node(lb_addr, runtime_id).await?;
    }

    Ok((status, body))
}

/// テスト用の管理者ユーザーを作成してAPIキーを取得する
#[allow(dead_code)]
pub async fn create_test_api_key(lb_addr: SocketAddr, db_pool: &SqlitePool) -> String {
    // 管理者ユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(db_pool, "admin", &password_hash, UserRole::Admin, false)
        .await
        .ok();

    let client = Client::new();

    // ログイン
    let login_response = client
        .post(format!("http://{}/api/auth/login", lb_addr))
        .json(&json!({
            "username": "admin",
            "password": "password123"
        }))
        .send()
        .await
        .expect("login should succeed");

    let login_data: Value = login_response.json().await.expect("login json");
    let jwt_token = login_data["token"].as_str().unwrap();

    // APIキーを発行
    let create_key_response = client
        .post(format!("http://{}/api/me/api-keys", lb_addr))
        .header("authorization", format!("Bearer {}", jwt_token))
        .json(&json!({
            "name": "Test API Key",
            "expires_at": null,
            "permissions": ["openai.inference", "openai.models.read"]
        }))
        .send()
        .await
        .expect("create api key should succeed");

    let key_data: Value = create_key_response.json().await.expect("api key json");
    key_data["key"].as_str().unwrap().to_string()
}

/// oneshotテストのリクエストにConnectInfo<SocketAddr>を追加する
///
/// ハンドラがConnectInfo<SocketAddr>を要求する場合、oneshotでは
/// extensionが自動設定されないため、このヘルパーで明示的に追加する。
#[allow(dead_code)]
pub fn with_connect_info(
    mut req: axum::http::Request<axum::body::Body>,
) -> axum::http::Request<axum::body::Body> {
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
    req
}

/// llmlbサーバーをテスト用に起動する（DBプールも返す）
#[allow(dead_code)]
pub async fn spawn_test_lb_with_db() -> (TestServer, SqlitePool) {
    // テスト用に一時ディレクトリを設定
    let temp_dir = std::env::temp_dir().join(format!(
        "or-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("LLMLB_DATA_DIR", &temp_dir);

    let db_pool = create_test_db_pool().await;
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = test_jwt_secret();

    // EndpointRegistryを初期化
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");

    // LoadManagerはEndpointRegistryを使用
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));

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

    let app = api::create_app(state);
    (spawn_lb(app).await, db_pool)
}
