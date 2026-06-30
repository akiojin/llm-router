//! クラウドプロバイダプロキシ（CloudProvider trait + 各プロバイダ実装）
//!
//! OpenAI、Google Generative AI、Anthropic へのリクエスト転送を
//! 共通の CloudProvider trait で抽象化する。

use crate::api::error::AppError;
use crate::api::openai_util::{
    map_openai_messages_to_anthropic, map_openai_messages_to_google_contents,
};
use crate::api::proxy::forward_streaming_response;
use crate::cloud_metrics;
use crate::common::error::LbError;
use axum::body::Body;
use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tracing::info;
use uuid::Uuid;

/// 非ストリーミングのクラウド転送に適用する全体タイムアウト（秒）。
///
/// 共有 http_client には全体タイムアウトが無いため、応答しないクラウドプロバイダで
/// 無期限ハングするのを防ぐ。ストリーミング経路にはストリームが途中で切れるため付与しない。
const CLOUD_PROXY_TIMEOUT_SECS: u64 = 300;

/// クラウドプロバイダへのプロキシ結果
pub struct CloudProxyResult {
    /// HTTPレスポンス
    pub response: Response,
    /// パース済みレスポンスボディ（非ストリーム成功時のみ）
    pub response_body: Option<Value>,
    /// HTTPステータスコード
    pub status: StatusCode,
    /// エラーメッセージ（失敗時のみ）
    pub error_message: Option<String>,
}

/// クラウドプロバイダの抽象化 trait
pub trait CloudProvider: Send + Sync {
    /// プロバイダ名（ログ・メトリクス用）
    fn provider_name(&self) -> &str;

    /// APIベースURLを返す
    fn api_base_url(&self) -> Result<String, AppError>;

    /// 認証ヘッダーを構築し、リクエストビルダーに適用する
    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, AppError>;

    /// OpenAI形式のペイロードをプロバイダ固有の形式に変換し、
    /// リクエストURLとボディを返す
    fn transform_request(
        &self,
        payload: &Value,
        model: &str,
        stream: bool,
    ) -> Result<(String, Value), AppError>;

    /// プロバイダ固有のレスポンスをOpenAI互換形式に変換する
    /// 非ストリーム成功レスポンスのみ呼ばれる
    fn transform_response(&self, data: &Value, model: &str) -> Value;
}

/// ジェネリックなクラウドプロバイダプロキシ関数
pub async fn proxy_cloud_provider(
    provider: &dyn CloudProvider,
    http_client: &reqwest::Client,
    payload: &Value,
    model: &str,
    stream: bool,
) -> Result<CloudProxyResult, AppError> {
    let req_id = Uuid::new_v4();
    let started = Instant::now();
    let provider_name = provider.provider_name();

    let (url, body) = provider.transform_request(payload, model, stream)?;

    let builder = http_client.post(&url).json(&body);
    let mut builder = provider.apply_auth(builder)?;
    // 非ストリーミングは有界レスポンスのため全体タイムアウトを付与する。
    // ストリーミングは connect_timeout（共有 client）のみで担保し、
    // 全体タイムアウトでストリームが途中で切れるのを避ける。
    if !stream {
        builder = builder.timeout(Duration::from_secs(CLOUD_PROXY_TIMEOUT_SECS));
    }
    let res = builder.send().await.map_err(map_reqwest_error)?;

    if stream {
        info!(
            provider = provider_name,
            model = model,
            request_id = %req_id,
            latency_ms = started.elapsed().as_millis(),
            stream = true,
            status = %res.status(),
            "cloud proxy stream ({})", provider_name
        );
        cloud_metrics::record(
            provider_name,
            res.status().as_u16(),
            started.elapsed().as_millis(),
        );
        let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let response = forward_streaming_response(res).map_err(AppError::from)?;
        return Ok(CloudProxyResult {
            response,
            response_body: None,
            status,
            error_message: if status.is_success() {
                None
            } else {
                Some(status.to_string())
            },
        });
    }

    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let ct = res.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let bytes = res.bytes().await.map_err(map_reqwest_error)?;
    let parsed_data = serde_json::from_slice::<Value>(&bytes).ok();

    let error_message = if status.is_success() {
        None
    } else {
        Some(String::from_utf8_lossy(&bytes).trim().to_string())
    };

    // 成功時のみレスポンス変換を適用
    let (response, response_body) = if status.is_success() {
        if let Some(ref data) = parsed_data {
            let transformed = provider.transform_response(data, model);
            let built = Response::builder()
                .status(status)
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(Body::from(transformed.to_string()))
                .expect("Response builder should not fail with valid status and bytes body");
            (built, Some(transformed))
        } else {
            // パースできなかった場合はそのまま返す
            let mut resp = Response::builder().status(status);
            if let Some(ct) = ct {
                if let Ok(hv) = HeaderValue::from_str(ct.to_str().unwrap_or("")) {
                    resp = resp.header(CONTENT_TYPE, hv);
                }
            }
            let built = resp
                .body(Body::from(bytes))
                .expect("Response builder should not fail with valid status and bytes body");
            (built, None)
        }
    } else {
        // エラー時はそのまま返す
        let mut resp = Response::builder().status(status);
        if let Some(ct) = ct {
            if let Ok(hv) = HeaderValue::from_str(ct.to_str().unwrap_or("")) {
                resp = resp.header(CONTENT_TYPE, hv);
            }
        }
        let built = resp
            .body(Body::from(bytes))
            .expect("Response builder should not fail with valid status and bytes body");
        (built, None)
    };

    info!(
        provider = provider_name,
        model = model,
        request_id = %req_id,
        latency_ms = started.elapsed().as_millis(),
        stream = false,
        status = %status,
        "cloud proxy complete ({})", provider_name
    );
    cloud_metrics::record(
        provider_name,
        status.as_u16(),
        started.elapsed().as_millis(),
    );

    Ok(CloudProxyResult {
        response,
        response_body,
        status,
        error_message,
    })
}

