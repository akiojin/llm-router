use super::*;
use crate::token::StreamingTokenAccumulator;

// --- QueueSelection enum ---

#[test]
fn queue_selection_ready_variant_holds_endpoint_and_wait() {
    let ep = Endpoint::new(
        "ep1".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::Xllm,
    );
    let qs = QueueSelection::Ready {
        endpoint: Box::new(ep),
        queued_wait_ms: Some(42),
    };
    let QueueSelection::Ready {
        endpoint,
        queued_wait_ms,
    } = qs;
    assert_eq!(endpoint.name, "ep1");
    assert_eq!(queued_wait_ms, Some(42));
}

#[test]
fn queue_selection_ready_variant_no_wait() {
    let ep = Endpoint::new(
        "ep2".to_string(),
        "http://localhost:8081".to_string(),
        crate::types::endpoint::EndpointType::Ollama,
    );
    let qs = QueueSelection::Ready {
        endpoint: Box::new(ep),
        queued_wait_ms: None,
    };
    let QueueSelection::Ready { queued_wait_ms, .. } = qs;
    assert!(queued_wait_ms.is_none());
}

// --- process_sse_lines ---

#[test]
fn process_sse_lines_empty_chunk() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_single_line_with_newline() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(
        &mut buffer,
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n",
        &mut acc,
    );
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_partial_line_no_newline() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "data: partial", &mut acc);
    assert_eq!(buffer, "data: partial");
}

#[test]
fn process_sse_lines_multiple_lines_in_one_chunk() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "data: line1\ndata: line2\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_split_across_chunks() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");

    // First chunk: partial line
    process_sse_lines(&mut buffer, "data: hel", &mut acc);
    assert_eq!(buffer, "data: hel");

    // Second chunk: rest of line
    process_sse_lines(&mut buffer, "lo\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_carriage_return_stripped() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "data: test\r\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_done_marker() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "data: [DONE]\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_buffer_accumulates_partial() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");

    process_sse_lines(&mut buffer, "abc", &mut acc);
    assert_eq!(buffer, "abc");

    process_sse_lines(&mut buffer, "def", &mut acc);
    assert_eq!(buffer, "abcdef");

    process_sse_lines(&mut buffer, "\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_empty_lines() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(&mut buffer, "\n\n\n", &mut acc);
    assert!(buffer.is_empty());
}

#[test]
fn process_sse_lines_mixed_content() {
    let mut buffer = String::new();
    let mut acc = StreamingTokenAccumulator::new("test-model");
    process_sse_lines(
            &mut buffer,
            "data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}\nremaining",
            &mut acc,
        );
    assert_eq!(buffer, "remaining");
}

// --- forward_to_endpoint URL construction ---

#[test]
fn forward_url_trims_trailing_slash() {
    let ep = Endpoint::new(
        "ep".to_string(),
        "http://localhost:8080/".to_string(),
        crate::types::endpoint::EndpointType::Xllm,
    );
    let url = format!(
        "{}{}",
        ep.base_url.trim_end_matches('/'),
        "/v1/chat/completions"
    );
    assert_eq!(url, "http://localhost:8080/v1/chat/completions");
}

#[test]
fn forward_url_no_trailing_slash() {
    let ep = Endpoint::new(
        "ep".to_string(),
        "http://10.0.0.1:11434".to_string(),
        crate::types::endpoint::EndpointType::Ollama,
    );
    let url = format!("{}{}", ep.base_url.trim_end_matches('/'), "/v1/completions");
    assert_eq!(url, "http://10.0.0.1:11434/v1/completions");
}

#[test]
fn forward_url_multiple_trailing_slashes() {
    let base_url = "http://localhost:8080///";
    let url = format!("{}{}", base_url.trim_end_matches('/'), "/v1/embeddings");
    assert_eq!(url, "http://localhost:8080/v1/embeddings");
}

// --- Endpoint bearer_auth logic ---

