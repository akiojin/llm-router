use super::*;
use crate::db::test_utils::{TestAppStateBuilder, TEST_LOCK};
use crate::types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI};
use crate::AppState;
use axum::body::to_bytes;
use axum::http::{HeaderMap, HeaderValue};
use serial_test::serial;
use tempfile::tempdir;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn create_local_state() -> AppState {
    TestAppStateBuilder::new().await.build().await
}

async fn create_state_with_tempdir() -> (AppState, tempfile::TempDir) {
    let dir = tempdir().expect("temp dir");
    std::env::set_var("LLMLB_DATA_DIR", dir.path());
    let state = create_local_state().await;
    (state, dir)
}

async fn add_online_chat_endpoint_with_model(
    state: &AppState,
    endpoint_name: &str,
    base_url: String,
    endpoint_type: EndpointType,
    model_id: &str,
    canonical_name: Option<&str>,
) {
    let mut endpoint = Endpoint::new(endpoint_name.to_string(), base_url, endpoint_type);
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
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: canonical_name.map(str::to_string),
        })
        .await
        .expect("add endpoint model");
}

#[test]
fn anthropic_request_to_openai_maps_system_and_messages() {
    let converted = anthropic_request_to_openai(&json!({
        "model": "test-model",
        "system": "You are helpful",
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi"}
        ],
        "max_tokens": 128,
        "temperature": 0.2,
        "top_p": 0.9,
        "stop_sequences": ["END"],
        "stream": true
    }))
    .expect("conversion should succeed");

    assert_eq!(converted.openai_payload["model"], "test-model");
    assert_eq!(converted.openai_payload["messages"][0]["role"], "system");
    assert_eq!(converted.openai_payload["messages"][1]["role"], "user");
    assert_eq!(converted.openai_payload["messages"][2]["role"], "assistant");
    assert_eq!(converted.openai_payload["max_tokens"], 128);
    assert_eq!(converted.openai_payload["stream"], true);
    assert_eq!(converted.openai_payload["stop"][0], "END");
    assert!(converted.request_text.contains("system: You are helpful"));
}

#[test]
fn anthropic_request_to_openai_rejects_non_text_blocks() {
    let response = anthropic_request_to_openai(&json!({
        "model": "test-model",
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "image", "source": {}}
                ]
            }
        ],
        "max_tokens": 32
    }))
    .expect_err("non-text content must be rejected");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn openai_response_maps_to_anthropic_message_shape() {
    let usage = TokenUsage::new(Some(10), Some(6), Some(16));
    let response = openai_to_anthropic_message_response(
        &json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hello from upstream"},
                    "finish_reason": "stop"
                }
            ]
        }),
        "local-model",
        &usage,
    );

    assert_eq!(response["type"], "message");
    assert_eq!(response["role"], "assistant");
    assert_eq!(response["content"][0]["type"], "text");
    assert_eq!(response["content"][0]["text"], "Hello from upstream");
    assert_eq!(response["stop_reason"], "end_turn");
    assert_eq!(response["usage"]["input_tokens"], 10);
    assert_eq!(response["usage"]["output_tokens"], 6);
}

#[test]
fn parse_anthropic_cloud_model_accepts_alias() {
    assert_eq!(
        parse_anthropic_cloud_model("anthropic:claude-3-7-sonnet"),
        Some("claude-3-7-sonnet".to_string())
    );
    assert_eq!(
        parse_anthropic_cloud_model("ahtnorpic:claude-3-7-sonnet"),
        Some("claude-3-7-sonnet".to_string())
    );
    assert_eq!(parse_anthropic_cloud_model("local-model"), None);
}

#[test]
fn test_tools_request_conversion() {
    // Test that tools and tool_choice are accepted and converted
    let converted = anthropic_request_to_openai(&json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Call the bash tool"}
        ],
        "max_tokens": 128,
        "tools": [
            {
                "name": "bash",
                "description": "Execute bash commands",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Command to execute"
                        }
                    },
                    "required": ["command"]
                }
            }
        ],
        "tool_choice": {"type": "auto"}
    }))
    .expect("tools should be accepted and converted");

    // Verify OpenAI format
    let functions = converted
        .openai_payload
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools should be present in OpenAI payload");

    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["type"], "function");
    assert_eq!(functions[0]["function"]["name"], "bash");
    assert_eq!(
        functions[0]["function"]["description"],
        "Execute bash commands"
    );

    // Verify tool_choice conversion
    let tool_choice = converted
        .openai_payload
        .get("tool_choice")
        .expect("tool_choice should be converted");
    assert_eq!(tool_choice, "auto");
}

