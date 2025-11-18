//! Contract Test: Ollama Chat APIプロキシ (POST /api/chat)
//!
//! 実際にHTTPで待ち受けるスタブノードとルーターを起動し、
//! OpenAI互換の正常系/異常系を確認する。

use std::sync::Arc;

use crate::support::{
    http::{spawn_router, TestServer},
    router::{register_node, spawn_test_router},
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use ollama_router_common::protocol::{ChatRequest, ChatResponse};
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
        .route("/api/chat", post(agent_chat_handler))
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
            "router should proxy the requested model name"
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
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    // Arrange: スタブノードとルーターを実ポートで起動
    let node_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        chat_response: AgentChatStubResponse::Success(ChatResponse {
            message: ollama_router_common::protocol::ChatMessage {
                role: "assistant".into(),
                content: "Hello from stub".into(),
            },
            done: true,
        }),
    })
    .await;
    let router = spawn_test_router().await;

    // ノード登録
    let register_response = register_node(router.addr(), node_stub.addr())
        .await
        .expect("register node request must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    // Act: /api/chat にOpenAI互換リクエストを送信
    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/chat", router.addr()))
        .json(&ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![ollama_router_common::protocol::ChatMessage {
                role: "user".into(),
                content: "Hello?".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should succeed");

    // Assert: ルーターがスタブのレスポンスをそのまま返す
    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: ChatResponse = response.json().await.expect("valid chat response");
    assert_eq!(body.message.content, "Hello from stub");
    assert!(body.done);

    // Shutdown
    router.stop().await;
    node_stub.stop().await;
}

#[tokio::test]
async fn proxy_chat_propagates_upstream_error() {
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    // Arrange: ノードが404を返すケース
    let node_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("missing-model".to_string()),
        chat_response: AgentChatStubResponse::Error(
            StatusCode::NOT_FOUND,
            "model not found".to_string(),
        ),
    })
    .await;
    let router = spawn_test_router().await;

    let register_response = register_node(router.addr(), node_stub.addr())
        .await
        .expect("register node must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    // Act: 存在しないモデルを指定
    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/chat", router.addr()))
        .json(&ChatRequest {
            model: "missing-model".into(),
            messages: vec![ollama_router_common::protocol::ChatMessage {
                role: "user".into(),
                content: "ping".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should reach coordinator");

    // Assert: ルーターが404とOpenAI互換のエラー形式を返す
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

    router.stop().await;
    node_stub.stop().await;
}
