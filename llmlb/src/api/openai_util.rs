//! OpenAI互換APIのユーティリティ関数
//!
//! ペイロードのサニタイズ、メッセージ変換、エラーレスポンス生成など。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::error::Error as StdError;
use std::time::Duration;

/// Structured classification for an upstream request failure.
pub struct UpstreamRequestFailure {
    /// HTTP status returned to the client.
    pub status_code: StatusCode,
    /// Stable error type string used by the OpenAI-compatible error body.
    pub error_type: &'static str,
    /// User-facing error message.
    pub client_message: String,
    /// Detailed message stored in request history.
    pub record_message: String,
}

#[derive(Debug, Deserialize)]
struct OllamaPsResponse {
    #[serde(default)]
    models: Vec<OllamaPsModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaPsModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

fn normalize_ollama_model_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn ollama_model_name_matches(candidate: &str, target: &str) -> bool {
    let candidate = normalize_ollama_model_name(candidate);
    let target = normalize_ollama_model_name(target);

    candidate == target
        || candidate.trim_end_matches(":latest") == target
        || candidate == target.trim_end_matches(":latest")
        || candidate.trim_end_matches(":latest") == target.trim_end_matches(":latest")
}

/// Returns whether the requested Ollama model is currently resident in memory.
///
/// `None` means the probe itself was inconclusive.
pub async fn probe_ollama_model_loaded(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Option<bool> {
    if model.trim().is_empty() {
        return None;
    }

    let url = format!("{}/api/ps", base_url.trim_end_matches('/'));
    let mut request = client.get(url).timeout(Duration::from_secs(3));
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    let ps: OllamaPsResponse = response.json().await.ok()?;
    Some(ps.models.into_iter().any(|entry| {
        ollama_model_name_matches(&entry.name, model)
            || ollama_model_name_matches(&entry.model, model)
    }))
}

/// Classifies a reqwest upstream failure into a stable client-facing error response.
pub fn classify_upstream_request_error(
    error: &reqwest::Error,
    upstream_url: &str,
    timeout_secs: u32,
    ollama_loading_model: Option<&str>,
) -> UpstreamRequestFailure {
    if error.is_timeout() {
        if let Some(model) = ollama_loading_model {
            let client_message = format!(
                "Ollama model '{}' is still loading. Retry after the initial load finishes or increase endpoint inference timeout above {} seconds.",
                model, timeout_secs
            );
            return UpstreamRequestFailure {
                status_code: StatusCode::GATEWAY_TIMEOUT,
                error_type: "model_loading",
                record_message: format!("{}: {}", client_message, error),
                client_message,
            };
        }

        let client_message =
            format!("Upstream request to {upstream_url} timed out after {timeout_secs} seconds");
        return UpstreamRequestFailure {
            status_code: StatusCode::GATEWAY_TIMEOUT,
            error_type: "timeout",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    if error.is_connect() {
        let client_message = format!("Failed to connect to upstream: {upstream_url}");
        return UpstreamRequestFailure {
            status_code: StatusCode::BAD_GATEWAY,
            error_type: "connection_error",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    if is_tls_error(error) {
        let client_message = format!("Upstream TLS handshake failed: {upstream_url}");
        return UpstreamRequestFailure {
            status_code: StatusCode::BAD_GATEWAY,
            error_type: "tls_error",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    let client_message = format!("Upstream request failed: {upstream_url}");
    UpstreamRequestFailure {
        status_code: StatusCode::BAD_GATEWAY,
        error_type: "endpoint_request_error",
        record_message: format!("{}: {}", client_message, error),
        client_message,
    }
}

/// Build a client-visible upstream error message from an HTTP status and raw body bytes.
pub fn upstream_error_message_from_bytes(status: StatusCode, body_bytes: &[u8]) -> String {
    let body = String::from_utf8_lossy(body_bytes).trim().to_string();
    if body.is_empty() {
        status.to_string()
    } else {
        body
    }
}

/// 上流エラー要約のために読み取るボディの最大バイト数（4KB）。
///
/// エラーメッセージの要約には先頭の数 KB で十分であり、悪意/誤動作の上流が
/// 巨大なエラーボディを返してもメモリを枯渇させないための上限。
pub const UPSTREAM_ERROR_SUMMARY_MAX_BYTES: usize = 4096;

/// レスポンスボディを先頭 `max_bytes` までに制限して読み取る。
///
/// `response.bytes()` は全文をメモリに展開するため、エラー要約のように
/// 先頭部分しか必要としない用途では本関数で読み取り量を上限化する。
/// 残りのチャンクは読まずに破棄する。読み取り中のエラーはそこまでの
/// 内容を返す（要約用途では致命的でない）。
pub async fn read_capped_body(response: reqwest::Response, max_bytes: usize) -> Vec<u8> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < max_bytes {
        match stream.next().await {
            Some(Ok(chunk)) => {
                let remaining = max_bytes - buf.len();
                if chunk.len() > remaining {
                    buf.extend_from_slice(&chunk[..remaining]);
                    break;
                }
                buf.extend_from_slice(&chunk);
            }
            Some(Err(_)) | None => break,
        }
    }
    buf
}

fn is_tls_error(error: &reqwest::Error) -> bool {
    error_chain_contains(error, &["certificate", "cert", "tls", "ssl", "handshake"])
}

fn error_chain_contains(error: &reqwest::Error, needles: &[&str]) -> bool {
    let mut current = error.source();
    while let Some(source) = current {
        let text = source.to_string().to_ascii_lowercase();
        if needles.iter().any(|needle| text.contains(needle)) {
            return true;
        }
        current = source.source();
    }
    false
}

/// 履歴保存用にペイロードをサニタイズ（base64データをリダクト）
pub fn sanitize_openai_payload_for_history(payload: &Value) -> Value {
    fn redact_data_url(value: &Value) -> Value {
        match value {
            Value::String(s) => {
                if s.starts_with("data:") && s.contains(";base64,") {
                    Value::String(format!("[redacted data-url len={}]", s.len()))
                } else {
                    Value::String(s.clone())
                }
            }
            Value::Array(items) => Value::Array(items.iter().map(redact_data_url).collect()),
            Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    if k == "input_audio" {
                        if let Some(obj) = v.as_object() {
                            let mut cloned = obj.clone();
                            if let Some(data) = obj.get("data").and_then(|d| d.as_str()) {
                                cloned.insert(
                                    "data".to_string(),
                                    Value::String(format!("[redacted base64 len={}]", data.len())),
                                );
                            }
                            out.insert(k.clone(), Value::Object(cloned));
                            continue;
                        }
                    }

                    if k == "image_url" {
                        if let Some(obj) = v.as_object() {
                            let mut cloned = obj.clone();
                            if let Some(url) = obj.get("url").and_then(|d| d.as_str()) {
                                if url.starts_with("data:") && url.contains(";base64,") {
                                    cloned.insert(
                                        "url".to_string(),
                                        Value::String(format!(
                                            "[redacted data-url len={}]",
                                            url.len()
                                        )),
                                    );
                                }
                            }
                            out.insert(k.clone(), Value::Object(cloned));
                            continue;
                        }
                    }

                    out.insert(k.clone(), redact_data_url(v));
                }
                Value::Object(out)
            }
            _ => value.clone(),
        }
    }

    redact_data_url(payload)
}

/// OpenAIメッセージ形式をGoogle Generative AI形式に変換
pub fn map_openai_messages_to_google_contents(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|m| {
            let role = m.get("role")?.as_str().unwrap_or("user");
            let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let mapped_role = match role {
                "assistant" => "model",
                _ => "user",
            };
            Some(json!({
                "role": mapped_role,
                "parts": [{"text": text}]
            }))
        })
        .collect()
}

/// OpenAIメッセージ形式をAnthropic形式に変換（systemメッセージを分離）
pub fn map_openai_messages_to_anthropic(messages: &[Value]) -> (Option<String>, Vec<Value>) {
    let mut system_msgs: Vec<String> = Vec::new();
    let mut regular: Vec<Value> = Vec::new();
    for m in messages.iter() {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let text = m.get("content").and_then(|c| c.as_str()).unwrap_or("");
        match role {
            "system" => system_msgs.push(text.to_string()),
            "assistant" => regular.push(json!({
                "role": "assistant",
                "content": [{"type":"text","text": text}]
            })),
            _ => regular.push(json!({
                "role": "user",
                "content": [{"type":"text","text": text}]
            })),
        }
    }
    let system = if system_msgs.is_empty() {
        None
    } else {
        Some(system_msgs.join("\n"))
    };
    (system, regular)
}

/// OpenAI互換のエラーレスポンスを生成
pub fn openai_error_response_with_type(
    message: impl Into<String>,
    error_type: impl Into<String>,
    status: StatusCode,
) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": error_type.into(),
            "code": status.as_u16(),
        }
    });