#[test]
fn test_tool_result_message_conversion() {
    // Test that tool_result content blocks are converted properly
    let converted = anthropic_request_to_openai(&json!({
            "model": "test-model",
            "messages": [
                {"role": "user", "content": "Call the bash tool"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "toolu_123", "name": "bash", "input": {"command": "ls"}}
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_123", "content": "file1.txt\nfile2.txt"}
                    ]
                }
            ],
            "max_tokens": 128,
            "tools": [
                {
                    "name": "bash",
                    "description": "Execute bash commands",
                    "input_schema": {
                        "type": "object",
                        "properties": {"command": {"type": "string"}},
                        "required": ["command"]
                    }
                }
            ]
        }))
        .expect("tool_result conversion should succeed");

    // Verify that tool_result message is converted to tool role
    let messages = converted
        .openai_payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages should be array");

    // Find the tool_result message (should be converted to tool role)
    let tool_message = messages
        .iter()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("tool"))
        .expect("tool_result should be converted to tool role message");

    assert_eq!(tool_message["tool_call_id"], "toolu_123");
    assert_eq!(tool_message["content"], "file1.txt\nfile2.txt");

    // assistant の tool_use が破棄されず assistant + tool_calls に変換されていること
    let assistant_with_tools = messages
        .iter()
        .find(|m| {
            m.get("role").and_then(Value::as_str) == Some("assistant")
                && m.get("tool_calls").is_some()
        })
        .expect("assistant tool_use must convert to assistant + tool_calls, not be dropped");
    let tool_calls = assistant_with_tools["tool_calls"]
        .as_array()
        .expect("tool_calls array");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "toolu_123");
    assert_eq!(tool_calls[0]["function"]["name"], "bash");
    let args: Value =
        serde_json::from_str(tool_calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(args["command"], "ls");

    // 順序整合: assistant(tool_calls) が tool 結果メッセージより前に出現する
    let assistant_pos = messages
        .iter()
        .position(|m| m.get("tool_calls").is_some())
        .unwrap();
    let tool_pos = messages
        .iter()
        .position(|m| m.get("role").and_then(Value::as_str) == Some("tool"))
        .unwrap();
    assert!(
        assistant_pos < tool_pos,
        "assistant tool_calls must precede the tool result message"
    );
}

#[test]
fn test_tool_use_response_conversion() {
    // Test that OpenAI tool_calls are converted to Anthropic tool_use content blocks
    let usage = TokenUsage::new(Some(10), Some(20), Some(30));
    let response = openai_to_anthropic_message_response(
        &json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "I'll execute the command for you.",
                        "tool_calls": [
                            {
                                "id": "call_abc123",
                                "type": "function",
                                "function": {
                                    "name": "bash",
                                    "arguments": "{\"command\": \"ls -la\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }
            ]
        }),
        "local-model",
        &usage,
    );

    // Verify response structure
    assert_eq!(response["type"], "message");
    assert_eq!(response["role"], "assistant");

    // Verify content blocks include both text and tool_use
    let content = response
        .get("content")
        .and_then(Value::as_array)
        .expect("content should be array");

    assert!(content.len() >= 2, "should have text and tool_use blocks");

    // Find tool_use block
    let tool_use = content
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("tool_use"))
        .expect("should have tool_use block");

    assert_eq!(tool_use["name"], "bash");
    assert_eq!(tool_use["id"], "call_abc123");
    assert_eq!(tool_use["input"]["command"], "ls -la");

    // Verify stop_reason
    assert_eq!(response["stop_reason"], "tool_use");
}

