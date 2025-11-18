//! Contract Test: OpenAI /v1/completions proxy

use std::sync::Arc;

use crate::support::{
    coordinator::{register_agent, spawn_coordinator},
    http::{spawn_router, TestServer},
};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
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
        .route("/api/generate", post(agent_generate_handler))
        .route("/v1/completions", post(agent_generate_handler))
        .route("/v1/chat/completions", post(agent_generate_handler))
        .route("/v1/models", get(agent_models_handler))
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

#[tokio::test]
async fn proxy_completions_end_to_end_success() {
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
    let coordinator = spawn_coordinator().await;

    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::OK);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/v1/completions", coordinator.addr()))
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
async fn proxy_completions_propagates_upstream_error() {
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
    assert_eq!(register_response.status(), ReqStatusCode::OK);

    let client = Client::new();
    let response = client
        .post(format!("http://{}/v1/completions", coordinator.addr()))
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
async fn proxy_completions_queue_overflow_returns_503() {
    use futures::future::join_all;
    use std::time::Duration;
    use tokio::time::sleep;

    // Agent stub will answer once it starts receiving traffic
    let agent_stub = spawn_agent_stub(AgentStubState {
        expected_model: Some("gpt-oss:20b".to_string()),
        response: AgentGenerateStubResponse::Success(serde_json::json!({
            "id": "cmpl-ready",
            "object": "text_completion",
            "choices": [
                {"text": "ok", "index": 0, "logprobs": null, "finish_reason": "stop"}
            ]
        })),
    })
    .await;

    let coordinator = spawn_coordinator().await;

    let register_response = register_agent(coordinator.addr(), agent_stub.addr())
        .await
        .expect("register agent must succeed");
    assert_eq!(register_response.status(), ReqStatusCode::OK);
    let register_body: serde_json::Value = register_response
        .json()
        .await
        .expect("register body must be JSON");
    let agent_id = register_body["agent_id"]
        .as_str()
        .expect("agent_id present")
        .to_string();

    // 事前にヘルスチェックを送り、LoadManager側に「初期化中」状態を作る
    let bootstrap_health = Client::new()
        .post(format!("http://{}/api/health", coordinator.addr()))
        .json(&serde_json::json!({
            "agent_id": agent_id,
            "cpu_usage": 0.1,
            "memory_usage": 0.1,
            "gpu_usage": null,
            "gpu_memory_usage": null,
            "gpu_memory_total_mb": null,
            "gpu_memory_used_mb": null,
            "gpu_temperature": null,
            "gpu_model_name": null,
            "gpu_compute_capability": null,
            "gpu_capability_score": null,
            "active_requests": 0,
            "average_response_time_ms": null,
            "loaded_models": [],
            "initializing": true,
            "ready_models": [0, 5]
        }))
        .send()
        .await
        .expect("bootstrap health must send");
    assert_eq!(bootstrap_health.status(), ReqStatusCode::OK);

    // MAX_WAITERS を小さくオーバーライドしてテストを高速化
    std::env::set_var("COORDINATOR_MAX_WAITERS", "2");

    // Fire 3 concurrent requests while the only agent is still initializing.
    // One request should overflow the MAX_WAITERS queue and return 503.
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client builds");
    let url = format!("http://{}/v1/completions", coordinator.addr());
    let payload = serde_json::json!({
        "model": "gpt-oss:20b",
        "prompt": "ping",
        "max_tokens": 4
    });

    // After a short delay, send a health check to mark the agent ready so queued
    // requests can drain.
    let health_client = client.clone();
    let agent_id_clone = agent_id.clone();
    let coordinator_addr = coordinator.addr();
    let health_task = tokio::spawn(async move {
        sleep(Duration::from_millis(50)).await;
        health_client
            .post(format!("http://{}/api/health", coordinator_addr))
            .json(&serde_json::json!({
                "agent_id": agent_id_clone,
                "cpu_usage": 1.0,
                "memory_usage": 1.0,
                "gpu_usage": null,
                "gpu_memory_usage": null,
                "gpu_memory_total_mb": null,
                "gpu_memory_used_mb": null,
                "gpu_temperature": null,
                "gpu_model_name": null,
                "gpu_compute_capability": null,
                "gpu_capability_score": null,
                "active_requests": 0,
                "average_response_time_ms": 1.0,
                "loaded_models": ["gpt-oss:20b"],
                "initializing": false,
                "ready_models": [1, 5]
            }))
            .send()
            .await
            .expect("health update send")
            .error_for_status()
            .expect("health update must succeed");
    });

    let total_requests = 3usize; // MAX_WAITERS(2) + 1
    let responses = join_all((0..total_requests).map(|_| {
        let client = client.clone();
        let url = url.clone();
        let payload = payload.clone();
        async move {
            match client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    (Ok(status), body)
                }
                Err(err) => (Err(err), String::new()),
            }
        }
    }))
    .await;

    health_task
        .await
        .expect("health update task should complete");

    let mut ok = 0;
    let mut svc_unavailable = 0;
    let mut unavailable_bodies = Vec::new();
    let mut unexpected = Vec::new();
    for (status_res, body) in responses {
        match status_res {
            Ok(status) if status == ReqStatusCode::OK => {
                ok += 1;
            }
            Ok(status) if status == ReqStatusCode::SERVICE_UNAVAILABLE => {
                svc_unavailable += 1;
                unavailable_bodies.push(body);
            }
            Ok(status) => unexpected.push(format!("{}: {}", status, body)),
            Err(err) => unexpected.push(format!("reqwest-error: {err}")),
        }
    }

    assert!(
        unexpected.is_empty(),
        "unexpected responses: {unexpected:?}"
    );
    assert_eq!(
        svc_unavailable + ok,
        total_requests,
        "response count mismatch"
    );
    assert!(
        svc_unavailable >= 1,
        "at least one request should be rejected when queue is full"
    );
    assert!(
        unavailable_bodies
            .iter()
            .all(|b| b.contains("warming up") || b.contains("Service Unavailable")),
        "503 responses should indicate warm-up/queue overflow: {unavailable_bodies:?}"
    );
}
