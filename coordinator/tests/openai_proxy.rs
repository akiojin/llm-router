use axum::{
    extract::connect_info::ConnectInfo,
    http::{header::CONTENT_TYPE, StatusCode},
};
use ollama_coordinator_common::{
    protocol::{ChatRequest, ChatResponse, GenerateRequest},
    types::GpuDeviceInfo,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager, AppState,
};
use std::net::SocketAddr;
use tower::ServiceExt;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn build_state_with_mock(mock: &MockServer) -> AppState {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = DownloadTaskManager::new();
    let state = AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    };

    // 登録済みエージェントを追加
    state
        .registry
        .register(ollama_coordinator_common::protocol::RegisterRequest {
            machine_name: "mock-agent".into(),
            ip_address: mock.address().ip(),
            ollama_version: "0.0.0".into(),
            // APIポート=ollama_port+1 となる仕様のため、実際のモックポートに合わせて -1 する
            ollama_port: mock.address().port().saturating_sub(1),
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        })
        .await
        .unwrap();

    // エージェントをready状態にしておく（初期化待ちやモデル未ロードで404/503にならないように）
    let agent_id = state.registry.list().await[0].id;

    // レジストリにロード済みモデル・初期化解除を反映
    state
        .registry
        .update_last_seen(
            agent_id,
            Some(vec!["gpt-oss:20b".to_string(), "gpt-oss:120b".to_string()]),
            None,
            None,
            None,
            Some(false),
            Some((4, 4)),
        )
        .await
        .ok();

    state
        .load_manager
        .record_metrics(ollama_coordinator_coordinator::balancer::MetricsUpdate {
            agent_id,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 0,
            average_response_time_ms: Some(1.0),
            initializing: false,
            ready_models: Some((4, 4)),
        })
        .await
        .unwrap();

    state
}

fn attach_test_client_ip<B>(mut request: axum::http::Request<B>) -> axum::http::Request<B> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 54000));
    request.extensions_mut().insert(ConnectInfo(addr));
    request
}

#[tokio::test]
async fn test_proxy_chat_success() {
    let mock_server = MockServer::start().await;

    let chat_response = ChatResponse {
        message: ollama_coordinator_common::protocol::ChatMessage {
            role: "assistant".into(),
            content: "hello".into(),
        },
        done: true,
    };

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&chat_response))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = ChatRequest {
        model: "test-model".into(),
        messages: vec![ollama_coordinator_common::protocol::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        stream: false,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let parsed: ChatResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed.message.content, "hello");
}

#[tokio::test]
async fn test_proxy_chat_streaming_passthrough() {
    let mock_server = MockServer::start().await;
    let sse_payload = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n";

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_bytes(sse_payload.as_bytes().to_vec()),
        )
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = ChatRequest {
        model: "test-model".into(),
        messages: vec![ollama_coordinator_common::protocol::ChatMessage {
            role: "user".into(),
            content: "stream?".into(),
        }],
        stream: true,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), sse_payload);
}

#[tokio::test]
async fn test_proxy_chat_missing_model_returns_openai_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(404).set_body_string("model not found"))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = ChatRequest {
        model: "missing".into(),
        messages: vec![ollama_coordinator_common::protocol::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        stream: false,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["type"], "ollama_upstream_error");
    assert_eq!(json["error"]["code"], 404);
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("model not found"));
}

#[tokio::test]
async fn test_proxy_chat_no_agents() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = DownloadTaskManager::new();
    let router = api::create_router(AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    });

    let payload = ChatRequest {
        model: "test-model".into(),
        messages: vec![ollama_coordinator_common::protocol::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
        stream: false,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_proxy_generate_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "response": "ok"
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = GenerateRequest {
        model: "test-model".into(),
        prompt: "hello".into(),
        stream: false,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_proxy_generate_streaming_passthrough() {
    let mock_server = MockServer::start().await;
    let ndjson_payload = "{\"response\":\"chunk-1\"}\n{\"response\":\"chunk-2\"}\n";

    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/x-ndjson")
                .set_body_bytes(ndjson_payload.as_bytes().to_vec()),
        )
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = GenerateRequest {
        model: "test-model".into(),
        prompt: "stream please".into(),
        stream: true,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-ndjson"
    );
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), ndjson_payload);
}

