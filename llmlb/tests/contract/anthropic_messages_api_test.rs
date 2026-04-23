//! Contract tests for the Anthropic-native Messages API (`/v1/messages`).

use crate::support::{
    http::TestServer,
    lb::{register_responses_endpoint, spawn_test_lb, spawn_test_lb_with_db},
};
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use llmlb::common::protocol::{RecordStatus, RequestResponseRecord};
use llmlb::db::request_history::RequestHistoryStorage;
use reqwest::{Client, StatusCode as ReqStatusCode};
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Clone)]
struct ChatNodeStubState {
    response: ChatStubResponse,
    /// /v1/models で広告するモデル ID。エンドポイント同期でこの値が登録される。
    advertised_model: String,
    /// llmlb のエンドポイント型自動検出に「どの runtime として振る舞うか」を指定する。
    detection_kind: DetectionKind,
    /// /v1/chat/completions に届いた最新の POST body を捕捉する（`/v1/messages` の
    /// モデル書き換えを検証するために使用）。
    captured_request: Arc<Mutex<Option<Value>>>,
}

#[derive(Clone, Copy)]
enum DetectionKind {
    OpenaiCompatible,
    Ollama,
    LmStudio,
}

#[derive(Clone)]
enum ChatStubResponse {
    Json(Value),
    Stream(String),
    Raw {
        status: StatusCode,
        body: String,
        content_type: &'static str,
        delay_ms: u64,
    },
}

fn chat_node_stub_openai_compat(response: ChatStubResponse) -> ChatNodeStubState {
    ChatNodeStubState {
        response,
        advertised_model: "test-model".to_string(),
        detection_kind: DetectionKind::OpenaiCompatible,
        captured_request: Arc::new(Mutex::new(None)),
    }
}

async fn spawn_chat_node_stub(state: ChatNodeStubState) -> (TestServer, Arc<Mutex<Option<Value>>>) {
    let captured = state.captured_request.clone();
    let detection = state.detection_kind;
    let mut app = Router::new()
        .route("/v1/chat/completions", post(chat_handler))
        .route("/v1/models", get(models_handler));
    if matches!(detection, DetectionKind::Ollama) {
        app = app.route("/api/tags", get(ollama_tags_handler));
    }
    let server = crate::support::http::spawn_lb(app.with_state(Arc::new(state))).await;
    (server, captured)
}

async fn chat_handler(
    State(state): State<Arc<ChatNodeStubState>>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    {
        let mut guard = state
            .captured_request
            .lock()
            .expect("captured_request lock should not be poisoned");
        *guard = Some(request);
    }
    match &state.response {
        ChatStubResponse::Json(payload) => (StatusCode::OK, Json(payload.clone())).into_response(),
        ChatStubResponse::Stream(body) => axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from(body.clone()))
            .expect("stream response should build"),
        ChatStubResponse::Raw {
            status,
            body,
            content_type,
            delay_ms,
        } => {
            if *delay_ms > 0 {
                sleep(Duration::from_millis(*delay_ms)).await;
            }
            axum::response::Response::builder()
                .status(*status)
                .header("content-type", *content_type)
                .body(Body::from(body.clone()))
                .expect("raw response should build")
        }
    }
}

async fn models_handler(State(state): State<Arc<ChatNodeStubState>>) -> impl IntoResponse {
    let owned_by = match state.detection_kind {
        DetectionKind::LmStudio => "lm-studio",
        DetectionKind::Ollama => "ollama",
        DetectionKind::OpenaiCompatible => "test",
    };
    (
        StatusCode::OK,
        Json(json!({
            "object": "list",
            "data": [
                {"id": state.advertised_model, "object": "model", "owned_by": owned_by}
            ]
        })),
    )
}

async fn ollama_tags_handler(State(state): State<Arc<ChatNodeStubState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "models": [
                {"name": state.advertised_model, "size": 12_000_000_000i64}
            ]
        })),
    )
}

async fn load_request_history_from_db(db_pool: &SqlitePool) -> Vec<RequestResponseRecord> {
    let storage = Arc::new(RequestHistoryStorage::new(db_pool.clone()));
    storage.load_records().await.unwrap_or_default()
}

async fn wait_for_latest_error_message(db_pool: &SqlitePool, model: &str) -> String {
    for _ in 0..20 {
        let records = load_request_history_from_db(db_pool).await;
        if let Some(message) = records
            .iter()
            .rev()
            .find(|record| record.model == model)
            .and_then(|record| match &record.status {
                RecordStatus::Error { message } => Some(message.clone()),
                RecordStatus::Success => None,
            })
        {
            return message;
        }
        sleep(Duration::from_millis(100)).await;
    }

    panic!("timed out waiting for request history error record for model {model}");
}

