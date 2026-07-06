use super::post_responses;
use crate::{
    common::protocol::{RecordStatus, RequestType},
    db::test_utils::TestAppStateBuilder,
    types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI},
    AppState,
};
use axum::{body::to_bytes, extract::State, http::StatusCode, Json};
use serde_json::json;
use tokio::time::{sleep, Duration};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn create_local_state() -> AppState {
    TestAppStateBuilder::new().await.build().await
}

async fn register_vllm_endpoint(state: &AppState, base_url: String, model_id: &str) -> uuid::Uuid {
    let mut endpoint = Endpoint::new(
        "responses-test-endpoint".to_string(),
        base_url,
        EndpointType::Vllm,
    );
    endpoint.status = EndpointStatus::Online;
    let endpoint_id = endpoint.id;
    state
        .endpoint_registry
        .add(endpoint)
        .await
        .expect("add endpoint");
    state
        .endpoint_registry
        .add_model(&EndpointModel {
            endpoint_id,
            model_id: model_id.to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::Responses],
            canonical_name: None,
        })
        .await
        .expect("add endpoint model");
    endpoint_id
}

#[tokio::test]
async fn responses_non_stream_success_updates_model_tps() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    let response_body = json!({
        "id": "resp_123",
        "object": "response",
        "usage": {
            "input_tokens": 8,
            "output_tokens": 12,
            "total_tokens": 20
        }
    });
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&server)
        .await;

    let endpoint_id = register_vllm_endpoint(&state, server.uri(), "responses-tps-model").await;

    let response = post_responses(
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({
            "model": "responses-tps-model",
            "input": "hello"
        })),
    )
    .await
    .expect("responses request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("response body should be readable");

    sleep(Duration::from_millis(100)).await;

    let tps = state.load_manager.get_model_tps(endpoint_id).await;
    let entry = tps
        .iter()
        .find(|info| info.model_id == "responses-tps-model")
        .expect("responses model should have TPS entry");
    assert!(entry.tps.is_some(), "TPS should be updated");
    assert!(
        entry.total_output_tokens >= 12,
        "usage.output_tokens should be accumulated"
    );
}

#[tokio::test]
async fn responses_stream_success_updates_model_tps_after_completion() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    let stream_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(stream_body, "text/event-stream"),
        )
        .mount(&server)
        .await;

    let endpoint_id = register_vllm_endpoint(&state, server.uri(), "responses-stream-model").await;

    let response = post_responses(
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({
            "model": "responses-stream-model",
            "input": "hello",
            "stream": true
        })),
    )
    .await
    .expect("streaming responses request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("stream body should be readable");

    sleep(Duration::from_millis(120)).await;

    let tps = state.load_manager.get_model_tps(endpoint_id).await;
    let entry = tps
        .iter()
        .find(|info| info.model_id == "responses-stream-model")
        .expect("streaming model should have TPS entry");
    assert!(
        entry.tps.is_some(),
        "TPS should be updated after stream completion"
    );
    assert!(
        entry.total_output_tokens > 0,
        "streaming output tokens should be accumulated"
    );
}

#[tokio::test]
async fn responses_interrupted_stream_still_records_success_stats() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    let stream_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(stream_body, "text/event-stream"),
        )
        .mount(&server)
        .await;

    let endpoint_id =
        register_vllm_endpoint(&state, server.uri(), "responses-stream-interrupted").await;

    let response = post_responses(
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0))),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({
            "model": "responses-stream-interrupted",
            "input": "hello",
            "stream": true
        })),
    )
    .await
    .expect("streaming responses request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    // Simulate client disconnect before consuming the full stream.
    drop(response);

    sleep(Duration::from_millis(120)).await;

    let endpoint = crate::db::endpoints::get_endpoint(&state.db_pool, endpoint_id)
        .await
        .expect("get endpoint should succeed")
        .expect("endpoint should exist");
    assert_eq!(endpoint.total_requests, 1);
    assert_eq!(endpoint.successful_requests, 1);
    assert_eq!(endpoint.failed_requests, 0);

    let model_stats = crate::db::endpoint_daily_stats::get_model_stats(&state.db_pool, endpoint_id)
        .await
        .expect("get model stats");
    let stat = model_stats
        .iter()
        .find(|s| s.model_id == "responses-stream-interrupted")
        .expect("model stats should exist for interrupted stream");
    assert_eq!(stat.total_requests, 1);
    assert_eq!(stat.successful_requests, 1);
    assert_eq!(stat.failed_requests, 0);
}