#[tokio::test]
async fn test_proxy_generate_no_agents() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let request_history = std::sync::Arc::new(
        ollama_coordinator_coordinator::db::request_history::RequestHistoryStorage::new().unwrap(),
    );
    let task_manager = DownloadTaskManager::new();
    let router = api::create_router(AppState {
        registry,
        load_manager,
        request_history,
        task_manager,
    });

    let payload = GenerateRequest {
        model: "test-model".into(),
        prompt: "hello".into(),
        stream: false,
    };

    let response = router
        .oneshot(attach_test_client_ip(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_openai_chat_completions_success() {
    let mock_server = MockServer::start().await;

    let upstream = serde_json::json!({
        "object": "chat.completion",
        "choices": [{
            "message": { "role": "assistant", "content": "Hello from OpenAI route" },
            "finish_reason": "stop",
            "index": 0
        }]
    });

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(upstream))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-model",
        "messages": [
            { "role": "user", "content": "hi?" }
        ],
        "stream": false
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["object"], "chat.completion");
    assert_eq!(
        json["choices"][0]["message"]["content"],
        "Hello from OpenAI route"
    );
}

#[tokio::test]
async fn test_openai_chat_completions_streaming_passthrough() {
    let mock_server = MockServer::start().await;
    let sse_payload = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello stream\"}}]}\n\n";

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_bytes(sse_payload.as_bytes().to_vec()),
        )
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-model",
        "messages": [
            { "role": "user", "content": "stream?" }
        ],
        "stream": true
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), sse_payload);
}

#[tokio::test]
async fn test_openai_completions_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "response": "Generated via OpenAI route",
            "done": true
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-model",
        "prompt": "say hello",
        "stream": false
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/completions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["response"], "Generated via OpenAI route");
}

#[tokio::test]
async fn test_openai_completions_streaming_passthrough() {
    let mock_server = MockServer::start().await;
    let ndjson_payload = "{\"response\":\"chunk-1\"}\n{\"response\":\"chunk-2\"}\n";

    Mock::given(method("POST"))
        .and(path("/v1/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/x-ndjson")
                .set_body_bytes(ndjson_payload.as_bytes().to_vec()),
        )
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-model",
        "prompt": "stream please",
        "stream": true
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/completions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/x-ndjson"
    );
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), ndjson_payload);
}

#[tokio::test]
async fn test_openai_chat_completions_preserves_extra_fields() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "temperature": 0.5,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "hi"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
                    ]
                }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "choices": [{
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop",
                "index": 0
            }]
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-model",
        "temperature": 0.5,
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "hi" },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            }
        ]
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_openai_embeddings_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "index": 0,
                "object": "embedding",
                "embedding": [0.1, 0.2, 0.3]
            }],
            "model": "test-embed",
            "object": "list"
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let payload = serde_json::json!({
        "model": "test-embed",
        "input": "embed me"
    });

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/embeddings")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["object"], "list");
}

#[tokio::test]
async fn test_openai_models_list_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "list",
            "data": [{
                "id": "gpt-oss:20b",
                "object": "model",
                "owned_by": "ollama"
            }]
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/v1/models")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["data"][0]["id"], "gpt-oss:20b");
}

#[tokio::test]
async fn test_openai_model_detail_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/models/gpt-oss:20b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "gpt-oss:20b",
            "object": "model",
            "owned_by": "ollama"
        })))
        .mount(&mock_server)
        .await;

    let state = build_state_with_mock(&mock_server).await;
    let router = api::create_router(state);

    let response = router
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/v1/models/gpt-oss:20b")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
