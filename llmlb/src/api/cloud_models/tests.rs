use super::*;

#[test]
fn test_parse_openai_models() {
    let response = OpenAIModelsResponse {
        data: vec![
            OpenAIModel {
                id: "gpt-4o".to_string(),
                object: "model".to_string(),
                created: 1704067200,
                owned_by: "openai".to_string(),
            },
            OpenAIModel {
                id: "gpt-3.5-turbo".to_string(),
                object: "model".to_string(),
                created: 1677649963,
                owned_by: "openai-internal".to_string(),
            },
        ],
    };

    let models = parse_openai_models(&response);
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "openai:gpt-4o");
    assert_eq!(models[0].owned_by, "openai");
    assert_eq!(models[0].created, 1704067200);
    assert_eq!(models[1].id, "openai:gpt-3.5-turbo");
}

#[test]
fn test_parse_google_models() {
    let response = GoogleModelsResponse {
        models: vec![
            GoogleModel {
                name: "models/gemini-2.0-flash".to_string(),
                display_name: Some("Gemini 2.0 Flash".to_string()),
            },
            GoogleModel {
                name: "models/gemini-1.5-pro".to_string(),
                display_name: None,
            },
        ],
    };

    let models = parse_google_models(&response);
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "google:gemini-2.0-flash");
    assert_eq!(models[0].owned_by, "google");
    assert_eq!(models[1].id, "google:gemini-1.5-pro");
}

#[test]
fn test_parse_anthropic_models() {
    let response = AnthropicModelsResponse {
        data: vec![AnthropicModel {
            id: "claude-sonnet-4-20250514".to_string(),
            model_type: "model".to_string(),
            display_name: Some("Claude Sonnet 4".to_string()),
            created_at: "2025-05-14T00:00:00Z".to_string(),
        }],
    };

    let models = parse_anthropic_models(&response);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "anthropic:claude-sonnet-4-20250514");
    assert_eq!(models[0].owned_by, "anthropic");
    // ISO 8601 → Unixタイムスタンプ変換を検証
    assert!(models[0].created > 0);
}

#[test]
fn test_cache_is_valid() {
    let cache = CloudModelsCache::new(vec![]);
    assert!(cache.is_valid());
}

#[test]
fn test_constants() {
    assert_eq!(CLOUD_MODELS_CACHE_TTL_SECS, 86400); // 24時間
    assert_eq!(CLOUD_MODELS_FETCH_TIMEOUT_SECS, 10); // 10秒
}

// --- additional coverage tests ---

#[test]
fn test_parse_openai_models_empty() {
    let response = OpenAIModelsResponse { data: vec![] };
    let models = parse_openai_models(&response);
    assert!(models.is_empty());
}

#[test]
fn test_parse_google_models_empty() {
    let response = GoogleModelsResponse { models: vec![] };
    let models = parse_google_models(&response);
    assert!(models.is_empty());
}

#[test]
fn test_parse_anthropic_models_empty() {
    let response = AnthropicModelsResponse { data: vec![] };
    let models = parse_anthropic_models(&response);
    assert!(models.is_empty());
}

#[test]
fn test_parse_google_models_without_prefix() {
    let response = GoogleModelsResponse {
        models: vec![GoogleModel {
            name: "gemini-flash".to_string(), // no "models/" prefix
            display_name: Some("Flash".to_string()),
        }],
    };
    let models = parse_google_models(&response);
    assert_eq!(models.len(), 1);
    // Without "models/" prefix, the name should be used as-is
    assert_eq!(models[0].id, "google:gemini-flash");
}

#[test]
fn test_parse_anthropic_models_invalid_date() {
    let response = AnthropicModelsResponse {
        data: vec![AnthropicModel {
            id: "claude-test".to_string(),
            model_type: "model".to_string(),
            display_name: None,
            created_at: "not-a-date".to_string(), // invalid ISO 8601
        }],
    };
    let models = parse_anthropic_models(&response);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "anthropic:claude-test");
    // Invalid date falls back to 0
    assert_eq!(models[0].created, 0);
}

