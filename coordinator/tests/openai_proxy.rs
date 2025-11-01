use axum::http::StatusCode;
use ollama_coordinator_common::{
    protocol::{ChatRequest, ChatResponse, GenerateRequest},
    types::GpuDeviceInfo,
};
use ollama_coordinator_coordinator::{
    api, balancer::LoadManager, registry::AgentRegistry, AppState,
};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn build_state_with_mock(mock: &MockServer) -> AppState {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let state = AppState {
        registry,
        load_manager,
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
async fn test_proxy_chat_no_agents() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let router = api::create_router(AppState {
        registry,
        load_manager,
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
async fn test_proxy_generate_no_agents() {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let router = api::create_router(AppState {
        registry,
        load_manager,
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