async fn register_chat_endpoint_with_timeout(
    lb_addr: std::net::SocketAddr,
    node_addr: std::net::SocketAddr,
    name: &str,
    inference_timeout_secs: u32,
) -> String {
    let client = Client::new();

    let create_response = client
        .post(format!("http://{}/api/endpoints", lb_addr))
        .header("authorization", "Bearer sk_debug")
        .json(&json!({
            "name": format!("{name} - {node_addr}"),
            "base_url": format!("http://{}", node_addr),
            "health_check_interval_secs": 30,
            "inference_timeout_secs": inference_timeout_secs,
        }))
        .send()
        .await
        .expect("endpoint registration must succeed");
    assert_eq!(create_response.status(), ReqStatusCode::CREATED);

    let create_body: Value = create_response
        .json()
        .await
        .expect("endpoint registration response must be json");
    let endpoint_id = create_body["id"]
        .as_str()
        .expect("endpoint id must be present")
        .to_string();

    let test_response = client
        .post(format!(
            "http://{}/api/endpoints/{}/test",
            lb_addr, endpoint_id
        ))
        .header("authorization", "Bearer sk_debug")
        .send()
        .await
        .expect("endpoint test must succeed");
    assert_eq!(test_response.status(), ReqStatusCode::OK);

    let sync_response = client
        .post(format!(
            "http://{}/api/endpoints/{}/sync",
            lb_addr, endpoint_id
        ))
        .header("authorization", "Bearer sk_debug")
        .send()
        .await
        .expect("endpoint sync must succeed");
    assert_eq!(sync_response.status(), ReqStatusCode::OK);

    endpoint_id
}

#[tokio::test]
#[serial]
async fn anthropic_messages_local_request_success() {
    let (node, _captured) = spawn_chat_node_stub(chat_node_stub_openai_compat(
        ChatStubResponse::Json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "test-model",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Hello from local endpoint"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 8,
                "completion_tokens": 5,
                "total_tokens": 13
            }
        })),
    ))
    .await;
    let lb = spawn_test_lb().await;
    let _ = register_responses_endpoint(lb.addr(), node.addr(), "test-model")
        .await
        .expect("endpoint registration should succeed");

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "test-model",
            "max_tokens": 128,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: Value = response.json().await.expect("response must be json");
    assert_eq!(body["type"], "message");
    assert_eq!(body["role"], "assistant");
    assert_eq!(body["content"][0]["text"], "Hello from local endpoint");
    assert_eq!(body["usage"]["input_tokens"], 8);
    assert_eq!(body["usage"]["output_tokens"], 5);
}

#[tokio::test]
#[serial]
async fn anthropic_messages_streaming_transforms_openai_sse() {
    let (node, _captured) = spawn_chat_node_stub(chat_node_stub_openai_compat(
        ChatStubResponse::Stream(
            concat!(
                "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n",
                "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"index\":0}]}\n\n",
                "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\" world\"},\"index\":0}]}\n\n",
                "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\",\"index\":0}]}\n\n",
                "data: [DONE]\n\n"
            )
            .to_string(),
        ),
    ))
    .await;
    let lb = spawn_test_lb().await;
    let _ = register_responses_endpoint(lb.addr(), node.addr(), "test-model")
        .await
        .expect("endpoint registration should succeed");

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "test-model",
            "max_tokens": 128,
            "stream": true,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );

    let body = response
        .text()
        .await
        .expect("stream body should be readable");
    assert!(body.contains("event: message_start"));
    assert!(body.contains("event: content_block_delta"));
    assert!(body.contains("\"text\":\"Hello\""));
    assert!(body.contains("\"text\":\" world\""));
    assert!(body.contains("event: message_stop"));
}

#[tokio::test]
#[serial]
async fn anthropic_messages_cloud_prefix_passthrough() {
    let upstream = MockServer::start().await;
    std::env::set_var("ANTHROPIC_API_KEY", "anthropic-test-key");
    std::env::set_var("ANTHROPIC_API_BASE_URL", upstream.uri());

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "anthropic-test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-7-sonnet",
            "content": [{"type": "text", "text": "Hello from Anthropic Cloud"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 12, "output_tokens": 7}
        })))
        .mount(&upstream)
        .await;

    let lb = spawn_test_lb().await;
    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "anthropic:claude-3-7-sonnet",
            "max_tokens": 128,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should succeed");

    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("ANTHROPIC_API_BASE_URL");

    assert_eq!(response.status(), ReqStatusCode::OK);
    let body: Value = response.json().await.expect("response must be json");
    assert_eq!(body["model"], "claude-3-7-sonnet");
    assert_eq!(body["content"][0]["text"], "Hello from Anthropic Cloud");
}

