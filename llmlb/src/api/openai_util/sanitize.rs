//! 履歴保存用に OpenAI ペイロードの base64/data-URL をリダクト
//!
//! arch-review [H6]: api/openai_util.rs からサニタイズロジックを分離。

use serde_json::Value;

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
