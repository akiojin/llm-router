use super::*;

#[tokio::test]
async fn search_cache_invalidation_clears_entries() {
    // arch-review [L1]: クリアフックが検索キャッシュを確実に空にすることを検証。
    get_search_cache().write().await.push(CacheEntry {
        key: "query:10".to_string(),
        response: SearchResponse { models: Vec::new() },
        fetched_at: Utc::now(),
    });
    assert!(!get_search_cache().read().await.is_empty());
    invalidate_search_cache().await;
    assert!(get_search_cache().read().await.is_empty());
}

#[test]
fn test_build_engine_names_known_model() {
    let names = build_engine_names("openai/gpt-oss-20b");
    assert_eq!(names.ollama, Some("gpt-oss:20b".to_string()));
    assert_eq!(names.lm_studio, Some("openai/gpt-oss-20b".to_string()));
    assert_eq!(names.xllm, None);
    assert_eq!(names.vllm, None);
}

#[test]
fn test_build_engine_names_unknown_model() {
    let names = build_engine_names("unknown/model-123");
    assert_eq!(names.ollama, None);
    assert_eq!(names.lm_studio, None);
    assert_eq!(names.xllm, None);
    assert_eq!(names.vllm, None);
}

#[test]
fn test_build_supports_download() {
    let download = build_supports_download("openai/gpt-oss-20b");
    assert!(download.contains(&"xllm".to_string()));
    assert!(download.contains(&"ollama".to_string()));
    assert!(download.contains(&"lm_studio".to_string()));
    assert!(!download.contains(&"openai_compatible".to_string()));
}

#[test]
fn test_build_supports_download_includes_new_lm_studio_aliases() {
    let download = build_supports_download("google/gemma-3-27b-it");
    assert!(download.contains(&"xllm".to_string()));
    assert!(download.contains(&"ollama".to_string()));
    assert!(download.contains(&"lm_studio".to_string()));
}

#[test]
fn test_build_supports_download_unknown_model_only_lists_xllm() {
    let download = build_supports_download("unknown/model-123");
    assert_eq!(download, vec!["xllm".to_string()]);
}

#[test]
fn test_can_recommend_download_requires_ollama_alias() {
    assert!(can_recommend_download(
        EndpointType::Ollama,
        Some("gpt-oss:20b")
    ));
    assert!(!can_recommend_download(EndpointType::Ollama, None));
}

#[test]
fn test_can_recommend_download_allows_xllm_without_alias() {
    assert!(can_recommend_download(EndpointType::Xllm, None));
}

#[test]
fn test_can_recommend_download_allows_lm_studio_without_alias() {
    assert!(can_recommend_download(
        EndpointType::LmStudio,
        Some("openai/gpt-oss-20b")
    ));
    assert!(can_recommend_download(EndpointType::LmStudio, None));
}

#[test]
fn test_resolve_engine_model_ids_includes_all_lm_studio_aliases() {
    let ids = resolve_engine_model_ids("Qwen/Qwen3.5-35B-A3B", EndpointType::LmStudio);
    assert_eq!(
        ids,
        vec![
            "qwen3.5-35b-a3b".to_string(),
            "qwen/qwen3.5-35b-a3b".to_string(),
            "qwen/qwen3.5-35b-a3b:2".to_string()
        ]
    );
}

#[test]
fn test_endpoint_has_model_matches_secondary_alias() {
    let engine_model_ids = resolve_engine_model_ids("Qwen/Qwen3.5-35B-A3B", EndpointType::LmStudio);
    assert!(endpoint_has_model(
        "qwen/qwen3.5-35b-a3b:2",
        "Qwen/Qwen3.5-35B-A3B",
        &engine_model_ids
    ));
}

#[test]
fn test_to_catalog_model() {
    let hf = HfModelInfo {
        model_id: Some("openai/gpt-oss-20b".to_string()),
        tags: vec!["text-generation".to_string(), "gguf".to_string()],
        downloads: 50000,
        siblings: vec![],
        description: Some("A test model".to_string()),
        pipeline_tag: Some("text-generation".to_string()),
    };

    let model = to_catalog_model(&hf);
    assert_eq!(model.repo_id, "openai/gpt-oss-20b");
    assert_eq!(model.downloads, 50000);
    assert_eq!(model.description, Some("A test model".to_string()));
    assert_eq!(model.tags.len(), 2);
    assert_eq!(model.engine_names.ollama, Some("gpt-oss:20b".to_string()));
    assert_eq!(
        model.engine_names.lm_studio,
        Some("openai/gpt-oss-20b".to_string())
    );
    assert_eq!(
        model.supports_download,
        vec![
            "xllm".to_string(),
            "ollama".to_string(),
            "lm_studio".to_string()
        ]
    );
}

#[test]
fn test_to_catalog_model_missing_id() {
    let hf = HfModelInfo {
        model_id: None,
        tags: vec![],
        downloads: 0,
        siblings: vec![],
        description: None,
        pipeline_tag: None,
    };

    let model = to_catalog_model(&hf);
    assert_eq!(model.repo_id, "");
}

#[test]
fn test_search_response_serialization() {
    let response = SearchResponse {
        models: vec![CatalogModel {
            repo_id: "test/model".to_string(),
            description: Some("desc".to_string()),
            downloads: 100,
            tags: vec!["gguf".to_string()],
            engine_names: EngineNames {
                ollama: None,
                lm_studio: None,
                xllm: None,
                vllm: None,
            },
            supports_download: vec!["xllm".to_string()],
        }],
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["models"][0]["repo_id"], "test/model");
    assert_eq!(json["models"][0]["downloads"], 100);
    assert_eq!(json["models"][0]["supports_download"][0], "xllm");
}

