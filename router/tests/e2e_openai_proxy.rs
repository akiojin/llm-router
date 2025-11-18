//! E2E: 実際にHTTP経由でルーターとスタブノードを起動し、
//! OpenAI互換APIのリクエスト・エラー・ストリーミングを検証する。

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use ollama_router_common::protocol::{ChatRequest, ChatResponse, GenerateRequest};
use reqwest::{header, Client};
use serde_json::{json, Value};
use tokio::time::sleep;

#[path = "support/mod.rs"]
mod support;

use support::{
    http::{spawn_router, TestServer},
    router::{register_node, spawn_test_router},
};

#[derive(Clone)]
struct AgentStubState {
    chat_response: ChatResponse,
    chat_stream_payload: String,
    generate_response: Value,
    generate_stream_payload: String,
}

async fn spawn_agent_stub(state: AgentStubState) -> TestServer {
    let shared_state = Arc::new(state);
    let router = Router::new()
        .route("/v1/chat/completions", post(agent_chat_handler))
        .route("/v1/completions", post(agent_generate_handler))
        .route("/v1/models", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"data": [{"id": "gpt-oss:20b"}], "object": "list"}))
        }))
        .route("/api/tags", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"models": [{"name": "gpt-oss:20b", "size": 10000000000i64}]}))
        }))
        .with_state(shared_state);

    spawn_router(router).await
}

async fn agent_chat_handler(
    State(state): State<Arc<AgentStubState>>,
    Json(request): Json<ChatRequest>,
) -> Response {
    if request.model == "missing-model" {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("model not found"))
            .unwrap();
    }

    if request.stream {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/event-stream")
            .body(Body::from(state.chat_stream_payload.clone()))
            .unwrap();
    }

    Json(state.chat_response.clone()).into_response()
}

async fn agent_generate_handler(
    State(state): State<Arc<AgentStubState>>,
    Json(request): Json<GenerateRequest>,
) -> Response {
    if request.model == "missing-model" {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("model not loaded"))
            .unwrap();
    }

    if request.stream {
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/x-ndjson")
            .body(Body::from(state.generate_stream_payload.clone()))
            .unwrap();
    }

    Json(state.generate_response.clone()).into_response()
}

#[tokio::test]
async fn openai_proxy_end_to_end_updates_dashboard_history() {
    std::env::set_var("OLLAMA_ROUTER_SKIP_HEALTH_CHECK", "1");
    let node_stub = spawn_agent_stub(AgentStubState {
        chat_response: ChatResponse {
            message: ollama_router_common::protocol::ChatMessage {
                role: "assistant".into(),
                content: "Hello from agent".into(),
            },
            done: true,
        },
        chat_stream_payload: "data: {\"choices\":[{\"delta\":{\"content\":\"Hello stream\"}}]}\n\n"
            .to_string(),
        generate_response: json!({
            "response": "generated text",
            "done": true
        }),
        generate_stream_payload: "{\"response\":\"chunk-1\"}\n{\"response\":\"chunk-2\"}\n"
            .to_string(),
    })
    .await;

    let router = spawn_test_router().await;

    register_node(router.addr(), node_stub.addr())
        .await
        .expect("agent registration should succeed");

    let client = Client::new();

    // 正常系チャット
    let chat_response = client
        .post(format!("http://{}/api/chat", router.addr()))
        .json(&ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![ollama_router_common::protocol::ChatMessage {
                role: "user".into(),
                content: "hello?".into(),
            }],
            stream: false,
        })
        .send()
        .await
        .expect("chat request should succeed");
    assert_eq!(chat_response.status(), reqwest::StatusCode::OK);
    let chat_payload: ChatResponse = chat_response.json().await.expect("chat json response");
    assert_eq!(chat_payload.message.content, "Hello from agent");

    // ストリーミングチャット
    let streaming_response = client
        .post(format!("http://{}/api/chat", router.addr()))
        .json(&ChatRequest {
            model: "gpt-oss:20b".into(),
            messages: vec![ollama_router_common::protocol::ChatMessage {
                role: "user".into(),
                content: "stream?".into(),
            }],
            stream: true,
        })
        .send()
        .await
        .expect("streaming chat request should succeed");
    assert_eq!(streaming_response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        streaming_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok()),
        Some("text/event-stream")
    );
    let streaming_body = streaming_response
        .text()
        .await
        .expect("streaming chat body");
    assert!(
        streaming_body.contains("Hello stream"),
        "expected streaming payload to contain agent content"
    );

    // 生成API正常系
    let generate_response = client
        .post(format!("http://{}/api/generate", router.addr()))
        .json(&GenerateRequest {
            model: "gpt-oss:20b".into(),
            prompt: "write something".into(),
            stream: false,
        })
        .send()
        .await
        .expect("generate request should succeed");
    assert_eq!(generate_response.status(), reqwest::StatusCode::OK);
    let generate_payload: Value = generate_response
        .json()
        .await
        .expect("generate json response");
    assert_eq!(generate_payload["response"], "generated text");

    // 生成APIエラーケース
    let missing_model_response = client
        .post(format!("http://{}/api/generate", router.addr()))
        .json(&GenerateRequest {
            model: "missing-model".into(),
            prompt: "fail please".into(),
            stream: false,
        })
        .send()
        .await
        .expect("missing model request should respond");
    assert_eq!(
        missing_model_response.status(),
        reqwest::StatusCode::BAD_REQUEST
    );

    // 集計が反映されるまで僅かに待機
    sleep(Duration::from_millis(100)).await;

    let history = client
        .get(format!(
            "http://{}/api/dashboard/request-history",
            router.addr()
        ))
        .send()
        .await
        .expect("request history endpoint should respond")
        .json::<Value>()
        .await
        .expect("history payload should be valid JSON");

    assert!(
        history.is_array(),
        "request history payload should be an array"
    );
    let entries = history.as_array().unwrap();
    assert_eq!(
        entries.len(),
        60,
        "request history maintains a fixed window of entries"
    );
    let latest = entries
        .last()
        .expect("history should contain at least one entry");
    let success = latest["success"].as_u64().unwrap_or_default();
    let error = latest["error"].as_u64().unwrap_or_default();
    assert!(
        success >= 3,
        "expected at least three successful requests recorded, got {success}"
    );
    assert!(
        error >= 1,
        "expected at least one failed request recorded, got {error}"
    );

    router.stop().await;
    node_stub.stop().await;
}