fn map_reqwest_error(err: reqwest::Error) -> AppError {
    AppError::from(LbError::Http(err.to_string()))
}

fn auth_error(msg: &str) -> AppError {
    AppError::from(LbError::Authentication(msg.to_string()))
}

fn get_required_key(provider: &str, env_key: &str, err_msg: &str) -> Result<String, AppError> {
    match std::env::var(env_key) {
        Ok(v) => {
            info!(provider = provider, key = env_key, "cloud api key present");
            Ok(v)
        }
        Err(_) => {
            tracing::warn!(provider = provider, key = env_key, "cloud api key missing");
            Err(auth_error(err_msg))
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAI プロバイダ
// ---------------------------------------------------------------------------

/// OpenAI APIプロバイダ
pub struct OpenAiProvider;

impl CloudProvider for OpenAiProvider {
    fn provider_name(&self) -> &str {
        "openai"
    }

    fn api_base_url(&self) -> Result<String, AppError> {
        Ok(std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".into()))
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, AppError> {
        let api_key = get_required_key(
            "openai",
            "OPENAI_API_KEY",
            "OPENAI_API_KEY is required for openai: models",
        )?;
        Ok(builder.bearer_auth(api_key))
    }

    fn transform_request(
        &self,
        payload: &Value,
        model: &str,
        _stream: bool,
    ) -> Result<(String, Value), AppError> {
        let base = self.api_base_url()?;
        let url = format!("{base}/v1/chat/completions");
        let mut body = payload.clone();
        body["model"] = Value::String(model.to_string());
        Ok((url, body))
    }

    fn transform_response(&self, data: &Value, _model: &str) -> Value {
        // OpenAIはそのまま返す
        data.clone()
    }
}

// ---------------------------------------------------------------------------
// Google プロバイダ
// ---------------------------------------------------------------------------

/// Google Generative AIプロバイダ
pub struct GoogleProvider;

fn map_generation_config(payload: &Value) -> Value {
    json!({
        "temperature": payload.get("temperature").and_then(|v| v.as_f64()),
        "topP": payload.get("top_p").and_then(|v| v.as_f64()),
        "maxOutputTokens": payload.get("max_tokens").and_then(|v| v.as_i64()),
    })
}

impl CloudProvider for GoogleProvider {
    fn provider_name(&self) -> &str {
        "google"
    }

    fn api_base_url(&self) -> Result<String, AppError> {
        Ok(std::env::var("GOOGLE_API_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".into()))
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, AppError> {
        let api_key = get_required_key(
            "google",
            "GOOGLE_API_KEY",
            "GOOGLE_API_KEY is required for google: models",
        )?;
        Ok(builder.query(&[("key", api_key)]))
    }

    fn transform_request(
        &self,
        payload: &Value,
        model: &str,
        stream: bool,
    ) -> Result<(String, Value), AppError> {
        let base = self.api_base_url()?;
        let messages = payload
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        let contents = map_openai_messages_to_google_contents(&messages);
        let mut body = json!({
            "contents": contents,
            "generationConfig": map_generation_config(payload),
        });
        if let Some(gen) = body["generationConfig"].as_object_mut() {
            gen.retain(|_, v| !v.is_null());
        }

        let endpoint_suffix = if stream {
            format!("models/{model}:streamGenerateContent")
        } else {
            format!("models/{model}:generateContent")
        };
        let url = format!("{base}/{endpoint_suffix}");

        Ok((url, body))
    }

    fn transform_response(&self, data: &Value, model: &str) -> Value {
        let text = data
            .get("candidates")
            .and_then(|c: &Value| c.get(0))
            .and_then(|c: &Value| c.get("content"))
            .and_then(|c: &Value| c.get("parts"))
            .and_then(|p: &Value| p.get(0))
            .and_then(|p: &Value| p.get("text"))
            .and_then(|t: &Value| t.as_str())
            .unwrap_or("");

        json!({
            "id": format!("google-{}", Uuid::new_v4()),
            "object": "chat.completion",
            "model": format!("google:{model}"),
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": text},
                "finish_reason": "stop"
            }],
        })
    }
}

// ---------------------------------------------------------------------------
// Anthropic プロバイダ
// ---------------------------------------------------------------------------

/// Anthropic APIプロバイダ
pub struct AnthropicProvider;

impl CloudProvider for AnthropicProvider {
    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn api_base_url(&self) -> Result<String, AppError> {
        Ok(std::env::var("ANTHROPIC_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".into()))
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, AppError> {
        let api_key = get_required_key(
            "anthropic",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_API_KEY is required for anthropic: models",
        )?;
        Ok(builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"))
    }

    fn transform_request(
        &self,
        payload: &Value,
        model: &str,
        stream: bool,
    ) -> Result<(String, Value), AppError> {
        let base = self.api_base_url()?;
        let messages = payload
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        let (system, mapped) = map_openai_messages_to_anthropic(&messages);
        let max_tokens = payload
            .get("max_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(1024);
        let mut body = json!({
            "model": model,
            "messages": mapped,
            "max_tokens": max_tokens,
            "stream": stream,
            "temperature": payload.get("temperature").and_then(|v| v.as_f64()),
            "top_p": payload.get("top_p").and_then(|v| v.as_f64()),
        });
        if let Some(s) = system {
            body["system"] = Value::String(s);
        }
        if let Some(obj) = body.as_object_mut() {
            obj.retain(|_, v| !v.is_null());
        }

        let url = format!("{base}/v1/messages");
        Ok((url, body))
    }

    fn transform_response(&self, data: &Value, model: &str) -> Value {
        let text = data
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|p: &Value| p.get("text"))
            .and_then(|t: &Value| t.as_str())
            .unwrap_or("");

        let id = data
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("anthropic-{}", Uuid::new_v4()));
        let model_label = data
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| model.to_string());

        json!({
            "id": id,
            "object": "chat.completion",
            "model": format!("anthropic:{}", model_label),
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": text},
                "finish_reason": "stop"
            }],
        })
    }
}

/// プロバイダ名からCloudProvider実装を返す
pub fn resolve_provider(provider_name: &str) -> Option<Box<dyn CloudProvider>> {
    match provider_name {
        "openai" => Some(Box::new(OpenAiProvider)),
        "google" => Some(Box::new(GoogleProvider)),
        "anthropic" => Some(Box::new(AnthropicProvider)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn openai_provider_transform_request_sets_model() {
        let provider = OpenAiProvider;
        std::env::set_var("OPENAI_BASE_URL", "http://localhost:1234");
        let payload = serde_json::json!({"model":"ignored","messages":[]});
        let (url, body) = provider
            .transform_request(&payload, "gpt-4o", false)
            .expect("transform");
        assert_eq!(url, "http://localhost:1234/v1/chat/completions");
        assert_eq!(body["model"].as_str().unwrap(), "gpt-4o");
        std::env::remove_var("OPENAI_BASE_URL");
    }

    #[test]
    #[serial]
    fn google_provider_transform_request_builds_correct_url() {
        let provider = GoogleProvider;
        std::env::set_var("GOOGLE_API_BASE_URL", "http://localhost:5678");
        let payload =
            serde_json::json!({"messages":[{"role":"user","content":"hi"}],"temperature":0.5});
        let (url, body) = provider
            .transform_request(&payload, "gemini-pro", false)
            .expect("transform");
        assert_eq!(
            url,
            "http://localhost:5678/models/gemini-pro:generateContent"
        );
        assert!(body.get("contents").is_some());
        std::env::remove_var("GOOGLE_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn google_provider_stream_url() {
        let provider = GoogleProvider;
        std::env::set_var("GOOGLE_API_BASE_URL", "http://localhost:5678");
        let payload = serde_json::json!({"messages":[]});
        let (url, _) = provider
            .transform_request(&payload, "gemini-pro", true)
            .expect("transform");
        assert_eq!(
            url,
            "http://localhost:5678/models/gemini-pro:streamGenerateContent"
        );
        std::env::remove_var("GOOGLE_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn anthropic_provider_transform_request_separates_system() {
        let provider = AnthropicProvider;
        std::env::set_var("ANTHROPIC_API_BASE_URL", "http://localhost:9999");
        let payload = serde_json::json!({
            "messages": [
                {"role":"system","content":"You are helpful"},
                {"role":"user","content":"Hi"}
            ],
            "max_tokens": 512
        });
        let (url, body) = provider
            .transform_request(&payload, "claude-3", false)
            .expect("transform");
        assert_eq!(url, "http://localhost:9999/v1/messages");
        assert_eq!(body["system"].as_str().unwrap(), "You are helpful");
        assert_eq!(body["model"].as_str().unwrap(), "claude-3");
        assert_eq!(body["max_tokens"].as_i64().unwrap(), 512);
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
    }

    #[test]
    fn google_provider_transform_response_maps_to_openai() {
        let provider = GoogleProvider;
        let data = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "hello from gemini"}]}}]
        });
        let result = provider.transform_response(&data, "gemini-pro");
        assert_eq!(result["model"].as_str().unwrap(), "google:gemini-pro");
        assert_eq!(
            result["choices"][0]["message"]["content"].as_str().unwrap(),
            "hello from gemini"
        );
    }

    #[test]
    fn anthropic_provider_transform_response_maps_to_openai() {
        let provider = AnthropicProvider;
        let data = serde_json::json!({
            "id": "msg-abc",
            "model": "claude-3-sonnet",
            "content": [{"text": "anthropic says hi"}]
        });
        let result = provider.transform_response(&data, "claude-3");
        assert_eq!(
            result["model"].as_str().unwrap(),
            "anthropic:claude-3-sonnet"
        );
        assert_eq!(
            result["choices"][0]["message"]["content"].as_str().unwrap(),
            "anthropic says hi"
        );
        assert_eq!(result["id"].as_str().unwrap(), "msg-abc");
    }

    #[test]
    fn openai_provider_transform_response_passthrough() {
        let provider = OpenAiProvider;
        let data =
            serde_json::json!({"id":"chatcmpl-123","choices":[{"message":{"content":"ok"}}]});
        let result = provider.transform_response(&data, "gpt-4o");
        assert_eq!(result, data);
    }

    #[test]
    fn resolve_provider_returns_correct_types() {
        assert!(resolve_provider("openai").is_some());
        assert!(resolve_provider("google").is_some());
        assert!(resolve_provider("anthropic").is_some());
        assert!(resolve_provider("unknown").is_none());
    }

    // --- additional coverage tests ---

    #[test]
    fn resolve_provider_names_match() {
        let openai = resolve_provider("openai").unwrap();
        assert_eq!(openai.provider_name(), "openai");

        let google = resolve_provider("google").unwrap();
        assert_eq!(google.provider_name(), "google");

        let anthropic = resolve_provider("anthropic").unwrap();
        assert_eq!(anthropic.provider_name(), "anthropic");
    }

    #[test]
    fn resolve_provider_empty_string_returns_none() {
        assert!(resolve_provider("").is_none());
    }

    #[test]
    fn resolve_provider_case_sensitive() {
        assert!(resolve_provider("OpenAI").is_none());
        assert!(resolve_provider("Google").is_none());
        assert!(resolve_provider("Anthropic").is_none());
    }

    #[test]
    #[serial]
    fn openai_provider_preserves_messages() {
        let provider = OpenAiProvider;
        std::env::set_var("OPENAI_BASE_URL", "http://localhost:1234");
        let payload = serde_json::json!({
            "model": "ignored",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello"}
            ],
            "temperature": 0.7,
            "stream": false
        });
        let (_url, body) = provider
            .transform_request(&payload, "gpt-4o-mini", false)
            .expect("transform");
        assert_eq!(body["model"].as_str().unwrap(), "gpt-4o-mini");
        // Messages should be preserved
        assert!(body["messages"].is_array());
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        // Temperature should be preserved
        assert_eq!(body["temperature"].as_f64(), Some(0.7));
        std::env::remove_var("OPENAI_BASE_URL");
    }

    #[test]
    #[serial]
    fn google_provider_generation_config() {
        let provider = GoogleProvider;
        std::env::set_var("GOOGLE_API_BASE_URL", "http://localhost:5678");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "test"}],
            "temperature": 0.5,
            "top_p": 0.9,
            "max_tokens": 100
        });
        let (_url, body) = provider
            .transform_request(&payload, "gemini-pro", false)
            .expect("transform");
        let gen_config = &body["generationConfig"];
        assert_eq!(gen_config["temperature"].as_f64(), Some(0.5));
        assert_eq!(gen_config["topP"].as_f64(), Some(0.9));
        assert_eq!(gen_config["maxOutputTokens"].as_i64(), Some(100));
        std::env::remove_var("GOOGLE_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn google_provider_generation_config_strips_nulls() {
        let provider = GoogleProvider;
        std::env::set_var("GOOGLE_API_BASE_URL", "http://localhost:5678");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "test"}]
            // no temperature, top_p, or max_tokens
        });
        let (_url, body) = provider
            .transform_request(&payload, "gemini-pro", false)
            .expect("transform");
        let gen_config = &body["generationConfig"];
        // Null values should be stripped
        assert!(!gen_config.as_object().unwrap().contains_key("temperature"));
        assert!(!gen_config.as_object().unwrap().contains_key("topP"));
        assert!(!gen_config
            .as_object()
            .unwrap()
            .contains_key("maxOutputTokens"));
        std::env::remove_var("GOOGLE_API_BASE_URL");
    }

    #[test]
    fn google_provider_transform_response_empty_candidates() {
        let provider = GoogleProvider;
        let data = serde_json::json!({
            "candidates": []
        });
        let result = provider.transform_response(&data, "gemini-pro");
        // Should handle gracefully with empty text
        assert_eq!(
            result["choices"][0]["message"]["content"].as_str().unwrap(),
            ""
        );
    }

    #[test]
    fn google_provider_transform_response_missing_candidates() {
        let provider = GoogleProvider;
        let data = serde_json::json!({});
        let result = provider.transform_response(&data, "gemini-pro");
        assert_eq!(
            result["choices"][0]["message"]["content"].as_str().unwrap(),
            ""
        );
        assert_eq!(result["object"].as_str().unwrap(), "chat.completion");
    }

    #[test]
    #[serial]
    fn anthropic_provider_default_max_tokens() {
        let provider = AnthropicProvider;
        std::env::set_var("ANTHROPIC_API_BASE_URL", "http://localhost:9999");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}]
            // no max_tokens specified
        });
        let (_url, body) = provider
            .transform_request(&payload, "claude-3", false)
            .expect("transform");
        // Default max_tokens should be 1024
        assert_eq!(body["max_tokens"].as_i64().unwrap(), 1024);
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn anthropic_provider_strips_null_values() {
        let provider = AnthropicProvider;
        std::env::set_var("ANTHROPIC_API_BASE_URL", "http://localhost:9999");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 256
            // no temperature or top_p
        });
        let (_url, body) = provider
            .transform_request(&payload, "claude-3", false)
            .expect("transform");
        // Null values should be stripped
        assert!(!body.as_object().unwrap().contains_key("temperature"));
        assert!(!body.as_object().unwrap().contains_key("top_p"));
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn anthropic_provider_no_system_message() {
        let provider = AnthropicProvider;
        std::env::set_var("ANTHROPIC_API_BASE_URL", "http://localhost:9999");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100
        });
        let (_url, body) = provider
            .transform_request(&payload, "claude-3", false)
            .expect("transform");
        // No system message means no "system" key
        assert!(!body.as_object().unwrap().contains_key("system"));
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
    }

    #[test]
    #[serial]
    fn anthropic_provider_stream_flag() {
        let provider = AnthropicProvider;
        std::env::set_var("ANTHROPIC_API_BASE_URL", "http://localhost:9999");
        let payload = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 100
        });
        let (_url, body) = provider
            .transform_request(&payload, "claude-3", true)
            .expect("transform");
        assert_eq!(body["stream"].as_bool(), Some(true));

        let (_url, body) = provider
            .transform_request(&payload, "claude-3", false)
            .expect("transform");
        assert_eq!(body["stream"].as_bool(), Some(false));
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
    }

    #[test]
    fn anthropic_provider_transform_response_missing_content() {
        let provider = AnthropicProvider;
        let data = serde_json::json!({
            "id": "msg-empty",
            "content": []
        });
        let result = provider.transform_response(&data, "claude-3");
        assert_eq!(
            result["choices"][0]["message"]["content"].as_str().unwrap(),
            ""
        );
        assert_eq!(result["id"].as_str().unwrap(), "msg-empty");
    }

    #[test]
    fn anthropic_provider_transform_response_no_id_or_model() {
        let provider = AnthropicProvider;
        let data = serde_json::json!({
            "content": [{"text": "hello"}]
        });
        let result = provider.transform_response(&data, "claude-3");
        // Should generate fallback id
        assert!(result["id"].as_str().unwrap().starts_with("anthropic-"));
        // Should use the provided model param
        assert_eq!(result["model"].as_str().unwrap(), "anthropic:claude-3");
    }

    #[test]
    fn map_generation_config_all_present() {
        let payload = serde_json::json!({
            "temperature": 0.8,
            "top_p": 0.95,
            "max_tokens": 2048
        });
        let config = map_generation_config(&payload);
        assert_eq!(config["temperature"].as_f64(), Some(0.8));
        assert_eq!(config["topP"].as_f64(), Some(0.95));
        assert_eq!(config["maxOutputTokens"].as_i64(), Some(2048));
    }

    #[test]
    fn map_generation_config_all_absent() {
        let payload = serde_json::json!({});
        let config = map_generation_config(&payload);
        assert!(config["temperature"].is_null());
        assert!(config["topP"].is_null());
        assert!(config["maxOutputTokens"].is_null());
    }

    #[test]
    #[serial]
    fn openai_provider_api_base_url_default() {
        std::env::remove_var("OPENAI_BASE_URL");
        let provider = OpenAiProvider;
        let url = provider.api_base_url().unwrap();
        assert_eq!(url, "https://api.openai.com");
    }

    #[test]
    #[serial]
    fn google_provider_api_base_url_default() {
        std::env::remove_var("GOOGLE_API_BASE_URL");
        let provider = GoogleProvider;
        let url = provider.api_base_url().unwrap();
        assert!(url.contains("generativelanguage.googleapis.com"));
    }

    #[test]
    #[serial]
    fn anthropic_provider_api_base_url_default() {
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
        let provider = AnthropicProvider;
        let url = provider.api_base_url().unwrap();
        assert_eq!(url, "https://api.anthropic.com");
    }
}
