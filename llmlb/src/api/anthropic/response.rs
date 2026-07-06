//! Anthropic Messages API ⇄ OpenAI Chat Completions のレスポンス変換
//!
//! arch-review [M4]: リクエスト変換(translation.rs)に続き、OpenAI のレスポンスを
//! Anthropic Message 形状へ変換する純粋関数群を submodule として切り出した。
//! トランスポート処理から分離され、単体テストしやすい。

use crate::api::proxy::copy_upstream_headers;
use crate::token::TokenUsage;
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Convert OpenAI tool_call format to Anthropic tool_use content block format
fn convert_openai_tool_call_to_anthropic_tool_use(tool_call: &Value) -> Option<Value> {
    let func = tool_call.get("function")?;
    let tool_name = func.get("name").and_then(Value::as_str)?;
    let tool_id = tool_call.get("id").and_then(Value::as_str)?;
    let arguments_str = func
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");

    // Parse arguments JSON - if parsing fails, use empty object as fallback
    let input = serde_json::from_str(arguments_str).unwrap_or_else(|_| Value::Object(Map::new()));

    Some(json!({
        "type": "tool_use",
        "id": tool_id,
        "name": tool_name,
        "input": input
    }))
}

pub(super) fn openai_to_anthropic_message_response(
    body: &Value,
    model: &str,
    usage: &TokenUsage,
) -> Value {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());

    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);

    let message = choice.and_then(|choice| choice.get("message"));

    let mut content = Vec::new();

    // Add text content if present
    let text = extract_openai_response_text(body);
    if !text.is_empty() {
        content.push(json!({
            "type": "text",
            "text": text
        }));
    }

    // Add tool_use blocks if tool_calls are present
    if let Some(tool_calls) = message
        .and_then(|msg| msg.get("tool_calls"))
        .and_then(Value::as_array)
    {
        for tool_call in tool_calls {
            if let Some(tool_use_block) = convert_openai_tool_call_to_anthropic_tool_use(tool_call)
            {
                content.push(tool_use_block);
            }
        }
    }

    // If no content was added, add an empty text block
    if content.is_empty() {
        content.push(json!({
            "type": "text",
            "text": ""
        }));
    }

    let stop_reason = if finish_reason == Some("tool_calls") {
        "tool_use"
    } else {
        finish_reason
            .map(map_finish_reason_to_stop_reason)
            .unwrap_or("end_turn")
    };

    json!({
        "id": body
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple())),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": {
            "input_tokens": usage.input_tokens.unwrap_or(0),
            "output_tokens": usage.output_tokens.unwrap_or(0)
        }
    })
}

pub(super) fn extract_openai_response_text(body: &Value) -> String {
    body.get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    choice
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .unwrap_or_default()
}

pub(super) fn map_finish_reason_to_stop_reason(finish_reason: &str) -> &'static str {
    match finish_reason {
        "length" => "max_tokens",
        "stop" => "end_turn",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

pub(super) fn build_response_from_upstream(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: Bytes,
) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    copy_upstream_headers(response.headers_mut(), headers);
    if !response.headers().contains_key(header::CONTENT_TYPE) {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    response
}