#[tokio::test]
#[serial]
async fn anthropic_messages_requires_anthropic_version_header() {
    let lb = spawn_test_lb().await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .json(&json!({
            "model": "test-model",
            "max_tokens": 128,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
#[serial]
async fn anthropic_messages_invalid_api_key_uses_anthropic_error_shape() {
    let lb = spawn_test_lb().await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "invalid-key")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "test-model",
            "max_tokens": 128,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::UNAUTHORIZED);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "authentication_error");
}

/// Claude Code 相当のクライアントが `openai/gpt-oss-20b` で `/v1/messages` を叩いた際、
/// Ollama バックエンドが `gpt-oss:20b` としてしかモデルを保持していないケース。
/// llmlb は `rewrite_payload_model_for_endpoint` を呼んで upstream へは
/// エイリアス名（`gpt-oss:20b`）で転送しなければ 502 "not found" になる。
#[tokio::test]
#[serial]
async fn anthropic_messages_rewrites_canonical_to_ollama_alias() {
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Json(json!({
        "id": "chatcmpl-rewrite",
        "object": "chat.completion",
        "model": "gpt-oss:20b",
        "choices": [
            {
                "message": {"role": "assistant", "content": "Hello from Ollama"},
                "finish_reason": "stop"
            }
        ],
        "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
    })));
    state.advertised_model = "gpt-oss:20b".to_string();
    state.detection_kind = DetectionKind::Ollama;

    let (node, captured) = spawn_chat_node_stub(state).await;
    let lb = spawn_test_lb().await;
    let _ = register_responses_endpoint(lb.addr(), node.addr(), "gpt-oss:20b")
        .await
        .expect("endpoint registration should succeed");

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "openai/gpt-oss-20b",
            "max_tokens": 32,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);

    let captured_body = captured
        .lock()
        .expect("captured_request lock should not be poisoned")
        .clone()
        .expect("upstream should have received at least one POST");
    assert_eq!(
        captured_body["model"], "gpt-oss:20b",
        "Ollama の upstream へは canonical 名ではなくエイリアス名で転送されること"
    );
}

/// LM Studio のように canonical 名（`openai/gpt-oss-20b`）を
/// そのまま広告するエンドポイントでは、書き換えが発生せず
/// リクエストがそのまま upstream に届くこと（no-op パスの保証）。
#[tokio::test]
#[serial]
async fn anthropic_messages_passes_canonical_through_lm_studio() {
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Json(json!({
        "id": "chatcmpl-passthrough",
        "object": "chat.completion",
        "model": "openai/gpt-oss-20b",
        "choices": [
            {
                "message": {"role": "assistant", "content": "Hello from LM Studio"},
                "finish_reason": "stop"
            }
        ],
        "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
    })));
    state.advertised_model = "openai/gpt-oss-20b".to_string();
    state.detection_kind = DetectionKind::LmStudio;

    let (node, captured) = spawn_chat_node_stub(state).await;
    let lb = spawn_test_lb().await;
    let _ = register_responses_endpoint(lb.addr(), node.addr(), "openai/gpt-oss-20b")
        .await
        .expect("endpoint registration should succeed");

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "openai/gpt-oss-20b",
            "max_tokens": 32,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), ReqStatusCode::OK);

    let captured_body = captured
        .lock()
        .expect("captured_request lock should not be poisoned")
        .clone()
        .expect("upstream should have received at least one POST");
    assert_eq!(
        captured_body["model"], "openai/gpt-oss-20b",
        "canonical 名を直接受理するエンドポイントでは書き換えが発生しないこと"
    );
}

#[tokio::test]
#[serial]
async fn anthropic_messages_timeout_surfaces_detailed_upstream_error_and_history() {
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Raw {
        status: StatusCode::OK,
        body: json!({
            "id": "chatcmpl-timeout",
            "object": "chat.completion",
            "model": "timeout-model",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "too late"},
                    "finish_reason": "stop"
                }
            ]
        })
        .to_string(),
        content_type: "application/json",
        delay_ms: 1_200,
    });
    state.advertised_model = "timeout-model".to_string();

    let (node, _captured) = spawn_chat_node_stub(state).await;
    let (lb, db_pool) = spawn_test_lb_with_db().await;
    let _endpoint_id =
        register_chat_endpoint_with_timeout(lb.addr(), node.addr(), "timeout-endpoint", 1).await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "timeout-model",
            "max_tokens": 32,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::GATEWAY_TIMEOUT);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["error"]["type"], "api_error");
    let message = body["error"]["message"]
        .as_str()
        .expect("error message should be string");
    assert!(
        message.contains("timed out after 1 seconds"),
        "timeout detail should be exposed, got: {message}"
    );

    let history_message = wait_for_latest_error_message(&db_pool, "timeout-model").await;
    assert_eq!(history_message, message);
}

