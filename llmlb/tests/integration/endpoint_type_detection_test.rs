//! Integration Test: US6 - エンドポイントタイプ自動判別
//!
//! SPEC-e8e9326e: エンドポイントタイプ自動判別機能
//!
//! 管理者として、エンドポイント登録時に自動的にタイプ
//! （xLLM/Ollama/vLLM/OpenAI互換）を判別してほしい。

use llmlb::common::auth::{ApiKeyPermission, UserRole};
use llmlb::health::EndpointHealthChecker;
use llmlb::registry::endpoints::EndpointRegistry;
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use crate::support::lb::spawn_test_lb_with_db;

async fn create_admin_api_key(db_pool: &SqlitePool) -> String {
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let created =
        llmlb::db::users::create(db_pool, "admin", &password_hash, UserRole::Admin, false).await;
    let admin_id = match created {
        Ok(user) => user.id,
        Err(_) => {
            llmlb::db::users::find_by_username(db_pool, "admin")
                .await
                .unwrap()
                .unwrap()
                .id
        }
    };

    let api_key = llmlb::db::api_keys::create(
        db_pool,
        "test-admin-key",
        admin_id,
        None,
        ApiKeyPermission::all(),
    )
    .await
    .expect("create admin api key");
    api_key.key
}

/// US6-シナリオ1: オフラインエンドポイント登録はエラーを返す
/// NOTE: 到達不能なエンドポイントは BAD_GATEWAY で拒否される
#[tokio::test]
async fn test_endpoint_type_auto_detection_offline() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    // エンドポイント登録（接続先がないのでエラーになる）
    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "Unreachable Endpoint",
            "base_url": "http://localhost:9999"
        }))
        .send()
        .await
        .expect("registration request failed");

    // 到達不能なエンドポイントは BAD_GATEWAY で拒否される
    assert_eq!(response.status().as_u16(), 502);
}

/// US6-シナリオ2: 判別の優先順位（xLLM > Ollama > vLLM > OpenAI互換）
/// NOTE: モックサーバーを使用したテストが必要（実際の判別ロジック検証）
#[tokio::test]
async fn test_endpoint_type_detection_priority() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "xllm_version": "0.1.0"
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": []
        })))
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("server", "vllm")
                .set_body_json(json!({"object": "list", "data": []})),
        )
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "Priority Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    assert_eq!(response.status().as_u16(), 201);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        body["endpoint_type"], "xllm",
        "xLLM should win priority over other detections"
    );
}

/// US6-シナリオ3: xLLM判別（/api/systemエンドポイント）
#[tokio::test]
async fn test_endpoint_type_detection_xllm() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "xllm_version": "0.1.0"
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "xLLM Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "xllm");
}

/// US6-シナリオ4: Ollama判別（/api/tagsエンドポイント）
#[tokio::test]
async fn test_endpoint_type_detection_ollama() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": []
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "Ollama Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "ollama");
}

/// US6-シナリオ5: vLLM判別（Serverヘッダー）
#[tokio::test]
async fn test_endpoint_type_detection_vllm() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("server", "vllm")
                .set_body_json(json!({"object": "list", "data": []})),
        )
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "vLLM Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "vllm");
}

/// US6-シナリオ5b: llama.cpp判別（Server header `llama.cpp/...`）
///
/// SPEC #575 US-013-A (2026-04-23 delta): UI 補完作業に併せて
/// llama.cpp 検出経路の登録 API レベル統合テストを追加する。
#[tokio::test]
async fn test_endpoint_type_detection_llamacpp_via_server_header() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("server", "llama.cpp/0.3.0")
                .set_body_json(json!({"object": "list", "data": []})),
        )
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "llama.cpp Endpoint via Server header",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "llamacpp");
}

