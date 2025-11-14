use axum::http::{header::CONTENT_TYPE, StatusCode};
use ollama_coordinator_common::{
    protocol::{ChatRequest, ChatResponse, GenerateRequest},
    types::GpuDeviceInfo,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, tasks::DownloadTaskManager, AppState,
};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
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
            ollama_port: mock.address().port(),
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

    state
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
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
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/generate")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&payload).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