#[test]
fn test_search_response_deserialization() {
    let json_str = r#"{"models":[{"repo_id":"test/model","description":"desc","downloads":100,"tags":["gguf"],"engine_names":{"ollama":null,"lm_studio":null,"xllm":null,"vllm":null},"supports_download":["xllm"]}]}"#;
    let response: SearchResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(response.models.len(), 1);
    assert_eq!(response.models[0].repo_id, "test/model");
}

#[test]
fn test_hf_model_info_deserialization() {
    let json_str = r#"{
            "modelId": "TheBloke/Llama-2-7B-GGUF",
            "tags": ["text-generation", "gguf"],
            "downloads": 123456,
            "siblings": [{"rfilename": "llama-2-7b.Q4_K_M.gguf"}],
            "description": "Llama 2 7B GGUF",
            "pipeline_tag": "text-generation"
        }"#;
    let info: HfModelInfo = serde_json::from_str(json_str).unwrap();
    assert_eq!(info.model_id, Some("TheBloke/Llama-2-7B-GGUF".to_string()));
    assert_eq!(info.downloads, 123456);
    assert_eq!(info.siblings.len(), 1);
    assert_eq!(info.siblings[0].rfilename, "llama-2-7b.Q4_K_M.gguf");
}

#[test]
fn test_hf_model_info_deserialization_minimal() {
    let json_str = r#"{"tags":[]}"#;
    let info: HfModelInfo = serde_json::from_str(json_str).unwrap();
    assert_eq!(info.model_id, None);
    assert_eq!(info.downloads, 0);
    assert!(info.siblings.is_empty());
}

#[test]
fn test_engine_names_equality() {
    let a = EngineNames {
        ollama: Some("model:7b".to_string()),
        lm_studio: None,
        xllm: None,
        vllm: None,
    };
    let b = EngineNames {
        ollama: Some("model:7b".to_string()),
        lm_studio: None,
        xllm: None,
        vllm: None,
    };
    assert_eq!(a, b);
}

#[test]
fn test_model_detail_response_serialization() {
    let detail = ModelDetailResponse {
        repo_id: "test/model".to_string(),
        tags: vec!["gguf".to_string()],
        downloads: 999,
        description: Some("A model".to_string()),
        pipeline_tag: Some("text-generation".to_string()),
        siblings: vec![HfSibling {
            rfilename: "model.gguf".to_string(),
        }],
        engine_names: EngineNames {
            ollama: None,
            lm_studio: None,
            xllm: None,
            vllm: None,
        },
        supports_download: vec!["xllm".to_string()],
    };

    let json = serde_json::to_value(&detail).unwrap();
    assert_eq!(json["repo_id"], "test/model");
    assert_eq!(json["downloads"], 999);
    assert_eq!(json["siblings"][0]["rfilename"], "model.gguf");
}

#[test]
fn test_recommended_endpoint_serialization() {
    let ep = RecommendedEndpoint {
        id: "123".to_string(),
        name: "My Endpoint".to_string(),
        endpoint_type: EndpointType::Ollama,
        can_download: true,
        has_model: false,
    };

    let json = serde_json::to_value(&ep).unwrap();
    assert_eq!(json["id"], "123");
    assert_eq!(json["name"], "My Endpoint");
    assert_eq!(json["endpoint_type"], "ollama");
    assert_eq!(json["can_download"], true);
    assert_eq!(json["has_model"], false);
}

#[test]
fn test_search_query_defaults() {
    let json_str = r#"q=llama"#;
    let query: SearchQuery = serde_urlencoded::from_str(json_str).unwrap();
    assert_eq!(query.q, "llama");
    assert_eq!(query.limit, 20);
}

#[test]
fn test_search_query_with_limit() {
    let json_str = r#"q=llama&limit=5"#;
    let query: SearchQuery = serde_urlencoded::from_str(json_str).unwrap();
    assert_eq!(query.q, "llama");
    assert_eq!(query.limit, 5);
}

#[test]
fn test_cache_entry_validity() {
    let entry = CacheEntry {
        key: "test".to_string(),
        response: SearchResponse { models: vec![] },
        fetched_at: Utc::now(),
    };
    assert!(entry.is_valid());
}

#[test]
fn test_cache_entry_expired() {
    let entry = CacheEntry {
        key: "test".to_string(),
        response: SearchResponse { models: vec![] },
        fetched_at: Utc::now() - chrono::Duration::seconds(CATALOG_CACHE_TTL_SECS + 1),
    };
    assert!(!entry.is_valid());
}

#[test]
fn test_hf_base_url_default() {
    // HF_BASE_URL が未設定の場合はデフォルト値を返す
    // NOTE: テスト環境で HF_BASE_URL が設定されている場合はそちらが返る
    let url = hf_base_url();
    assert!(!url.is_empty());
}

#[test]
fn test_build_engine_names_qwen3() {
    let names = build_engine_names("Qwen/Qwen3-30B");
    assert_eq!(names.ollama, Some("qwen3:30b".to_string()));
    assert_eq!(names.lm_studio, Some("qwen/qwen3-30b-a3b".to_string()));
}

#[test]
fn test_build_engine_names_gemma3() {
    let names = build_engine_names("google/gemma-3-27b-it");
    assert_eq!(names.ollama, Some("gemma3:27b".to_string()));
    assert_eq!(names.lm_studio, Some("google/gemma-3-27b".to_string()));
}