/// US6-シナリオ5c: llama.cpp判別（`/v1/version` の server="llama.cpp"）
#[tokio::test]
async fn test_endpoint_type_detection_llamacpp_via_version_endpoint() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    // /v1/models は Server header なしで OK 応答（vLLM とも判定されない）
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"object": "list", "data": []})),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "server": "llama.cpp",
            "version": "0.3.0"
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "llama.cpp Endpoint via /v1/version",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "llamacpp");
}

/// US6-シナリオ6: LM Studio判別（/v1/models owned_byフォールバック）
#[tokio::test]
async fn test_endpoint_type_detection_lm_studio_owned_by_fallback() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    // /api/v1/models が使えない環境を想定し、/v1/models の owned_by で判定する
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "meta-llama-3.1-8b-instruct", "object": "model", "owned_by": "lmstudio-community"}]
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "LM Studio OwnedBy Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "lm_studio");
}

/// US6-シナリオ6b: LM Studio判別（/api/v1/models 新形式: models[]）
#[tokio::test]
async fn test_endpoint_type_detection_lm_studio_api_models_shape() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [{
                "type": "llm",
                "publisher": "openai",
                "key": "openai/gpt-oss-20b",
                "display_name": "GPT-OSS 20B",
                "architecture": "gpt_oss",
                "loaded_instances": [],
                "max_context_length": 131072,
                "format": "mlx"
            }]
        })))
        .mount(&mock)
        .await;
    // /v1/models の owned_by だけでは LM Studio 判別できないケース
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "openai/gpt-oss-20b", "object": "model", "owned_by": "organization_owner"}]
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "LM Studio API Models Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "lm_studio");
}

/// US6-シナリオ6c: /api/tags が 200 + error payload でも LM Studio を優先判定
#[tokio::test]
async fn test_endpoint_type_detection_lm_studio_when_api_tags_returns_error_payload() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    // LM Studio 実環境で見られる 200 + {"error": "..."} を再現
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": "Unexpected endpoint or method. (GET /api/tags)"
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [{
                "type": "llm",
                "publisher": "openai",
                "key": "openai/gpt-oss-20b",
                "display_name": "GPT-OSS 20B",
                "architecture": "gpt_oss",
                "loaded_instances": [{"id":"openai/gpt-oss-20b","config":{"context_length":131072}}],
                "max_context_length": 131072,
                "format": "mlx"
            }]
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id":"openai/gpt-oss-20b","object":"model","owned_by":"organization_owner"}]
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "LM Studio Tags Error Payload Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "lm_studio");
}

/// US6-シナリオ7: OpenAI互換判別（フォールバック）
#[tokio::test]
async fn test_endpoint_type_detection_openai_compatible() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "OpenAI Compatible Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "openai_compatible");
}

/// US6-シナリオ8: オンライン復帰時のタイプ再判別
#[tokio::test]
async fn test_endpoint_type_redetection_on_online() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let admin_key = create_admin_api_key(&db_pool).await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/health"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    // モックサーバーをbase_urlにして登録（OpenAI互換として判定される）
    let response = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", admin_key))
        .json(&json!({
            "name": "Test Endpoint",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .expect("registration request failed");

    assert_eq!(response.status().as_u16(), 201);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["endpoint_type"], "openai_compatible");

    let endpoint_id = body["id"].as_str().expect("endpoint id");
    let endpoint_uuid = uuid::Uuid::parse_str(endpoint_id).expect("endpoint uuid");

    let registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("endpoint registry init");
    let checker = EndpointHealthChecker::new(registry);
    let endpoint = llmlb::db::endpoints::get_endpoint(&db_pool, endpoint_uuid)
        .await
        .expect("get endpoint")
        .expect("endpoint exists");

    checker
        .check_endpoint(&endpoint)
        .await
        .expect("health check should succeed");

    let updated = llmlb::db::endpoints::get_endpoint(&db_pool, endpoint_uuid)
        .await
        .expect("get endpoint")
        .expect("endpoint exists");
    assert_eq!(updated.endpoint_type.as_str(), "openai_compatible");
}
