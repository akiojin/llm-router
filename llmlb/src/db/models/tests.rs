
use super::*;

async fn create_test_pool() -> SqlitePool {
    crate::db::test_utils::test_db_pool().await
}

#[tokio::test]
async fn test_save_and_load_model() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo {
        name: "test-model".to_string(),
        size: 1000000,
        description: "Test model".to_string(),
        required_memory: 2000000,
        tags: vec!["llm".to_string(), "test".to_string()],
        capabilities: vec![ModelCapability::TextGeneration],
        source: ModelSource::Predefined,
        chat_template: None,
        repo: Some("test/repo".to_string()),
        filename: Some("model.gguf".to_string()),
        last_modified: Some(Utc::now()),
        status: Some("available".to_string()),
    };

    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("test-model").await.unwrap();
    assert!(loaded.is_some());

    let loaded = loaded.unwrap();
    assert_eq!(loaded.name, "test-model");
    assert_eq!(loaded.size, 1000000);
    assert_eq!(loaded.tags, vec!["llm", "test"]);
    assert_eq!(loaded.capabilities, vec![ModelCapability::TextGeneration]);
}

#[tokio::test]
async fn test_load_models() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model1 = ModelInfo::new(
        "model1".to_string(),
        1000,
        "Model 1".to_string(),
        2000,
        vec!["tag1".to_string()],
    );
    let model2 = ModelInfo::new(
        "model2".to_string(),
        2000,
        "Model 2".to_string(),
        4000,
        vec!["tag2".to_string()],
    );

    storage.save_model(&model1).await.unwrap();
    storage.save_model(&model2).await.unwrap();

    let models = storage.load_models().await.unwrap();
    assert_eq!(models.len(), 2);
}

#[tokio::test]
async fn test_load_models_groups_tags_and_capabilities_per_model() {
    // N+1 一括取得版で tags/capabilities がモデル間で混線しないことを固定する
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model1 = ModelInfo::new(
        "m1".to_string(),
        1000,
        "M1".to_string(),
        2000,
        vec!["a".to_string(), "b".to_string()],
    );
    model1.capabilities = vec![ModelCapability::TextGeneration, ModelCapability::Embedding];

    let mut model2 = ModelInfo::new(
        "m2".to_string(),
        2000,
        "M2".to_string(),
        4000,
        vec!["c".to_string()],
    );
    model2.capabilities = vec![ModelCapability::ImageInput];

    // タグも能力も持たないモデル
    let mut model3 = ModelInfo::new("m3".to_string(), 3000, "M3".to_string(), 6000, vec![]);
    model3.capabilities = vec![];

    storage.save_model(&model1).await.unwrap();
    storage.save_model(&model2).await.unwrap();
    storage.save_model(&model3).await.unwrap();

    let models = storage.load_models().await.unwrap();
    assert_eq!(models.len(), 3);

    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let get = |name: &str| models.iter().find(|m| m.name == name).unwrap().clone();

    let cap_set = |caps: &[ModelCapability]| {
        caps.iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
    };

    let m1 = get("m1");
    assert_eq!(
        sorted(m1.tags.clone()),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(
        cap_set(&m1.capabilities),
        cap_set(&[ModelCapability::TextGeneration, ModelCapability::Embedding])
    );

    let m2 = get("m2");
    assert_eq!(m2.tags, vec!["c".to_string()]);
    assert_eq!(m2.capabilities, vec![ModelCapability::ImageInput]);

    let m3 = get("m3");
    assert!(m3.tags.is_empty());
    assert!(m3.capabilities.is_empty());
}

#[tokio::test]
async fn test_delete_model() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo::new(
        "to-delete".to_string(),
        1000,
        "To delete".to_string(),
        2000,
        vec![],
    );

    storage.save_model(&model).await.unwrap();
    assert!(storage.load_model("to-delete").await.unwrap().is_some());

    storage.delete_model("to-delete").await.unwrap();
    assert!(storage.load_model("to-delete").await.unwrap().is_none());
}

#[tokio::test]
async fn test_update_model() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model = ModelInfo::new(
        "updatable".to_string(),
        1000,
        "Original".to_string(),
        2000,
        vec!["original".to_string()],
    );

    storage.save_model(&model).await.unwrap();

    // Update the model
    model.description = "Updated".to_string();
    model.tags = vec!["updated".to_string()];
    model.capabilities = vec![ModelCapability::TextGeneration];

    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("updatable").await.unwrap().unwrap();
    assert_eq!(loaded.description, "Updated");
    assert_eq!(loaded.tags, vec!["updated"]);
    assert_eq!(loaded.capabilities, vec![ModelCapability::TextGeneration]);
}