#[tokio::test]
#[serial]
async fn anthropic_messages_non_streaming_404_preserves_raw_upstream_body() {
    let raw_body = json!({
        "error": {"message": "model missing", "type": "not_found_error"},
        "upstream": "lm-studio"
    })
    .to_string();
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Raw {
        status: StatusCode::NOT_FOUND,
        body: raw_body.clone(),
        content_type: "application/json",
        delay_ms: 0,
    });
    state.advertised_model = "missing-upstream-model".to_string();

    let (node, _captured) = spawn_chat_node_stub(state).await;
    let (lb, db_pool) = spawn_test_lb_with_db().await;
    let _endpoint_id = register_chat_endpoint_with_timeout(
        lb.addr(),
        node.addr(),
        "missing-upstream-endpoint",
        30,
    )
    .await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "missing-upstream-model",
            "max_tokens": 32,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["error"]["type"], "not_found_error");
    assert_eq!(body["error"]["message"], raw_body);

    let history_message = wait_for_latest_error_message(&db_pool, "missing-upstream-model").await;
    assert_eq!(history_message, raw_body);
}

#[tokio::test]
#[serial]
async fn anthropic_messages_streaming_404_preserves_raw_upstream_body() {
    let raw_body = json!({
        "error": {"message": "model missing", "type": "not_found_error"},
        "upstream": "streaming-endpoint"
    })
    .to_string();
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Raw {
        status: StatusCode::NOT_FOUND,
        body: raw_body.clone(),
        content_type: "application/json",
        delay_ms: 0,
    });
    state.advertised_model = "stream-missing-model".to_string();

    let (node, _captured) = spawn_chat_node_stub(state).await;
    let (lb, db_pool) = spawn_test_lb_with_db().await;
    let _endpoint_id =
        register_chat_endpoint_with_timeout(lb.addr(), node.addr(), "stream-error-endpoint", 30)
            .await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "stream-missing-model",
            "max_tokens": 32,
            "stream": true,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["error"]["type"], "not_found_error");
    assert_eq!(body["error"]["message"], raw_body);

    let history_message = wait_for_latest_error_message(&db_pool, "stream-missing-model").await;
    assert_eq!(history_message, raw_body);
}

#[tokio::test]
#[serial]
async fn anthropic_messages_streaming_500_without_body_uses_status_text() {
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Raw {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: String::new(),
        content_type: "application/json",
        delay_ms: 0,
    });
    state.advertised_model = "stream-empty-body-model".to_string();

    let (node, _captured) = spawn_chat_node_stub(state).await;
    let (lb, db_pool) = spawn_test_lb_with_db().await;
    let _endpoint_id =
        register_chat_endpoint_with_timeout(lb.addr(), node.addr(), "stream-empty-body", 30).await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "stream-empty-body-model",
            "max_tokens": 32,
            "stream": true,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::BAD_GATEWAY);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["error"]["type"], "api_error");
    assert_eq!(body["error"]["message"], "500 Internal Server Error");

    let history_message = wait_for_latest_error_message(&db_pool, "stream-empty-body-model").await;
    assert_eq!(history_message, "500 Internal Server Error");
}

#[tokio::test]
#[serial]
async fn anthropic_messages_parse_failure_surfaces_parser_detail() {
    let mut state = chat_node_stub_openai_compat(ChatStubResponse::Raw {
        status: StatusCode::OK,
        body: "not-json".to_string(),
        content_type: "text/plain",
        delay_ms: 0,
    });
    state.advertised_model = "parse-failure-model".to_string();

    let (node, _captured) = spawn_chat_node_stub(state).await;
    let (lb, db_pool) = spawn_test_lb_with_db().await;
    let _endpoint_id =
        register_chat_endpoint_with_timeout(lb.addr(), node.addr(), "parse-failure-endpoint", 30)
            .await;

    let response = Client::new()
        .post(format!("http://{}/v1/messages", lb.addr()))
        .header("x-api-key", "sk_debug")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "parse-failure-model",
            "max_tokens": 32,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .send()
        .await
        .expect("request should complete");

    assert_eq!(response.status(), ReqStatusCode::BAD_GATEWAY);
    let body: Value = response.json().await.expect("error body must be json");
    assert_eq!(body["error"]["type"], "api_error");
    let message = body["error"]["message"]
        .as_str()
        .expect("error message should be string");
    assert!(
        message.contains("Failed to parse OpenAI-compatible upstream response:"),
        "parse detail should be exposed, got: {message}"
    );

    let history_message = wait_for_latest_error_message(&db_pool, "parse-failure-model").await;
    assert_eq!(history_message, message);
}
