//! Contract Test: LLM runtime Chat APIプロキシ (POST /api/chat)
//!
//! 実際にHTTPで待ち受けるスタブノードとルーターを起動し、
//! OpenAI互換の正常系/異常系を確認する。

use std::sync::Arc;

use crate::support::{
    http::{spawn_router, TestServer},
    router::{register_node, spawn_test_router},
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use llm_router_common::protocol::ChatRequest;
use reqwest::{Client, StatusCode as ReqStatusCode};
use serde_json::{json, Value};
use serial_test::serial;

#[derive(Clone)]
struct AgentStubState {
    expected_model: Option<String>,
    chat_response: AgentChatStubResponse,
}

#[derive(Clone)]
enum AgentChatStubResponse {
    Success(Value),
    Error(StatusCode, String),
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn remove(key: &'static str) -> Self {
        let original = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
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
        AgentChatStubResponse::Success(resp) => (StatusCode::OK, Json(resp)).into_response(),
        AgentChatStubResponse::Error(status, body) => (*status, body.clone()).into_response(),
    }
}

#[tokio::test]
#[serial]
async fn proxy_chat_end_to_end_success() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
    // Arrange: スタブノードとルーターを実ポートで起動
    let node_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        // OpenAI互換形式のレスポンス
        chat_response: AgentChatStubResponse::Success(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from stub"
                },
                "finish_reason": "stop"
            }]
        })),
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
            messages: vec![llm_router_common::protocol::ChatMessage {
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
    let body: Value = response.json().await.expect("valid chat response");
    assert_eq!(body["choices"][0]["message"]["content"], "Hello from stub");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    // Shutdown
    router.stop().await;
    node_stub.stop().await;
}

#[tokio::test]
#[serial]
async fn proxy_chat_uses_health_check_without_skip_flag() {
    let _guard = EnvVarGuard::remove("LLM_ROUTER_SKIP_HEALTH_CHECK");

    let node_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        // OpenAI互換形式のレスポンス
        chat_response: AgentChatStubResponse::Success(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello via health check"
                },
                "finish_reason": "stop"
            }]
        })),
    })
    .await;
    let router = spawn_test_router().await;

    let register_response = register_node(router.addr(), node_stub.addr())
        .await
        .expect("register node request must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::CREATED);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/api/chat", router.addr()))
        .json(&ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![llm_router_common::protocol::ChatMessage {
                role: "user".into(),
                content: "Hello?".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: Value = response.json().await.expect("valid chat response");
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Hello via health check"
    );
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    router.stop().await;
    node_stub.stop().await;
}

#[tokio::test]
#[serial]
async fn proxy_chat_propagates_upstream_error() {
    std::env::set_var("LLM_ROUTER_SKIP_HEALTH_CHECK", "1");
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
            messages: vec![llm_router_common::protocol::ChatMessage {
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
    assert_eq!(body["error"]["type"], "node_upstream_error");
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