#[tokio::test]
async fn test_load_model_nonexistent() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);
    let result = storage.load_model("no-such-model").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_model_succeeds() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);
    // Deleting a non-existent model should not error
    storage.delete_model("phantom").await.unwrap();
}

#[tokio::test]
async fn test_save_model_all_sources() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let sources = [
        ("m-predefined", ModelSource::Predefined),
        ("m-hf-gguf", ModelSource::HfGguf),
        ("m-hf-safetensors", ModelSource::HfSafetensors),
        ("m-hf-onnx", ModelSource::HfOnnx),
    ];

    for (name, source) in &sources {
        let mut model = ModelInfo::new(name.to_string(), 100, "desc".to_string(), 200, vec![]);
        model.source = source.clone();
        storage.save_model(&model).await.unwrap();
    }

    let models = storage.load_models().await.unwrap();
    assert_eq!(models.len(), 4);
}

#[tokio::test]
async fn test_save_model_multiple_capabilities() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo {
        name: "multi-cap".to_string(),
        size: 500,
        description: "Multi capability".to_string(),
        required_memory: 1000,
        tags: vec![],
        capabilities: vec![ModelCapability::TextGeneration, ModelCapability::Embedding],
        source: ModelSource::Predefined,
        chat_template: None,
        repo: None,
        filename: None,
        last_modified: None,
        status: None,
    };

    storage.save_model(&model).await.unwrap();
    let loaded = storage.load_model("multi-cap").await.unwrap().unwrap();
    assert_eq!(loaded.capabilities.len(), 2);
    assert!(loaded
        .capabilities
        .contains(&ModelCapability::TextGeneration));
    assert!(loaded.capabilities.contains(&ModelCapability::Embedding));
}

#[tokio::test]
async fn test_save_models_batch() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let models: Vec<ModelInfo> = (0..5)
        .map(|i| {
            ModelInfo::new(
                format!("batch-{}", i),
                100 * (i + 1),
                format!("Model {}", i),
                200 * (i + 1),
                vec![format!("tag-{}", i)],
            )
        })
        .collect();

    storage.save_models(&models).await.unwrap();

    let loaded = storage.load_models().await.unwrap();
    assert_eq!(loaded.len(), 5);
}

#[tokio::test]
async fn test_save_model_with_optional_fields() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo {
        name: "full-model".to_string(),
        size: 999,
        description: "Full".to_string(),
        required_memory: 1998,
        tags: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        capabilities: vec![ModelCapability::TextGeneration],
        source: ModelSource::HfGguf,
        chat_template: Some("chatml".to_string()),
        repo: Some("org/repo".to_string()),
        filename: Some("model-q4.gguf".to_string()),
        last_modified: Some(Utc::now()),
        status: Some("ready".to_string()),
    };

    storage.save_model(&model).await.unwrap();
    let loaded = storage.load_model("full-model").await.unwrap().unwrap();
    assert_eq!(loaded.chat_template, Some("chatml".to_string()));
    assert_eq!(loaded.repo, Some("org/repo".to_string()));
    assert_eq!(loaded.filename, Some("model-q4.gguf".to_string()));
    assert!(loaded.last_modified.is_some());
    assert_eq!(loaded.status, Some("ready".to_string()));
    assert_eq!(loaded.tags.len(), 3);
}

#[tokio::test]
async fn test_upsert_preserves_name() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo::new(
        "upsert-test".to_string(),
        100,
        "v1".to_string(),
        200,
        vec![],
    );
    storage.save_model(&model).await.unwrap();

    let mut model2 = ModelInfo::new(
        "upsert-test".to_string(),
        999,
        "v2".to_string(),
        1998,
        vec!["new-tag".to_string()],
    );
    model2.source = ModelSource::HfSafetensors;
    storage.save_model(&model2).await.unwrap();

    let models = storage.load_models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].description, "v2");
    assert_eq!(models[0].size, 999);
}

#[tokio::test]
async fn test_empty_load_models() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);
    let models = storage.load_models().await.unwrap();
    assert!(models.is_empty());
}

#[tokio::test]
async fn test_save_model_no_tags_no_capabilities() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo {
        name: "bare-model".to_string(),
        size: 100,
        description: "No tags or caps".to_string(),
        required_memory: 200,
        tags: vec![],
        capabilities: vec![],
        source: ModelSource::Predefined,
        chat_template: None,
        repo: None,
        filename: None,
        last_modified: None,
        status: None,
    };

    storage.save_model(&model).await.unwrap();
    let loaded = storage.load_model("bare-model").await.unwrap().unwrap();
    assert!(loaded.tags.is_empty());
    assert!(loaded.capabilities.is_empty());
}