#[tokio::test]
#[serial]
async fn streaming_tool_use_accumulates_fragmented_arguments() {
    // OpenAI/llama.cpp はtool_callをデルタ間で断片化して送る。
    // 引数を捨てずに input_json_delta として蓄積・配信することを検証する。
    let _guard = TEST_LOCK.lock().await;
    let (state, _dir) = create_state_with_tempdir().await;

    let mut tracker = AnthropicStreamTracker {
        upstream: Box::pin(futures::stream::empty::<Result<Bytes, reqwest::Error>>()),
        upstream_line_buffer: String::new(),
        output_queue: VecDeque::new(),
        accumulator: StreamingTokenAccumulator::new("test-model"),
        endpoint_id: Uuid::new_v4(),
        model_id: "test-model".to_string(),
        endpoint_type: EndpointType::OpenaiCompatible,
        request_started_at: Instant::now(),
        endpoint_registry: state.endpoint_registry.clone(),
        load_manager: state.load_manager.clone(),
        event_bus: state.event_bus.clone(),
        sent_message_start: false,
        sent_message_stop: false,
        next_block_index: 0,
        open_text_index: None,
        tool_buffer: BTreeMap::new(),
        emitted_tool_use: false,
        response_id: "msg_test".to_string(),
        public_model: "test-model".to_string(),
        stop_reason: None,
        stop_sequence: None,
        stats_recorded: false,
    };

    tracker.process_upstream_line(
            r#"data: {"id":"chatcmpl-1","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]}}]}"#,
        );
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"location\":"}}]}}]}"#,
        );
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"NYC\"}"}}]}}]}"#,
        );
    tracker
        .process_upstream_line(r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
    tracker.finish_stream();

    let events = parse_sse_events(&tracker);

    // tool 単独ストリームでも先頭ブロックは index 0 の tool_use（index 0 欠番の回帰防止）
    let (first_index, first_type) = first_content_block(&events);
    assert_eq!(
            (first_index, first_type.as_str()),
            (0, "tool_use"),
            "tool-only stream must start at content block index 0 as tool_use, got ({first_index}, {first_type})"
        );
    assert_content_blocks_well_formed(&events);

    // tool_use の id / name が保持されている
    let (_, start_block) = events
        .iter()
        .find(|(name, _)| name == "content_block_start")
        .expect("content_block_start present");
    assert_eq!(start_block["content_block"]["type"], "tool_use");
    assert_eq!(start_block["content_block"]["name"], "get_weather");
    assert_eq!(start_block["content_block"]["id"], "call_abc");

    // 全 input_json_delta の partial_json を結合して引数JSONを復元する
    let reconstructed = reconstruct_tool_arguments(&events);
    assert_eq!(
        reconstructed, r#"{"location":"NYC"}"#,
        "tool arguments were not accumulated correctly: {reconstructed:?}"
    );
}

