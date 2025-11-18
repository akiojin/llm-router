//! Contract Test: Ollama Chat APIプロキシ (POST /api/chat)
//!
//! 実際にHTTPで待ち受けるスタブエージェントとコーディネーターを起動し、
//! OpenAI互換の正常系/異常系を確認する。

use std::sync::Arc;

use crate::support::{
    coordinator::{register_agent, spawn_coordinator},
    http::{spawn_router, TestServer},
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use ollama_coordinator_common::protocol::{ChatRequest, ChatResponse};
use reqwest::{Client, StatusCode as ReqStatusCode};

#[derive(Clone)]
struct AgentStubState {
    expected_model: Option<String>,
    chat_response: AgentChatStubResponse,
}

#[derive(Clone)]
enum AgentChatStubResponse {
    Success(ChatResponse),
    Error(StatusCode, String),
}

async fn spawn_agent_stub(state: AgentStubState) -> TestServer {
    let router = Router::new()
        .route("/v1/chat/completions", post(agent_chat_handler))
        .route("/v1/models", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"data": [{"id": "gpt-oss:20b"}], "object": "list"}))
        }))
        .route("/api/tags", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"models": [{"name": "gpt-oss:20b", "size": 10000000000i64}]}))
        }))
        .with_state(Arc::new(state));

    spawn_router(router).await
}

async fn agent_chat_handler(
    State(state): State<Arc<AgentStubState>>,
    Json(req): Json<ChatRequest>,
) -> impl axum::response::IntoResponse {
    if let Some(expected) = &state.expected_model {
        assert_eq!(
            &req.model, expected,
            "coordinator should proxy the requested model name"
        );
    }

    match &state.chat_response {
        AgentChatStubResponse::Success(resp) => {
            (StatusCode::OK, Json(resp.clone())).into_response()
        }
        AgentChatStubResponse::Error(status, body) => (*status, body.clone()).into_response(),
    }
}

#[tokio::test]
async fn proxy_chat_end_to_end_success() {
    // Arrange: スタブエージェントとコーディネーターを実ポートで起動
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        chat_response: AgentChatStubResponse::Success(ChatResponse {
            message: ollama_coordinator_common::protocol::ChatMessage {
                role: "assistant".into(),
                content: "Hello from stub".into(),
            },
            done: true,
        }),
    })
    .await;
    let coordinator = spawn_coordinator().await;

    // エージェント登録
    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent request must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    // Act: /api/chat にOpenAI互換リクエストを送信
    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/chat", coordinator.addr()))
        .json(&ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![ollama_coordinator_common::protocol::ChatMessage {
                role: "user".into(),
                content: "Hello?".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should succeed");

    // Assert: コーディネーターがスタブのレスポンスをそのまま返す
    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: ChatResponse = response.json().await.expect("valid chat response");
    assert_eq!(body.message.content, "Hello from stub");
    assert!(body.done);

    // Shutdown
    coordinator.stop().await;
    agent_stub.stop().await;
}

#[tokio::test]
async fn proxy_chat_propagates_upstream_error() {
    // Arrange: エージェントが404を返すケース
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("missing-model".to_string()),
        chat_response: AgentChatStubResponse::Error(
            StatusCode::NOT_FOUND,
            "model not found".to_string(),
        ),
    })
    .await;
    let coordinator = spawn_coordinator().await;

    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    // Act: 存在しないモデルを指定
    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/chat", coordinator.addr()))
        .json(&ChatRequest {
            model: "missing-model".into(),
            messages: vec![ollama_coordinator_common::protocol::ChatMessage {
                role: "user".into(),
                content: "ping".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should reach coordinator");

    // Assert: コーディネーターが404とOpenAI互換のエラー形式を返す
    assert_eq!(response.status(), ReqStatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("error payload");
    assert_eq!(body["error"]["type"], "ollama_upstream_error");
    assert_eq!(body["error"]["code"], 404);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("model not found"),
        "original upstream message should be preserved"
    );

    coordinator.stop().await;
    agent_stub.stop().await;
}