#[test]
fn test_cloud_model_info_serialization() {
    let model = CloudModelInfo {
        id: "openai:gpt-4o".to_string(),
        object: "model".to_string(),
        created: 1704067200,
        owned_by: "openai".to_string(),
    };
    let json = serde_json::to_value(&model).unwrap();
    assert_eq!(json["id"], "openai:gpt-4o");
    assert_eq!(json["object"], "model");
    assert_eq!(json["created"], 1704067200);
    assert_eq!(json["owned_by"], "openai");
}

#[test]
fn test_cloud_model_info_deserialization() {
    let json_str = r#"{"id":"google:gemini-pro","object":"model","created":0,"owned_by":"google"}"#;
    let model: CloudModelInfo = serde_json::from_str(json_str).unwrap();
    assert_eq!(model.id, "google:gemini-pro");
    assert_eq!(model.owned_by, "google");
}

#[test]
fn test_cloud_models_cache_new() {
    let models = vec![
        CloudModelInfo {
            id: "openai:gpt-4o".to_string(),
            object: "model".to_string(),
            created: 1704067200,
            owned_by: "openai".to_string(),
        },
        CloudModelInfo {
            id: "google:gemini".to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "google".to_string(),
        },
    ];
    let cache = CloudModelsCache::new(models.clone());
    assert_eq!(cache.models.len(), 2);
    assert!(cache.is_valid());
}

#[test]
fn test_cloud_models_cache_expired() {
    let cache = CloudModelsCache {
        models: vec![],
        fetched_at: Utc::now() - chrono::Duration::seconds(CLOUD_MODELS_CACHE_TTL_SECS as i64 + 1),
    };
    assert!(!cache.is_valid());
}

#[test]
fn test_cloud_models_cache_just_created_is_valid() {
    let cache = CloudModelsCache {
        models: vec![],
        fetched_at: Utc::now(),
    };
    assert!(cache.is_valid());
}

#[test]
fn test_parse_openai_models_preserves_created_timestamp() {
    let response = OpenAIModelsResponse {
        data: vec![OpenAIModel {
            id: "model-a".to_string(),
            object: "model".to_string(),
            created: 1234567890,
            owned_by: "test-owner".to_string(),
        }],
    };
    let models = parse_openai_models(&response);
    assert_eq!(models[0].created, 1234567890);
    assert_eq!(models[0].object, "model");
}

#[test]
fn test_parse_google_models_created_is_always_zero() {
    let response = GoogleModelsResponse {
        models: vec![GoogleModel {
            name: "models/gemini-2.0-flash".to_string(),
            display_name: Some("Flash".to_string()),
        }],
    };
    let models = parse_google_models(&response);
    // Google doesn't provide creation timestamp
    assert_eq!(models[0].created, 0);
}

#[test]
fn test_parse_anthropic_models_valid_rfc3339() {
    let response = AnthropicModelsResponse {
        data: vec![AnthropicModel {
            id: "claude-3-opus".to_string(),
            model_type: "model".to_string(),
            display_name: Some("Claude 3 Opus".to_string()),
            created_at: "2024-02-29T12:00:00+00:00".to_string(),
        }],
    };
    let models = parse_anthropic_models(&response);
    assert_eq!(models[0].id, "anthropic:claude-3-opus");
    assert!(models[0].created > 0);
}

#[test]
fn test_openai_model_deserialization() {
    let json_str = r#"{"id":"gpt-4o","object":"model","created":1704067200,"owned_by":"openai"}"#;
    let model: OpenAIModel = serde_json::from_str(json_str).unwrap();
    assert_eq!(model.id, "gpt-4o");
    assert_eq!(model.created, 1704067200);
}

#[test]
fn test_google_model_deserialization() {
    let json_str = r#"{"name":"models/gemini-pro","displayName":"Gemini Pro"}"#;
    let model: GoogleModel = serde_json::from_str(json_str).unwrap();
    assert_eq!(model.name, "models/gemini-pro");
    assert_eq!(model.display_name, Some("Gemini Pro".to_string()));
}

