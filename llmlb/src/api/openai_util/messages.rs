//! OpenAI メッセージ配列を Google Generative AI / Anthropic 形式へ変換
//!
//! arch-review [H6]: api/openai_util.rs からメッセージ変換ロジックを分離。

use serde_json::{json, Value};

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
