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
    let data = serde_json::json!({"id":"chatcmpl-123","choices":[{"message":{"content":"ok"}}]});
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