#[test]
fn test_google_model_deserialization_no_display_name() {
    let json_str = r#"{"name":"models/gemini-nano"}"#;
    let model: GoogleModel = serde_json::from_str(json_str).unwrap();
    assert_eq!(model.name, "models/gemini-nano");
    assert_eq!(model.display_name, None);
}

#[test]
fn test_anthropic_model_deserialization() {
    let json_str = r#"{"id":"claude-3-haiku","type":"model","display_name":"Claude 3 Haiku","created_at":"2024-03-07T00:00:00Z"}"#;
    let model: AnthropicModel = serde_json::from_str(json_str).unwrap();
    assert_eq!(model.id, "claude-3-haiku");
    assert_eq!(model.model_type, "model");
    assert_eq!(model.display_name, Some("Claude 3 Haiku".to_string()));
}

#[test]
fn test_openai_models_response_deserialization() {
    let json_str = r#"{"data":[{"id":"gpt-4o","object":"model","created":1704067200,"owned_by":"openai"},{"id":"gpt-3.5-turbo","object":"model","created":1677649963,"owned_by":"openai-internal"}]}"#;
    let response: OpenAIModelsResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(response.data.len(), 2);
}

#[test]
fn test_google_models_response_deserialization() {
    let json_str = r#"{"models":[{"name":"models/gemini-pro"},{"name":"models/gemini-flash","displayName":"Gemini Flash"}]}"#;
    let response: GoogleModelsResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(response.models.len(), 2);
}

#[test]
fn test_anthropic_models_response_deserialization() {
    let json_str = r#"{"data":[{"id":"claude-3-opus","type":"model","display_name":"Opus","created_at":"2024-02-29T00:00:00Z"}]}"#;
    let response: AnthropicModelsResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(response.data.len(), 1);
}

#[test]
fn test_cloud_model_info_clone() {
    let model = CloudModelInfo {
        id: "openai:gpt-4o".to_string(),
        object: "model".to_string(),
        created: 100,
        owned_by: "openai".to_string(),
    };
    let cloned = model.clone();
    assert_eq!(cloned.id, model.id);
    assert_eq!(cloned.created, model.created);
}

#[test]
fn test_parse_openai_models_owned_by_normalized() {
    let response = OpenAIModelsResponse {
        data: vec![OpenAIModel {
            id: "custom-model".to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "user-org-abc123".to_string(), // original owned_by varies
        }],
    };
    let models = parse_openai_models(&response);
    // parse_openai_models normalizes owned_by to "openai"
    assert_eq!(models[0].owned_by, "openai");
}

// --- resolve_cloud_models_update tests (arch-review [M12]) ---

fn sample_cloud_model(id: &str) -> super::CloudModelInfo {
    super::CloudModelInfo {
        id: id.to_string(),
        object: "model".to_string(),
        created: 0,
        owned_by: "openai".to_string(),
    }
}

#[test]
fn resolve_cloud_models_update_stores_fetched_on_success() {
    let fetched = vec![sample_cloud_model("openai:gpt-4o")];
    let update = super::resolve_cloud_models_update(None, fetched);
    assert_eq!(update.returned.len(), 1);
    assert_eq!(update.returned[0].id, "openai:gpt-4o");
    let stored = update.to_store.expect("成功時は保存する");
    assert_eq!(stored[0].id, "openai:gpt-4o");
}

#[test]
fn resolve_cloud_models_update_falls_back_to_stale_on_empty_fetch() {
    let old = vec![sample_cloud_model("openai:gpt-4o")];
    let update = super::resolve_cloud_models_update(Some(old), vec![]);
    assert_eq!(update.returned.len(), 1, "空取得時は旧キャッシュを返す");
    assert_eq!(update.returned[0].id, "openai:gpt-4o");
    assert!(
        update.to_store.is_none(),
        "stale フォールバック時は保存しない"
    );
}

#[test]
fn resolve_cloud_models_update_stores_empty_when_no_old_cache() {
    let update = super::resolve_cloud_models_update(None, vec![]);
    assert!(update.returned.is_empty());
    assert!(
        update.to_store.map(|m| m.is_empty()).unwrap_or(false),
        "旧キャッシュ無しなら空を保存する"
    );
}