#[tokio::test]
async fn test_upsert_updates_tags_completely() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model = ModelInfo::new(
        "tag-update".to_string(),
        100,
        "desc".to_string(),
        200,
        vec!["old-tag-1".to_string(), "old-tag-2".to_string()],
    );
    storage.save_model(&model).await.unwrap();

    // Update with completely different tags
    model.tags = vec!["new-tag".to_string()];
    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("tag-update").await.unwrap().unwrap();
    assert_eq!(loaded.tags, vec!["new-tag"]);
}

#[tokio::test]
async fn test_upsert_updates_capabilities_completely() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model = ModelInfo::new(
        "cap-update".to_string(),
        100,
        "desc".to_string(),
        200,
        vec![],
    );
    model.capabilities = vec![ModelCapability::TextGeneration, ModelCapability::Embedding];
    storage.save_model(&model).await.unwrap();

    // Replace capabilities
    model.capabilities = vec![ModelCapability::SpeechToText];
    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("cap-update").await.unwrap().unwrap();
    assert_eq!(loaded.capabilities, vec![ModelCapability::SpeechToText]);
}

#[tokio::test]
async fn test_delete_model_removes_tags_and_capabilities() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let model = ModelInfo {
        name: "delete-cascade".to_string(),
        size: 100,
        description: "desc".to_string(),
        required_memory: 200,
        tags: vec!["tag1".to_string()],
        capabilities: vec![ModelCapability::TextGeneration],
        source: ModelSource::Predefined,
        chat_template: None,
        repo: None,
        filename: None,
        last_modified: None,
        status: None,
    };
    storage.save_model(&model).await.unwrap();
    storage.delete_model("delete-cascade").await.unwrap();

    // Re-create with same name should work without conflicts
    let model2 = ModelInfo::new(
        "delete-cascade".to_string(),
        500,
        "new desc".to_string(),
        1000,
        vec!["new-tag".to_string()],
    );
    storage.save_model(&model2).await.unwrap();
    let loaded = storage.load_model("delete-cascade").await.unwrap().unwrap();
    assert_eq!(loaded.size, 500);
    assert_eq!(loaded.tags, vec!["new-tag"]);
}

#[tokio::test]
async fn test_all_model_capabilities() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let all_caps = vec![
        ModelCapability::TextGeneration,
        ModelCapability::TextToSpeech,
        ModelCapability::SpeechToText,
        ModelCapability::ImageGeneration,
        ModelCapability::ImageInput,
        ModelCapability::Embedding,
    ];

    let model = ModelInfo {
        name: "all-caps".to_string(),
        size: 100,
        description: "all capabilities".to_string(),
        required_memory: 200,
        tags: vec![],
        capabilities: all_caps.clone(),
        source: ModelSource::Predefined,
        chat_template: None,
        repo: None,
        filename: None,
        last_modified: None,
        status: None,
    };

    storage.save_model(&model).await.unwrap();
    let loaded = storage.load_model("all-caps").await.unwrap().unwrap();
    assert_eq!(loaded.capabilities.len(), 6);
    for cap in &all_caps {
        assert!(loaded.capabilities.contains(cap));
    }
}

#[tokio::test]
async fn test_save_model_hf_safetensors_source() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model = ModelInfo::new(
        "st-model".to_string(),
        1000,
        "SafeTensors model".to_string(),
        2000,
        vec![],
    );
    model.source = ModelSource::HfSafetensors;
    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("st-model").await.unwrap().unwrap();
    assert_eq!(loaded.source, ModelSource::HfSafetensors);
}

#[tokio::test]
async fn test_save_model_hf_onnx_source() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let mut model = ModelInfo::new(
        "onnx-model".to_string(),
        500,
        "ONNX model".to_string(),
        1000,
        vec![],
    );
    model.source = ModelSource::HfOnnx;
    storage.save_model(&model).await.unwrap();

    let loaded = storage.load_model("onnx-model").await.unwrap().unwrap();
    assert_eq!(loaded.source, ModelSource::HfOnnx);
}

#[tokio::test]
async fn test_model_last_modified_roundtrip() {
    let pool = create_test_pool().await;
    let storage = ModelStorage::new(pool);

    let now = Utc::now();
    let model = ModelInfo {
        name: "time-model".to_string(),
        size: 100,
        description: "desc".to_string(),
        required_memory: 200,
        tags: vec![],
        capabilities: vec![],
        source: ModelSource::Predefined,
        chat_template: None,
        repo: None,
        filename: None,
        last_modified: Some(now),
        status: None,
    };

    storage.save_model(&model).await.unwrap();
    let loaded = storage.load_model("time-model").await.unwrap().unwrap();
    assert!(loaded.last_modified.is_some());
    // Within 1 second tolerance due to RFC3339 precision
    let diff = (loaded.last_modified.unwrap() - now).num_seconds().abs();
    assert!(diff <= 1);
}