// --- request history recording tests (finding [C1]) ---

fn test_connect_info() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
}

#[tokio::test]
async fn responses_non_stream_success_records_history() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    let response_body = json!({
        "id": "resp_1",
        "object": "response",
        "usage": {"input_tokens": 5, "output_tokens": 9, "total_tokens": 14}
    });
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&server)
        .await;
    register_vllm_endpoint(&state, server.uri(), "resp-hist-model").await;

    let response = post_responses(
        test_connect_info(),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({"model": "resp-hist-model", "input": "hello"})),
    )
    .await
    .expect("responses request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

    sleep(Duration::from_millis(150)).await;
    let records = state
        .request_history
        .load_records()
        .await
        .expect("load records");
    assert_eq!(records.len(), 1, "one history record expected");
    let rec = &records[0];
    assert_eq!(rec.model, "resp-hist-model");
    assert_eq!(rec.request_type, RequestType::Responses);
    assert!(matches!(rec.status, RecordStatus::Success));
    assert!(
        rec.response_body.is_some(),
        "success should keep response_body"
    );
    assert_eq!(rec.output_tokens, Some(9));
}

#[tokio::test]
async fn responses_non_stream_error_records_error_history() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream boom"))
        .mount(&server)
        .await;
    register_vllm_endpoint(&state, server.uri(), "resp-err-model").await;

    let response = post_responses(
        test_connect_info(),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({"model": "resp-err-model", "input": "x"})),
    )
    .await
    .expect("passthrough should return a response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

    sleep(Duration::from_millis(150)).await;
    let records = state.request_history.load_records().await.unwrap();
    assert_eq!(records.len(), 1);
    assert!(matches!(records[0].status, RecordStatus::Error { .. }));
    assert!(records[0].response_body.is_none());
}

#[tokio::test]
async fn responses_stream_success_records_history_without_body() {
    let state = create_local_state().await;
    let server = MockServer::start().await;
    let stream_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(stream_body, "text/event-stream"),
        )
        .mount(&server)
        .await;
    register_vllm_endpoint(&state, server.uri(), "resp-stream-hist").await;

    let response = post_responses(
        test_connect_info(),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({"model": "resp-stream-hist", "input": "hi", "stream": true})),
    )
    .await
    .expect("streaming responses request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let _ = to_bytes(response.into_body(), 1_000_000).await.unwrap();

    sleep(Duration::from_millis(150)).await;
    let records = state.request_history.load_records().await.unwrap();
    assert_eq!(records.len(), 1);
    assert!(matches!(records[0].status, RecordStatus::Success));
    assert_eq!(records[0].request_type, RequestType::Responses);
    assert!(
        records[0].response_body.is_none(),
        "streaming success should not carry response_body"
    );
}

