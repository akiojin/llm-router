//! Contract Test: Ollama Generate APIプロキシ (POST /api/generate)
//!
//! `/api/generate` を実ポートで起動したスタブエージェントに中継し、
//! OpenAI互換のレスポンス/エラーハンドリングを検証する。

use std::sync::Arc;

use crate::support::{
    coordinator::{register_agent, spawn_coordinator},
    http::{spawn_router, TestServer},
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use ollama_coordinator_common::protocol::GenerateRequest;
use reqwest::{Client, StatusCode as ReqStatusCode};
use serde_json::Value;

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
        .route("/v1/completions", post(agent_generate_handler))
        .route("/v1/models", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"data": [{"id": "gpt-oss:20b"}], "object": "list"}))
        }))
        .route("/api/tags", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"models": [{"name": "gpt-oss:20b", "size": 10000000000i64}]}))
        }))
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

#[tokio::test]
async fn proxy_generate_end_to_end_success() {
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        response: AgentGenerateStubResponse::Success(serde_json::json!({
            "response": "stubbed",
            "done": true
        })),
    })
    .await;
    let coordinator = spawn_coordinator().await;

    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/generate", coordinator.addr()))
        .json(&GenerateRequest {
            model: "gpt-oss:20b".into(),
            prompt: "ping".into(),
            stream: false,
        })
        .send()
        .await
        .expect("generate request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: Value = response.json().await.expect("valid json response");
    assert_eq!(body["response"], "stubbed");
    assert_eq!(body["done"], true);

    coordinator.stop().await;
    agent_stub.stop().await;
}

#[tokio::test]
async fn proxy_generate_propagates_upstream_error() {
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("missing-model".to_string()),
        response: AgentGenerateStubResponse::Error(
            StatusCode::BAD_REQUEST,
            "model not loaded".to_string(),
        ),
    })
    .await;
    let coordinator = spawn_coordinator().await;

    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/generate", coordinator.addr()))
        .json(&GenerateRequest {
            model: "missing-model".into(),
            prompt: "ping".into(),
            stream: false,
        })
        .send()
        .await
        .expect("generate request should succeed");

    assert_eq!(response.status(), ReqStatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("error payload");
    assert_eq!(body["error"]["type"], "ollama_upstream_error");
    assert_eq!(body["error"]["code"], 400);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("model not loaded"));

    coordinator.stop().await;
    agent_stub.stop().await;
}