#[tokio::test]
#[serial]
async fn streaming_text_then_interleaved_parallel_tools() {
    // text の後に、引数フラグメントが index を跨いで interleave される並列 tool_call が来ても、
    // index 単位で正しく結合され、単一オープンの逐次 tool_use ブロックとして出力されることを検証する。
    let _guard = TEST_LOCK.lock().await;
    let (state, _dir) = create_state_with_tempdir().await;

    let mut tracker = AnthropicStreamTracker {
        upstream: Box::pin(futures::stream::empty::<Result<Bytes, reqwest::Error>>()),
        upstream_line_buffer: String::new(),
        output_queue: VecDeque::new(),
        accumulator: StreamingTokenAccumulator::new("test-model"),
        endpoint_id: Uuid::new_v4(),
        model_id: "test-model".to_string(),
        endpoint_type: EndpointType::OpenaiCompatible,
        request_started_at: Instant::now(),
        endpoint_registry: state.endpoint_registry.clone(),
        load_manager: state.load_manager.clone(),
        event_bus: state.event_bus.clone(),
        sent_message_start: false,
        sent_message_stop: false,
        next_block_index: 0,
        open_text_index: None,
        tool_buffer: BTreeMap::new(),
        emitted_tool_use: false,
        response_id: "msg_test".to_string(),
        public_model: "test-model".to_string(),
        stop_reason: None,
        stop_sequence: None,
        stats_recorded: false,
    };

    tracker.process_upstream_line(r#"data: {"choices":[{"delta":{"content":"thinking"}}]}"#);
    // index 0 と 1 を開き、引数を 0,1,0,1 と交互に流す
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c0","type":"function","function":{"name":"f0","arguments":"{\"a\":"}}]}}]}"#,
        );
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c1","type":"function","function":{"name":"f1","arguments":"{\"b\":"}}]}}]}"#,
        );
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}}]}"#,
        );
    tracker.process_upstream_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"2}"}}]}}]}"#,
        );
    tracker
        .process_upstream_line(r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
    tracker.finish_stream();

    let events = parse_sse_events(&tracker);
    assert_content_blocks_well_formed(&events);

    // ブロックは text(0) → tool_use(1) → tool_use(2) の単一オープン逐次
    let blocks: Vec<(u64, String)> = events
        .iter()
        .filter(|(name, _)| name == "content_block_start")
        .map(|(_, j)| {
            (
                j["index"].as_u64().unwrap(),
                j["content_block"]["type"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(
        blocks,
        vec![
            (0, "text".to_string()),
            (1, "tool_use".to_string()),
            (2, "tool_use".to_string())
        ],
        "blocks must be text(0) -> tool_use(1) -> tool_use(2): {blocks:?}"
    );

    // 各 tool の id/name と interleave 結合された引数を検証
    let tool_starts: Vec<&Value> = events
        .iter()
        .filter(|(name, j)| {
            name == "content_block_start" && j["content_block"]["type"].as_str() == Some("tool_use")
        })
        .map(|(_, j)| j)
        .collect();
    assert_eq!(tool_starts[0]["content_block"]["id"], "c0");
    assert_eq!(tool_starts[0]["content_block"]["name"], "f0");
    assert_eq!(tool_starts[1]["content_block"]["id"], "c1");
    assert_eq!(tool_starts[1]["content_block"]["name"], "f1");

    // input_json_delta を index 別に結合
    let mut args_by_index: std::collections::BTreeMap<u64, String> = Default::default();
    for (name, j) in &events {
        if name == "content_block_delta" && j["delta"]["type"].as_str() == Some("input_json_delta")
        {
            let idx = j["index"].as_u64().unwrap();
            args_by_index
                .entry(idx)
                .or_default()
                .push_str(j["delta"]["partial_json"].as_str().unwrap());
        }
    }
    assert_eq!(
        args_by_index.get(&1).map(String::as_str),
        Some(r#"{"a":1}"#)
    );
    assert_eq!(
        args_by_index.get(&2).map(String::as_str),
        Some(r#"{"b":2}"#)
    );
}

/// emit された SSE ペイロードを (event名, data JSON) の列にパースする。
fn parse_sse_events(tracker: &AnthropicStreamTracker) -> Vec<(String, Value)> {
    tracker
        .output_queue
        .iter()
        .filter_map(|b| {
            let payload = String::from_utf8_lossy(b);
            let name = payload
                .lines()
                .find(|l| l.starts_with("event: "))?
                .trim_start_matches("event: ")
                .to_string();
            let data = payload.lines().find(|l| l.starts_with("data: "))?;
            let json = serde_json::from_str::<Value>(data.trim_start_matches("data: ")).ok()?;
            Some((name, json))
        })
        .collect()
}

fn first_content_block(events: &[(String, Value)]) -> (u64, String) {
    let (_, j) = events
        .iter()
        .find(|(name, _)| name == "content_block_start")
        .expect("at least one content_block_start");
    (
        j["index"].as_u64().unwrap(),
        j["content_block"]["type"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    )
}

/// content_block_start の index が 0 始まり連番で、各 start に対応する stop が
/// ちょうど1回あり、message_stop の後にイベントが無いことを検証する。
fn assert_content_blocks_well_formed(events: &[(String, Value)]) {
    // Anthropic 契約: 同時に開く content block は常に1つ。各 start は 0 始まり連番で、
    // 対応する stop で閉じてから次を start する。delta は開いているブロックにのみ送る。
    let mut open_block: Option<usize> = None;
    let mut next_expected = 0usize;
    let mut seen_message_stop = false;
    for (name, j) in events {
        assert!(
            !seen_message_stop,
            "no event may follow message_stop, got {name}"
        );
        match name.as_str() {
            "content_block_start" => {
                let idx = j["index"].as_u64().unwrap() as usize;
                assert!(
                    open_block.is_none(),
                    "content_block_start({idx}) while block {open_block:?} is still open (overlap)"
                );
                assert_eq!(
                    idx, next_expected,
                    "content_block_start index must be 0-based contiguous"
                );
                open_block = Some(idx);
                next_expected += 1;
            }
            "content_block_delta" => {
                let idx = j["index"].as_u64().unwrap() as usize;
                assert_eq!(
                        open_block,
                        Some(idx),
                        "content_block_delta({idx}) must target the currently open block {open_block:?}"
                    );
            }
            "content_block_stop" => {
                let idx = j["index"].as_u64().unwrap() as usize;
                assert_eq!(
                    open_block,
                    Some(idx),
                    "content_block_stop({idx}) must close the currently open block {open_block:?}"
                );
                open_block = None;
            }
            "message_stop" => seen_message_stop = true,
            _ => {}
        }
    }
    assert!(
        open_block.is_none(),
        "a content block was left open: {open_block:?}"
    );
    assert!(seen_message_stop, "stream must end with message_stop");
}

fn reconstruct_tool_arguments(events: &[(String, Value)]) -> String {
    let mut out = String::new();
    for (_, j) in events {
        if j.get("delta")
            .and_then(|d| d.get("type"))
            .and_then(Value::as_str)
            == Some("input_json_delta")
        {
            if let Some(frag) = j["delta"]["partial_json"].as_str() {
                out.push_str(frag);
            }
        }
    }
    out
}

#[tokio::test]
#[serial]
async fn local_body_read_failure_returns_error_response_and_records_history() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _guard = TEST_LOCK.lock().await;
    let (state, _dir) = create_state_with_tempdir().await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut read_buf = [0u8; 4096];
            let _ = socket.read(&mut read_buf).await;
            let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 256\r\nConnection: close\r\n\r\n{\"id\":\"truncated\"}";
            let _ = socket.write_all(response).await;
            let _ = socket.shutdown().await;
        }
    });

    let mut endpoint = Endpoint::new(
        "broken-anthropic-endpoint".to_string(),
        format!("http://{addr}"),
        EndpointType::OpenaiCompatible,
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
            model_id: "broken-model".to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        })
        .await
        .expect("add endpoint model");

    let mut headers = HeaderMap::new();
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    let payload = json!({
        "model": "broken-model",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let response = handle_messages(
        std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
        headers,
        state.clone(),
        None,
        payload,
    )
    .await
    .expect("body read failure should return response");

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("response body");
    let json: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "api_error");
    let message = json["error"]["message"]
        .as_str()
        .expect("error message")
        .to_string();
    assert!(
        message.contains("Failed to read OpenAI-compatible upstream response"),
        "unexpected error message: {message}"
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let snapshot = state
        .load_manager
        .snapshot(endpoint_id)
        .await
        .expect("snapshot");
    assert_eq!(snapshot.active_requests, 0);

    let records = state.request_history.load_records().await.expect("records");
    assert_eq!(records.len(), 1);
    match &records[0].status {
        RecordStatus::Error {
            message: record_message,
        } => assert_eq!(record_message, &message),
        _ => panic!("expected request history error record"),
    }
}

#[tokio::test]
#[serial]
async fn canonical_model_routes_to_ollama_alias_backed_endpoint() {
    let _guard = TEST_LOCK.lock().await;
    let (state, _dir) = create_state_with_tempdir().await;
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({"model": "gpt-oss:20b"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-alias",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        })))
        .mount(&server)
        .await;

    add_online_chat_endpoint_with_model(
        &state,
        "ollama-alias",
        server.uri(),
        EndpointType::Ollama,
        "gpt-oss:20b",
        Some("openai/gpt-oss-20b"),
    )
    .await;

    let mut headers = HeaderMap::new();
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    let payload = json!({
        "model": "openai/gpt-oss-20b",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let response = handle_messages(
        std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
        headers,
        state.clone(),
        None,
        payload,
    )
    .await
    .expect("Anthropic request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("response body");
    let json: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(json["model"], "openai/gpt-oss-20b");
    assert_eq!(json["content"][0]["text"], "ok");
}

#[tokio::test]
#[serial]
async fn canonical_model_routes_to_lmstudio_endpoint_without_alias_rewrite() {
    let _guard = TEST_LOCK.lock().await;
    let (state, _dir) = create_state_with_tempdir().await;
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({"model": "openai/gpt-oss-20b"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-canonical",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        })))
        .mount(&server)
        .await;

    add_online_chat_endpoint_with_model(
        &state,
        "lmstudio-canonical",
        server.uri(),
        EndpointType::LmStudio,
        "openai/gpt-oss-20b",
        Some("openai/gpt-oss-20b"),
    )
    .await;

    let mut headers = HeaderMap::new();
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    let payload = json!({
        "model": "openai/gpt-oss-20b",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let response = handle_messages(
        std::net::SocketAddr::from(([127, 0, 0, 1], 8080)),
        headers,
        state.clone(),
        None,
        payload,
    )
    .await
    .expect("Anthropic request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("response body");
    let json: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(json["model"], "openai/gpt-oss-20b");
    assert_eq!(json["content"][0]["text"], "ok");
}