#[test]
fn endpoint_with_api_key_has_some() {
    let mut ep = Endpoint::new(
        "ep".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    ep.api_key = Some("sk-test-key".to_string());
    assert!(ep.api_key.is_some());
}

#[test]
fn endpoint_without_api_key_has_none() {
    let ep = Endpoint::new(
        "ep".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    assert!(ep.api_key.is_none());
}

// --- LbError::Http construction from forward_to_endpoint ---

#[test]
fn lb_error_http_format() {
    let err = LbError::Http("Endpoint request failed: connection refused".to_string());
    assert!(err.to_string().contains("Endpoint request failed"));
}

// --- forward_streaming_response content-type behavior ---

#[tokio::test]
async fn forward_streaming_response_sets_json_content_type() {
    // Create a minimal reqwest response
    let response = axum::http::Response::builder()
        .status(200)
        .header("x-custom", "test")
        .body("test body")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(axum_response.status(), StatusCode::OK);
    assert_eq!(
        axum_response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn forward_streaming_response_preserves_sse_content_type() {
    let response = axum::http::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body("data: test\n\n")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(
        axum_response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream"
    );
}

#[tokio::test]
async fn forward_streaming_response_maps_status_code() {
    let response = axum::http::Response::builder()
        .status(201)
        .body("")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(axum_response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn forward_streaming_response_maps_error_status() {
    let response = axum::http::Response::builder()
        .status(500)
        .body("")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(axum_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn forward_streaming_response_preserves_custom_headers() {
    let response = axum::http::Response::builder()
        .status(200)
        .header("x-request-id", "abc123")
        .body("")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(
        axum_response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap(),
        "abc123"
    );
}

#[tokio::test]
async fn forward_streaming_response_maps_404() {
    let response = axum::http::Response::builder()
        .status(404)
        .body("")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(axum_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forward_streaming_response_maps_429() {
    let response = axum::http::Response::builder()
        .status(429)
        .body("")
        .unwrap();
    let reqwest_response = reqwest::Response::from(response);

    let axum_response = forward_streaming_response(reqwest_response).unwrap();
    assert_eq!(axum_response.status(), StatusCode::TOO_MANY_REQUESTS);
}

// --- lease-ttfb: ストリーミング forwarder は配信完了まで lease を保持する ---

#[tokio::test]
async fn streaming_forwarder_holds_lease_until_stream_consumed() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("create test db");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    let registry = crate::registry::endpoints::EndpointRegistry::new(pool)
        .await
        .expect("create registry");
    let endpoint = Endpoint::new(
        "lease-stream-ep".to_string(),
        "http://localhost:1".to_string(),
        crate::types::endpoint::EndpointType::OpenaiCompatible,
    );
    let endpoint_id = endpoint.id;
    registry.add(endpoint).await.expect("add endpoint");
    let load_manager = crate::balancer::LoadManager::new(Arc::new(registry.clone()));
    let event_bus = crate::events::create_shared_event_bus();

    let lease = load_manager
        .begin_request(endpoint_id)
        .await
        .expect("begin request");
    assert_eq!(
        load_manager
            .snapshot(endpoint_id)
            .await
            .unwrap()
            .active_requests,
        1,
        "begin_request must mark one active request"
    );

    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .body("data: {\"choices\":[]}\n\ndata: [DONE]\n\n")
            .unwrap(),
    );

    let axum_response = forward_streaming_response_with_tps_tracking(
        response,
        endpoint_id,
        "m".to_string(),
        None,
        crate::types::endpoint::EndpointType::OpenaiCompatible,
        Instant::now(),
        registry,
        load_manager.clone(),
        event_bus,
        Some(lease),
    )
    .expect("forward streaming");

    // ストリーム未消費の段階では lease は保持されたまま（早期解放しない）
    assert_eq!(
        load_manager
            .snapshot(endpoint_id)
            .await
            .unwrap()
            .active_requests,
        1,
        "lease must be held until the stream is consumed"
    );

    // ボディを完全に消費するとストリーム終端で lease が完了する
    let _ = axum::body::to_bytes(axum_response.into_body(), usize::MAX)
        .await
        .expect("consume body");

    assert_eq!(
        load_manager
            .snapshot(endpoint_id)
            .await
            .unwrap()
            .active_requests,
        0,
        "lease must be completed after the stream is fully consumed"
    );
}

// --- Timeout configuration in forward_to_endpoint ---

#[test]
fn endpoint_inference_timeout_default() {
    let ep = Endpoint::new(
        "ep".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::Xllm,
    );
    assert_eq!(ep.inference_timeout_secs, 600);
}

#[test]
fn endpoint_inference_timeout_custom() {
    let mut ep = Endpoint::new(
        "ep".to_string(),
        "http://localhost:8080".to_string(),
        crate::types::endpoint::EndpointType::Xllm,
    );
    ep.inference_timeout_secs = 60;
    assert_eq!(
        std::time::Duration::from_secs(ep.inference_timeout_secs as u64),
        std::time::Duration::from_secs(60)
    );
}