#[tokio::test]
async fn responses_unregistered_model_records_nothing() {
    let state = create_local_state().await;
    let response = post_responses(
        test_connect_info(),
        axum::http::HeaderMap::new(),
        State(state.clone()),
        None,
        Json(json!({"model": "totally-unknown-model", "input": "x"})),
    )
    .await
    .expect("should return a 404 response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    sleep(Duration::from_millis(100)).await;
    let records = state.request_history.load_records().await.unwrap();
    assert!(
        records.is_empty(),
        "404 (unregistered model) must not record history"
    );
}

// --- extract_model tests ---

#[test]
fn extract_model_returns_model_string() {
    let payload = json!({"model": "gpt-4", "input": "hello"});
    let model = super::extract_model(&payload).expect("should extract model");
    assert_eq!(model, "gpt-4");
}

#[test]
fn extract_model_returns_error_when_missing() {
    let payload = json!({"input": "hello"});
    let result = super::extract_model(&payload);
    assert!(result.is_err());
}

#[test]
fn extract_model_returns_error_for_non_string() {
    let payload = json!({"model": 42, "input": "hello"});
    let result = super::extract_model(&payload);
    assert!(result.is_err());
}

#[test]
fn extract_model_returns_error_for_null() {
    let payload = json!({"model": null});
    let result = super::extract_model(&payload);
    assert!(result.is_err());
}

#[test]
fn extract_model_returns_error_for_array_model() {
    let payload = json!({"model": ["gpt-4"]});
    let result = super::extract_model(&payload);
    assert!(result.is_err());
}

#[test]
fn extract_model_returns_error_for_empty_object() {
    let payload = json!({});
    let result = super::extract_model(&payload);
    assert!(result.is_err());
}

// --- extract_stream tests ---

#[test]
fn extract_stream_returns_true_when_set() {
    let payload = json!({"model": "test", "stream": true});
    assert!(super::extract_stream(&payload));
}

#[test]
fn extract_stream_returns_false_when_set_false() {
    let payload = json!({"model": "test", "stream": false});
    assert!(!super::extract_stream(&payload));
}

#[test]
fn extract_stream_defaults_to_false_when_missing() {
    let payload = json!({"model": "test"});
    assert!(!super::extract_stream(&payload));
}

#[test]
fn extract_stream_defaults_to_false_for_non_boolean() {
    let payload = json!({"model": "test", "stream": "yes"});
    assert!(!super::extract_stream(&payload));
}

#[test]
fn extract_stream_defaults_to_false_for_integer() {
    let payload = json!({"model": "test", "stream": 1});
    assert!(!super::extract_stream(&payload));
}

#[test]
fn extract_stream_defaults_to_false_for_null() {
    let payload = json!({"model": "test", "stream": null});
    assert!(!super::extract_stream(&payload));
}

// --- openai_error_response tests ---

#[test]
fn openai_error_response_returns_requested_status() {
    let resp = super::openai_error_response("bad request", StatusCode::BAD_REQUEST);
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn openai_error_response_returns_404() {
    let resp = super::openai_error_response("not found", StatusCode::NOT_FOUND);
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[test]
fn openai_error_response_returns_500() {
    let resp = super::openai_error_response("internal error", StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// --- model_unavailable_response tests ---

#[test]
fn model_unavailable_response_returns_503() {
    let resp = super::model_unavailable_response("no endpoints for model X", "no_capable_nodes");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn model_unavailable_response_accepts_owned_string() {
    let msg = format!("No available endpoints support model: {}", "llama3");
    let resp = super::model_unavailable_response(msg, "no_capable_nodes");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// --- add_queue_headers tests ---

#[test]
fn add_queue_headers_sets_status_and_wait() {
    use axum::http::HeaderName;

    let mut resp = axum::response::Response::new(axum::body::Body::empty());
    super::add_queue_headers(&mut resp, 150);

    assert_eq!(
        resp.headers()
            .get(HeaderName::from_static("x-queue-status"))
            .map(|v| v.to_str().unwrap()),
        Some("queued")
    );
    assert_eq!(
        resp.headers()
            .get(HeaderName::from_static("x-queue-wait-ms"))
            .map(|v| v.to_str().unwrap()),
        Some("150")
    );
}

#[test]
fn add_queue_headers_zero_wait() {
    use axum::http::HeaderName;

    let mut resp = axum::response::Response::new(axum::body::Body::empty());
    super::add_queue_headers(&mut resp, 0);

    assert_eq!(
        resp.headers()
            .get(HeaderName::from_static("x-queue-wait-ms"))
            .map(|v| v.to_str().unwrap()),
        Some("0")
    );
}

#[test]
fn add_queue_headers_large_wait_value() {
    use axum::http::HeaderName;

    let mut resp = axum::response::Response::new(axum::body::Body::empty());
    super::add_queue_headers(&mut resp, 999_999);

    assert_eq!(
        resp.headers()
            .get(HeaderName::from_static("x-queue-wait-ms"))
            .map(|v| v.to_str().unwrap()),
        Some("999999")
    );
}
