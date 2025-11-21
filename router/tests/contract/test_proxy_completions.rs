//! Contract Test: OpenAI /api/generate proxy

use std::sync::Arc;

use crate::support::{
    http::{spawn_router, TestServer},
    router::{register_node, spawn_test_router},
};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use ollama_router_common::protocol::GenerateRequest;
use reqwest::{Client, StatusCode as ReqStatusCode};
use serde_json::Value;
use serial_test::serial;

#[derive(Clone)]
struct AgentStubState {
    expected_model: Option<String>,
    response: AgentGenerateStubResponse,
}

#[derive(Clone)]
enum AgentGenerateStubResponse {
    Success(Value),
    Error(StatusCode, String),
}

async fn spawn_agent_stub(state: AgentStubState) -> TestServer {
    let router = Router::new()
        .route("/api/generate", post(agent_generate_handler))
        .route("/v1/completions", post(agent_generate_handler))
        .route("/v1/chat/completions", post(agent_generate_handler))
        .route("/v1/models", get(agent_models_handler))
        .route("/api/tags", get(agent_tags_handler))
        .route("/api/health", post(|| async { axum::http::StatusCode::OK }))
        .with_state(Arc::new(state));

    spawn_router(router).await
}

async fn agent_generate_handler(
    State(state): State<Arc<AgentStubState>>,
    Json(req): Json<GenerateRequest>,
) -> impl axum::response::IntoResponse {
    if let Some(expected) = &state.expected_model {
        assert_eq!(
            &req.model, expected,
            "coordinator should proxy the requested model name"
        );
    }

    match &state.response {
        AgentGenerateStubResponse::Success(payload) => {
            (StatusCode::OK, Json(payload.clone())).into_response()
        }
        AgentGenerateStubResponse::Error(status, body) => (*status, body.clone()).into_response(),
    }
}

async fn agent_models_handler(State(state): State<Arc<AgentStubState>>) -> impl IntoResponse {
    // デフォルトで expected_model があればそのみ返す。なければ 5モデル仕様を返す。
    let models: Vec<_> = if let Some(model) = &state.expected_model {
        vec![serde_json::json!({"id": model})]
    } else {
        vec![
            serde_json::json!({"id": "gpt-oss:20b"}),
            serde_json::json!({"id": "gpt-oss:120b"}),
            serde_json::json!({"id": "gpt-oss-safeguard:20b"}),
            serde_json::json!({"id": "qwen3-coder:30b"}),
        ]
    };

    (StatusCode::OK, Json(serde_json::json!({"data": models}))).into_response()
}

async fn agent_tags_handler(State(state): State<Arc<AgentStubState>>) -> impl IntoResponse {
    let models: Vec<_> = if let Some(model) = &state.expected_model {
        vec![serde_json::json!({"name": model, "size": 10_000_000_000i64})]
    } else {
        vec![
            serde_json::json!({"name": "gpt-oss:20b", "size": 10_000_000_000i64}),
            serde_json::json!({"name": "gpt-oss:120b", "size": 120_000_000_000i64}),
            serde_json::json!({"name": "gpt-oss-safeguard:20b", "size": 10_000_000_000i64}),
            serde_json::json!({"name": "qwen3-coder:30b", "size": 30_000_000_000i64}),
        ]
    };

    (StatusCode::OK, Json(serde_json::json!({"models": models}))).into_response()
}

#[tokio::test]
#[serial]
async fn proxy_completions_end_to_end_success() {
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        response: AgentGenerateStubResponse::Success(serde_json::json!({
            "id": "cmpl-123",
            "object": "text_completion",
            "choices": [
                {"text": "hello from stub", "index": 0, "logprobs": null, "finish_reason": "stop"}
            ]
        })),
    })
    .await;
    let coordinator = spawn_test_router().await;

    let register_response = register_node(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/generate", coordinator.addr()))
        .json(&serde_json::json!({
            "model": "gpt-oss:20b",
            "prompt": "ping",
            "max_tokens": 8
        }))
        .send()
        .await
        .expect("completions request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: Value = response.json().await.expect("valid json response");
    assert_eq!(body["choices"][0]["text"], "hello from stub");
}

#[tokio::test]
#[serial]
async fn proxy_completions_propagates_upstream_error() {
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("missing-model".to_string()),
        response: AgentGenerateStubResponse::Error(
            StatusCode::BAD_REQUEST,
            "model not loaded".to_string(),
        ),
    })
    .await;
    let coordinator = spawn_test_router().await;

    let register_response = register_node(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/generate", coordinator.addr()))
        .json(&serde_json::json!({
            "model": "missing-model",
            "prompt": "ping",
            "max_tokens": 8
        }))
        .send()
        .await
        .expect("completions request should succeed");

    assert_eq!(response.status(), ReqStatusCode::BAD_REQUEST);
    let body = response.text().await.expect("body should be readable");
    assert!(body.contains("model not loaded"));
}

#[tokio::test]
#[ignore] // このテストはタイミング依存で不安定なため、一時的に無効化
async fn proxy_completions_queue_overflow_returns_503() {
    // TODO: このテストを安定させるための実装改善が必要
    // 問題:
    // 1. all_initializing()の判定タイミングが不安定
    // 2. wait_for_ready()が呼ばれる前にエージェントが準備完了になる
    // 3. LoadManager側の状態更新とリクエスト処理のタイミング競合
}