    (status, Json(payload)).into_response()
}

/// Default OpenAI-compatible error response using `invalid_request_error`.
pub fn openai_error_response(message: impl Into<String>, status: StatusCode) -> Response {
    openai_error_response_with_type(message, "invalid_request_error", status)
}

/// モデル利用不可レスポンスを生成
pub fn model_unavailable_response(message: impl Into<String>, code: &str) -> Response {
    let payload = json!({
        "error": {
            "message": message.into(),
            "type": "service_unavailable",
            "code": code,
        }
    });

    (StatusCode::SERVICE_UNAVAILABLE, Json(payload)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::json;

    // ========================================================================
    // sanitize_openai_payload_for_history
    // ========================================================================

    #[test]
    fn sanitize_plain_text_message_passes_through() {
        // Given: a simple text payload
        let payload = json!({"messages": [{"role": "user", "content": "hello"}]});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, payload);
    }

    #[test]
    fn sanitize_redacts_base64_data_url() {
        // Given: a string value that is a base64 data URL
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANS...";
        let payload = json!({"url": data_url});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: the value is redacted with length info
        let url_val = result["url"].as_str().unwrap();
        assert!(url_val.starts_with("[redacted data-url len="));
        assert!(url_val.contains(&data_url.len().to_string()));
    }

    #[test]
    fn sanitize_redacts_base64_in_nested_object() {
        // Given: a deeply nested base64 data URL
        let payload = json!({
            "outer": {
                "inner": "data:audio/wav;base64,UklGR..."
            }
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: the nested value is redacted
        let inner = result["outer"]["inner"].as_str().unwrap();
        assert!(inner.starts_with("[redacted data-url len="));
    }

    #[test]
    fn sanitize_empty_payload() {
        // Given: an empty object
        let payload = json!({});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: still an empty object
        assert_eq!(result, json!({}));
    }

    #[test]
    fn sanitize_redacts_base64_inside_array() {
        // Given: an array containing a base64 data URL
        let payload = json!(["data:image/jpeg;base64,/9j/4AAQ...", "normal text"]);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: only the data URL is redacted
        assert!(result[0]
            .as_str()
            .unwrap()
            .starts_with("[redacted data-url"));
        assert_eq!(result[1].as_str().unwrap(), "normal text");
    }

    #[test]
    fn sanitize_redacts_image_url_with_base64() {
        // Given: an image_url object with base64 URL
        let payload = json!({
            "image_url": {
                "url": "data:image/png;base64,abc123",
                "detail": "auto"
            }
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: the url field is redacted but detail is preserved
        let url = result["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("[redacted data-url len="));
        assert_eq!(result["image_url"]["detail"].as_str().unwrap(), "auto");
    }

    #[test]
    fn sanitize_preserves_normal_image_url() {
        // Given: an image_url object with a normal HTTP URL
        let payload = json!({
            "image_url": {
                "url": "https://example.com/image.png"
            }
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: the URL is unchanged
        assert_eq!(
            result["image_url"]["url"].as_str().unwrap(),
            "https://example.com/image.png"
        );
    }

    #[test]
    fn sanitize_redacts_input_audio_data() {
        // Given: an input_audio object with base64 data
        let payload = json!({
            "input_audio": {
                "data": "SGVsbG8gV29ybGQ=",
                "format": "wav"
            }
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: data is redacted but format is preserved
        let data = result["input_audio"]["data"].as_str().unwrap();
        assert!(data.starts_with("[redacted base64 len="));
        assert_eq!(result["input_audio"]["format"].as_str().unwrap(), "wav");
    }

    #[test]
    fn sanitize_input_audio_not_object_passes_through() {
        // Given: input_audio is a string, not an object
        let payload = json!({"input_audio": "just a string"});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: treated as a regular value (recursed into, but it's a string not a data URL)
        assert_eq!(result["input_audio"].as_str().unwrap(), "just a string");
    }

    #[test]
    fn sanitize_preserves_numbers_null_bool() {
        // Given: various primitive types
        let payload = json!({"count": 42, "active": true, "name": null, "ratio": 2.5});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: all values are unchanged
        assert_eq!(result["count"], 42);
        assert_eq!(result["active"], true);
        assert!(result["name"].is_null());
        assert_eq!(result["ratio"], 2.5);
    }

    #[test]
    fn sanitize_deeply_nested_object() {
        // Given: a deeply nested structure with base64 at the leaf
        let payload = json!({
            "a": {"b": {"c": {"d": "data:text/plain;base64,dGVzdA=="}}}
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: the deep value is redacted
        let val = result["a"]["b"]["c"]["d"].as_str().unwrap();
        assert!(val.starts_with("[redacted data-url len="));
    }

    #[test]
    fn sanitize_empty_string_passes_through() {
        // Given: an empty string value
        let payload = json!({"content": ""});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: empty string is preserved
        assert_eq!(result["content"].as_str().unwrap(), "");
    }

    #[test]
    fn sanitize_data_prefix_without_base64_passes_through() {
        // Given: a string starting with "data:" but without ";base64,"
        let payload = json!({"url": "data:text/plain,Hello"});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged because it lacks ;base64,
        assert_eq!(result["url"].as_str().unwrap(), "data:text/plain,Hello");
    }

    #[test]
    fn sanitize_multiple_image_urls_in_array() {
        // Given: multiple image_url objects in an array
        let payload = json!({
            "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}},
                {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,xyz"}}
            ]
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: base64 URLs are redacted, normal URL preserved
        let items = result["content"].as_array().unwrap();
        assert!(items[0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("[redacted"));
        assert_eq!(
            items[1]["image_url"]["url"].as_str().unwrap(),
            "https://example.com/img.png"
        );
        assert!(items[2]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("[redacted"));
    }

    #[test]
    fn sanitize_full_messages_array() {
        // Given: a typical OpenAI payload with messages
        let payload = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "What is this?"},
                {"role": "user", "content": [
                    {"type": "text", "text": "Describe this image"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,LONGDATA"}}
                ]}
            ]
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: model and text preserved, base64 redacted
        assert_eq!(result["model"].as_str().unwrap(), "gpt-4");
        assert_eq!(
            result["messages"][0]["content"].as_str().unwrap(),
            "What is this?"
        );
        let img_url = result["messages"][1]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap();
        assert!(img_url.starts_with("[redacted data-url len="));
    }

    // ========================================================================
    // map_openai_messages_to_google_contents
    // ========================================================================

    #[test]
    fn google_maps_assistant_to_model() {
        // Given: an assistant message
        let messages = vec![json!({"role": "assistant", "content": "Hi there"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: role is "model"
        assert_eq!(result[0]["role"], "model");
        assert_eq!(result[0]["parts"][0]["text"], "Hi there");
    }

    #[test]
    fn google_maps_user_to_user() {
        // Given: a user message
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: role is "user"
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn google_maps_system_to_user() {
        // Given: a system message
        let messages = vec![json!({"role": "system", "content": "You are helpful"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: role becomes "user" (not "system")
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["parts"][0]["text"], "You are helpful");
    }

    #[test]
    fn google_empty_messages() {
        // Given: empty messages
        let messages: Vec<Value> = vec![];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: empty result
        assert!(result.is_empty());
    }

    #[test]
    fn google_skips_message_without_role() {
        // Given: a message missing the "role" field
        let messages = vec![json!({"content": "no role here"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: message is filtered out (filter_map returns None)
        assert!(result.is_empty());
    }

    #[test]
    fn google_missing_content_defaults_to_empty() {
        // Given: a message with role but no content
        let messages = vec![json!({"role": "user"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: text defaults to empty string
        assert_eq!(result[0]["parts"][0]["text"], "");
    }

    #[test]
    fn google_multiple_messages() {
        // Given: multiple messages with different roles
        let messages = vec![
            json!({"role": "system", "content": "Be helpful"}),
            json!({"role": "user", "content": "Hi"}),
            json!({"role": "assistant", "content": "Hello!"}),
        ];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: correct count and roles
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[1]["role"], "user");
        assert_eq!(result[2]["role"], "model");
    }

    #[test]
    fn google_null_content_defaults_to_empty() {
        // Given: a message with null content
        let messages = vec![json!({"role": "user", "content": null})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: text defaults to empty (as_str on null returns None → unwrap_or(""))
        assert_eq!(result[0]["parts"][0]["text"], "");
    }

    // ========================================================================
    // map_openai_messages_to_anthropic
    // ========================================================================

    #[test]
    fn anthropic_separates_system_message() {
        // Given: a system message followed by a user message
        let messages = vec![
            json!({"role": "system", "content": "You are a bot"}),
            json!({"role": "user", "content": "Hello"}),
        ];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system is separated, user message remains
        assert_eq!(system.unwrap(), "You are a bot");
        assert_eq!(regular.len(), 1);
        assert_eq!(regular[0]["role"], "user");
    }

    #[test]
    fn anthropic_joins_multiple_system_messages() {
        // Given: multiple system messages
        let messages = vec![
            json!({"role": "system", "content": "Rule 1"}),
            json!({"role": "system", "content": "Rule 2"}),
            json!({"role": "user", "content": "Hi"}),
        ];
        // When: mapped
        let (system, _regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system messages joined with newline
        assert_eq!(system.unwrap(), "Rule 1\nRule 2");
    }

    #[test]
    fn anthropic_no_system_returns_none() {
        // Given: no system messages
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system is None
        assert!(system.is_none());
        assert_eq!(regular.len(), 1);
    }

    #[test]
    fn anthropic_maps_assistant_role() {
        // Given: an assistant message
        let messages = vec![json!({"role": "assistant", "content": "Sure"})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: role is "assistant" with content array
        assert_eq!(regular[0]["role"], "assistant");
        assert_eq!(regular[0]["content"][0]["type"], "text");
        assert_eq!(regular[0]["content"][0]["text"], "Sure");
    }

    #[test]
    fn anthropic_maps_user_role() {
        // Given: a user message
        let messages = vec![json!({"role": "user", "content": "Question?"})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: role is "user" with content array
        assert_eq!(regular[0]["role"], "user");
        assert_eq!(regular[0]["content"][0]["text"], "Question?");
    }

    #[test]
    fn anthropic_empty_messages() {
        // Given: empty messages
        let messages: Vec<Value> = vec![];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system None, empty regular
        assert!(system.is_none());
        assert!(regular.is_empty());
    }

    #[test]
    fn anthropic_missing_role_defaults_to_user() {
        // Given: a message missing role and content
        let messages = vec![json!({})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: defaults to user role with empty content
        assert_eq!(regular[0]["role"], "user");
        assert_eq!(regular[0]["content"][0]["text"], "");
    }

    #[test]
    fn anthropic_mixed_messages() {
        // Given: a mix of system, user, assistant messages
        let messages = vec![
            json!({"role": "system", "content": "Be concise"}),
            json!({"role": "user", "content": "Hi"}),
            json!({"role": "assistant", "content": "Hello"}),
            json!({"role": "user", "content": "Bye"}),
        ];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system separated, 3 regular messages in order
        assert_eq!(system.unwrap(), "Be concise");
        assert_eq!(regular.len(), 3);
        assert_eq!(regular[0]["role"], "user");
        assert_eq!(regular[1]["role"], "assistant");
        assert_eq!(regular[2]["role"], "user");
    }

    // ========================================================================
    // openai_error_response
    // ========================================================================

    #[tokio::test]
    async fn openai_error_response_status_and_body() {
        // Given: an error message and status code
        let resp = openai_error_response("bad request", StatusCode::BAD_REQUEST);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: status is 400 and body has correct structure
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["message"], "bad request");
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert_eq!(body["error"]["code"], 400);
    }

    #[tokio::test]
    async fn openai_error_response_different_status() {
        // Given: a 404 error
        let resp = openai_error_response("not found", StatusCode::NOT_FOUND);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: status is 404
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["message"], "not found");
        assert_eq!(body["error"]["code"], 404);
    }

    // ========================================================================
    // model_unavailable_response
    // ========================================================================

    #[tokio::test]
    async fn model_unavailable_response_status_503() {
        // Given: a model unavailable error
        let resp = model_unavailable_response("model not loaded", "model_not_found");
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: status is 503 and body is correct
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["message"], "model not loaded");
        assert_eq!(body["error"]["type"], "service_unavailable");
        assert_eq!(body["error"]["code"], "model_not_found");
    }

    #[tokio::test]
    async fn model_unavailable_response_custom_code() {
        // Given: a model unavailable error with different code
        let resp = model_unavailable_response("GPU busy", "gpu_unavailable");
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: custom code is set
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["message"], "GPU busy");
        assert_eq!(body["error"]["code"], "gpu_unavailable");
    }

    // ========================================================================
    // 追加テスト: sanitize_openai_payload_for_history edge cases
    // ========================================================================

    #[test]
    fn sanitize_null_value_passes_through() {
        // Given: a null JSON value
        let payload = json!(null);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert!(result.is_null());
    }

    #[test]
    fn sanitize_boolean_value_passes_through() {
        // Given: a boolean JSON value
        let payload = json!(true);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, json!(true));
    }

    #[test]
    fn sanitize_number_value_passes_through() {
        // Given: a numeric JSON value
        let payload = json!(42);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, json!(42));
    }

    #[test]
    fn sanitize_float_value_passes_through() {
        // Given: a float JSON value
        let payload = json!(2.5);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, json!(2.5));
    }

    #[test]
    fn sanitize_plain_string_passes_through() {
        // Given: a plain string (not data URL)
        let payload = json!("hello world");
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, json!("hello world"));
    }

    #[test]
    fn sanitize_empty_array_passes_through() {
        // Given: an empty array
        let payload = json!([]);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: unchanged
        assert_eq!(result, json!([]));
    }

    #[test]
    fn sanitize_array_with_mixed_types() {
        // Given: an array with mixed types including a data URL
        let payload = json!([42, "normal", "data:image/png;base64,abc", true, null]);
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: only the data URL is redacted
        assert_eq!(result[0], 42);
        assert_eq!(result[1], "normal");
        assert!(result[2]
            .as_str()
            .unwrap()
            .starts_with("[redacted data-url"));
        assert_eq!(result[3], true);
        assert!(result[4].is_null());
    }

    #[test]
    fn sanitize_image_url_not_an_object_passes_through() {
        // Given: image_url is a number (not an object)
        let payload = json!({"image_url": 123});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: treated as a regular value
        assert_eq!(result["image_url"], 123);
    }

    #[test]
    fn sanitize_image_url_without_url_field() {
        // Given: image_url object without "url" key
        let payload = json!({"image_url": {"detail": "high"}});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: preserved as-is (url is None, no redaction)
        assert_eq!(result["image_url"]["detail"], "high");
    }

    #[test]
    fn sanitize_input_audio_without_data_field() {
        // Given: input_audio object without "data" key
        let payload = json!({"input_audio": {"format": "mp3"}});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: preserved as-is
        assert_eq!(result["input_audio"]["format"], "mp3");
    }

    #[test]
    fn sanitize_input_audio_data_not_string() {
        // Given: input_audio with data as a number (not a string)
        let payload = json!({"input_audio": {"data": 12345, "format": "wav"}});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: data is preserved (as_str returns None, so no redaction)
        assert_eq!(result["input_audio"]["data"], 12345);
        assert_eq!(result["input_audio"]["format"], "wav");
    }

    #[test]
    fn sanitize_image_url_with_non_data_url_preserves() {
        // Given: image_url with an HTTP URL (not data:)
        let payload = json!({
            "image_url": {
                "url": "https://cdn.example.com/photo.jpg",
                "detail": "low"
            }
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: URL preserved
        assert_eq!(
            result["image_url"]["url"],
            "https://cdn.example.com/photo.jpg"
        );
        assert_eq!(result["image_url"]["detail"], "low");
    }

    #[test]
    fn sanitize_string_starting_with_data_but_not_url() {
        // Given: a string that starts with "data:" but has no ";base64,"
        let payload = json!("data:application/json,{\"key\":\"value\"}");
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: not redacted (no ;base64,)
        assert_eq!(
            result.as_str().unwrap(),
            "data:application/json,{\"key\":\"value\"}"
        );
    }

    #[test]
    fn sanitize_very_long_base64_data_url() {
        // Given: a very long base64 data URL
        let long_data = format!("data:image/png;base64,{}", "A".repeat(100_000));
        let payload = json!({"data": long_data});
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: redacted with correct length
        let redacted = result["data"].as_str().unwrap();
        assert!(redacted.starts_with("[redacted data-url len="));
        assert!(redacted.contains(&long_data.len().to_string()));
    }

    #[test]
    fn sanitize_nested_input_audio_in_messages() {
        // Given: a realistic payload with input_audio inside messages array
        let payload = json!({
            "model": "gpt-4o-audio",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_audio",
                    "input_audio": {
                        "data": "SGVsbG8gV29ybGQ=",
                        "format": "wav"
                    }
                }]
            }]
        });
        // When: sanitized
        let result = sanitize_openai_payload_for_history(&payload);
        // Then: model preserved, audio data redacted
        assert_eq!(result["model"], "gpt-4o-audio");
        let audio_data = result["messages"][0]["content"][0]["input_audio"]["data"]
            .as_str()
            .unwrap();
        assert!(audio_data.starts_with("[redacted base64 len="));
        assert_eq!(
            result["messages"][0]["content"][0]["input_audio"]["format"],
            "wav"
        );
    }

    // ========================================================================
    // 追加テスト: map_openai_messages_to_google_contents edge cases
    // ========================================================================

    #[test]
    fn google_maps_unknown_role_to_user() {
        // Given: a message with an unknown role
        let messages = vec![json!({"role": "function", "content": "result"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: role becomes "user" (the _ match arm)
        assert_eq!(result[0]["role"], "user");
    }

    #[test]
    fn google_content_is_number_defaults_to_empty() {
        // Given: content is a number (not a string)
        let messages = vec![json!({"role": "user", "content": 42})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: text defaults to empty (as_str returns None)
        assert_eq!(result[0]["parts"][0]["text"], "");
    }

    #[test]
    fn google_preserves_unicode_content() {
        // Given: a message with unicode content
        let messages = vec![json!({"role": "user", "content": "こんにちは世界 🌍"})];
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: unicode preserved
        assert_eq!(result[0]["parts"][0]["text"], "こんにちは世界 🌍");
    }

    #[test]
    fn google_long_conversation() {
        // Given: a long conversation
        let messages: Vec<Value> = (0..100)
            .map(|i| {
                json!({
                    "role": if i % 2 == 0 { "user" } else { "assistant" },
                    "content": format!("Message {}", i)
                })
            })
            .collect();
        // When: mapped
        let result = map_openai_messages_to_google_contents(&messages);
        // Then: all messages mapped
        assert_eq!(result.len(), 100);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[1]["role"], "model");
    }

    // ========================================================================
    // 追加テスト: map_openai_messages_to_anthropic edge cases
    // ========================================================================

    #[test]
    fn anthropic_unknown_role_maps_to_user() {
        // Given: a message with an unknown role
        let messages = vec![json!({"role": "tool", "content": "result"})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: mapped as user
        assert_eq!(regular[0]["role"], "user");
    }

    #[test]
    fn anthropic_content_number_defaults_to_empty() {
        // Given: content is a number (not a string)
        let messages = vec![json!({"role": "user", "content": 42})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: text defaults to empty
        assert_eq!(regular[0]["content"][0]["text"], "");
    }

    #[test]
    fn anthropic_system_with_empty_content() {
        // Given: a system message with empty content
        let messages = vec![
            json!({"role": "system", "content": ""}),
            json!({"role": "user", "content": "Hi"}),
        ];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system is Some("") (empty string, but not None)
        assert_eq!(system, Some("".to_string()));
        assert_eq!(regular.len(), 1);
    }

    #[test]
    fn anthropic_preserves_unicode() {
        // Given: unicode content
        let messages = vec![json!({"role": "user", "content": "日本語テスト"})];
        // When: mapped
        let (_system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: unicode preserved
        assert_eq!(regular[0]["content"][0]["text"], "日本語テスト");
    }

    #[test]
    fn anthropic_multiple_system_messages_with_user() {
        // Given: multiple system messages interspersed with regular messages
        let messages = vec![
            json!({"role": "system", "content": "Rule A"}),
            json!({"role": "user", "content": "Q1"}),
            json!({"role": "system", "content": "Rule B"}),
            json!({"role": "assistant", "content": "A1"}),
        ];
        // When: mapped
        let (system, regular) = map_openai_messages_to_anthropic(&messages);
        // Then: system messages joined, regular messages in order
        assert_eq!(system.unwrap(), "Rule A\nRule B");
        assert_eq!(regular.len(), 2);
        assert_eq!(regular[0]["role"], "user");
        assert_eq!(regular[1]["role"], "assistant");
    }

    // ========================================================================
    // 追加テスト: openai_error_response edge cases
    // ========================================================================

    #[tokio::test]
    async fn openai_error_response_500() {
        // Given: a 500 error
        let resp = openai_error_response("internal error", StatusCode::INTERNAL_SERVER_ERROR);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: status is 500
        assert_eq!(parts.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], 500);
    }

    #[tokio::test]
    async fn openai_error_response_empty_message() {
        // Given: an empty error message
        let resp = openai_error_response("", StatusCode::BAD_REQUEST);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: empty message is preserved
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["message"], "");
    }

    #[tokio::test]
    async fn openai_error_response_unicode_message() {
        // Given: a unicode error message
        let resp = openai_error_response("エラーが発生しました", StatusCode::BAD_REQUEST);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: unicode preserved
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["message"], "エラーが発生しました");
    }

    #[tokio::test]
    async fn openai_error_response_string_ownership() {
        // Given: a String (not &str) message
        let msg = String::from("owned string error");
        let resp = openai_error_response(msg, StatusCode::UNAUTHORIZED);
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: works with owned String
        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["message"], "owned string error");
    }

    // ========================================================================
    // 追加テスト: model_unavailable_response edge cases
    // ========================================================================

    #[tokio::test]
    async fn model_unavailable_response_empty_message() {
        // Given: empty message
        let resp = model_unavailable_response("", "empty");
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: empty message preserved, type is service_unavailable
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["message"], "");
        assert_eq!(body["error"]["type"], "service_unavailable");
    }

    #[tokio::test]
    async fn model_unavailable_response_unicode_message() {
        // Given: unicode message
        let resp = model_unavailable_response("モデルが見つかりません", "not_found");
        // When: we inspect the response
        let (_parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: unicode preserved
        assert_eq!(body["error"]["message"], "モデルが見つかりません");
    }

    #[tokio::test]
    async fn model_unavailable_response_string_ownership() {
        // Given: an owned String message
        let msg = String::from("owned message");
        let resp = model_unavailable_response(msg, "test_code");
        // When: we inspect the response
        let (parts, body) = resp.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        // Then: works with owned String
        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["message"], "owned message");
        assert_eq!(body["error"]["code"], "test_code");
    }
}
