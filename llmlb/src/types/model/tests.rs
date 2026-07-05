use super::*;

#[test]
fn test_model_type_serialization() {
    assert_eq!(serde_json::to_string(&ModelType::Llm).unwrap(), "\"llm\"");
    assert_eq!(
        serde_json::to_string(&ModelType::Embedding).unwrap(),
        "\"embedding\""
    );
    assert_eq!(
        serde_json::to_string(&ModelType::SpeechToText).unwrap(),
        "\"speech_to_text\""
    );
    assert_eq!(
        serde_json::to_string(&ModelType::TextToSpeech).unwrap(),
        "\"text_to_speech\""
    );
    assert_eq!(
        serde_json::to_string(&ModelType::ImageGeneration).unwrap(),
        "\"image_generation\""
    );
}

#[test]
fn test_model_type_default() {
    let default_type: ModelType = Default::default();
    assert_eq!(default_type, ModelType::Llm);
}

#[test]
fn test_model_type_deserialization() {
    let speech_to_text: ModelType = serde_json::from_str("\"speech_to_text\"").unwrap();
    assert_eq!(speech_to_text, ModelType::SpeechToText);

    let text_to_speech: ModelType = serde_json::from_str("\"text_to_speech\"").unwrap();
    assert_eq!(text_to_speech, ModelType::TextToSpeech);

    let image_gen: ModelType = serde_json::from_str("\"image_generation\"").unwrap();
    assert_eq!(image_gen, ModelType::ImageGeneration);
}

#[test]
fn test_model_capability_serialization() {
    assert_eq!(
        serde_json::to_string(&ModelCapability::TextGeneration).unwrap(),
        "\"text_generation\""
    );
    assert_eq!(
        serde_json::to_string(&ModelCapability::TextToSpeech).unwrap(),
        "\"text_to_speech\""
    );
    assert_eq!(
        serde_json::to_string(&ModelCapability::SpeechToText).unwrap(),
        "\"speech_to_text\""
    );
    assert_eq!(
        serde_json::to_string(&ModelCapability::ImageGeneration).unwrap(),
        "\"image_generation\""
    );
    assert_eq!(
        serde_json::to_string(&ModelCapability::ImageInput).unwrap(),
        "\"image_input\""
    );
    assert_eq!(
        serde_json::to_string(&ModelCapability::Embedding).unwrap(),
        "\"embedding\""
    );
}

#[test]
fn test_model_capability_deserialization() {
    let text_gen: ModelCapability = serde_json::from_str("\"text_generation\"").unwrap();
    assert_eq!(text_gen, ModelCapability::TextGeneration);

    let embedding: ModelCapability = serde_json::from_str("\"embedding\"").unwrap();
    assert_eq!(embedding, ModelCapability::Embedding);
}

#[test]
fn test_model_capability_from_model_type() {
    let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
    assert_eq!(llm_caps, vec![ModelCapability::TextGeneration]);

    let embed_caps = ModelCapability::from_model_type(ModelType::Embedding);
    assert_eq!(embed_caps, vec![ModelCapability::Embedding]);

    let stt_caps = ModelCapability::from_model_type(ModelType::SpeechToText);
    assert_eq!(stt_caps, vec![ModelCapability::SpeechToText]);

    let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
    assert_eq!(tts_caps, vec![ModelCapability::TextToSpeech]);

    let img_caps = ModelCapability::from_model_type(ModelType::ImageGeneration);
    assert_eq!(img_caps, vec![ModelCapability::ImageGeneration]);
}

#[test]
fn test_model_capabilities_from_vec() {
    let llm_caps = vec![ModelCapability::TextGeneration];
    let caps: ModelCapabilities = llm_caps.into();
    assert!(caps.chat_completion);
    assert!(caps.completion);
    assert!(caps.inference);
    assert!(!caps.embeddings);
    assert!(!caps.text_to_speech);
    assert!(!caps.speech_to_text);
    assert!(!caps.image_generation);
    assert!(!caps.image_input);
    assert!(!caps.fine_tune);
}

#[test]
fn test_model_capabilities_serialization() {
    let caps = ModelCapabilities {
        chat_completion: true,
        completion: true,
        embeddings: false,
        fine_tune: false,
        inference: true,
        text_to_speech: false,
        speech_to_text: false,
        image_generation: false,
        image_input: false,
    };

    let json = serde_json::to_string(&caps).unwrap();
    assert!(json.contains("\"chat_completion\":true"));
    assert!(json.contains("\"inference\":true"));

    let deserialized: ModelCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(caps, deserialized);
}

// --- 追加テスト ---

#[test]
fn test_sync_state_serde_roundtrip() {
    for state in [
        SyncState::Idle,
        SyncState::Running,
        SyncState::Success,
        SyncState::Failed,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: SyncState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, state);
    }
}

#[test]
fn test_sync_state_serialization_values() {
    assert_eq!(serde_json::to_string(&SyncState::Idle).unwrap(), "\"idle\"");
    assert_eq!(
        serde_json::to_string(&SyncState::Running).unwrap(),
        "\"running\""
    );
    assert_eq!(
        serde_json::to_string(&SyncState::Success).unwrap(),
        "\"success\""
    );
    assert_eq!(
        serde_json::to_string(&SyncState::Failed).unwrap(),
        "\"failed\""
    );
}

#[test]
fn test_sync_progress_serde_roundtrip() {
    let progress = SyncProgress {
        model_id: "llama3".to_string(),
        file: "model.gguf".to_string(),
        downloaded_bytes: 1024,
        total_bytes: 4096,
    };
    let json = serde_json::to_string(&progress).unwrap();
    let deserialized: SyncProgress = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, progress);
}

#[test]
fn test_model_type_serde_roundtrip() {
    for mt in [
        ModelType::Llm,
        ModelType::Embedding,
        ModelType::SpeechToText,
        ModelType::TextToSpeech,
        ModelType::ImageGeneration,
    ] {
        let json = serde_json::to_string(&mt).unwrap();
        let deserialized: ModelType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, mt);
    }
}

#[test]
fn test_model_capabilities_from_slice() {
    let caps_slice: &[ModelCapability] =
        &[ModelCapability::TextGeneration, ModelCapability::Embedding];
    let caps: ModelCapabilities = ModelCapabilities::from(caps_slice);
    assert!(caps.chat_completion);
    assert!(caps.completion);
    assert!(caps.embeddings);
    assert!(caps.inference);
    assert!(!caps.text_to_speech);
    assert!(!caps.speech_to_text);
    assert!(!caps.image_generation);
    assert!(!caps.image_input);
    assert!(!caps.fine_tune);
}

#[test]
fn test_model_capabilities_from_multimodal() {
    let caps: ModelCapabilities = vec![
        ModelCapability::TextGeneration,
        ModelCapability::TextToSpeech,
        ModelCapability::SpeechToText,
        ModelCapability::ImageGeneration,
    ]
    .into();
    assert!(caps.chat_completion);
    assert!(caps.text_to_speech);
    assert!(caps.speech_to_text);
    assert!(caps.image_generation);
    assert!(!caps.embeddings);
}

#[test]
fn test_model_capabilities_default() {
    let caps: ModelCapabilities = Default::default();
    assert!(!caps.chat_completion);
    assert!(!caps.completion);
    assert!(!caps.embeddings);
    assert!(!caps.inference);
    assert!(!caps.text_to_speech);
    assert!(!caps.speech_to_text);
    assert!(!caps.image_generation);
    assert!(!caps.fine_tune);
}

#[test]
fn test_model_capabilities_serde_roundtrip() {
    let caps = ModelCapabilities {
        chat_completion: true,
        completion: false,
        embeddings: true,
        fine_tune: false,
        inference: true,
        text_to_speech: true,
        speech_to_text: false,
        image_generation: true,
        image_input: true,
    };
    let json = serde_json::to_string(&caps).unwrap();
    let deserialized: ModelCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, caps);
}

#[test]
fn test_model_capability_serde_roundtrip() {
    for cap in [
        ModelCapability::TextGeneration,
        ModelCapability::TextToSpeech,
        ModelCapability::SpeechToText,
        ModelCapability::ImageGeneration,
        ModelCapability::ImageInput,
        ModelCapability::Embedding,
    ] {
        let json = serde_json::to_string(&cap).unwrap();
        let deserialized: ModelCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, cap);
    }
}

// ========================================================================
// 追加テスト: SyncState
// ========================================================================

#[test]
fn test_sync_state_deserialization_values() {
    let idle: SyncState = serde_json::from_str("\"idle\"").unwrap();
    assert_eq!(idle, SyncState::Idle);
    let running: SyncState = serde_json::from_str("\"running\"").unwrap();
    assert_eq!(running, SyncState::Running);
    let success: SyncState = serde_json::from_str("\"success\"").unwrap();
    assert_eq!(success, SyncState::Success);
    let failed: SyncState = serde_json::from_str("\"failed\"").unwrap();
    assert_eq!(failed, SyncState::Failed);
}

#[test]
fn test_sync_state_invalid_deserialization_fails() {
    let result = serde_json::from_str::<SyncState>("\"unknown\"");
    assert!(result.is_err());
}

#[test]
fn test_sync_state_debug_format() {
    assert_eq!(format!("{:?}", SyncState::Idle), "Idle");
    assert_eq!(format!("{:?}", SyncState::Running), "Running");
    assert_eq!(format!("{:?}", SyncState::Success), "Success");
    assert_eq!(format!("{:?}", SyncState::Failed), "Failed");
}

// ========================================================================
// 追加テスト: SyncProgress
// ========================================================================

#[test]
fn test_sync_progress_zero_bytes() {
    let progress = SyncProgress {
        model_id: "test".to_string(),
        file: "model.gguf".to_string(),
        downloaded_bytes: 0,
        total_bytes: 0,
    };
    let json = serde_json::to_string(&progress).unwrap();
    let deserialized: SyncProgress = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.downloaded_bytes, 0);
    assert_eq!(deserialized.total_bytes, 0);
}

#[test]
fn test_sync_progress_max_bytes() {
    let progress = SyncProgress {
        model_id: "huge-model".to_string(),
        file: "model.safetensors".to_string(),
        downloaded_bytes: u64::MAX,
        total_bytes: u64::MAX,
    };
    let json = serde_json::to_string(&progress).unwrap();
    let deserialized: SyncProgress = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.downloaded_bytes, u64::MAX);
    assert_eq!(deserialized.total_bytes, u64::MAX);
}

#[test]
fn test_sync_progress_empty_strings() {
    let progress = SyncProgress {
        model_id: "".to_string(),
        file: "".to_string(),
        downloaded_bytes: 0,
        total_bytes: 100,
    };
    let json = serde_json::to_string(&progress).unwrap();
    let deserialized: SyncProgress = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model_id, "");
    assert_eq!(deserialized.file, "");
}

#[test]
fn test_sync_progress_partial_download() {
    let progress = SyncProgress {
        model_id: "llama3-8b".to_string(),
        file: "model-00001-of-00002.safetensors".to_string(),
        downloaded_bytes: 2_000_000_000,
        total_bytes: 4_000_000_000,
    };
    let json = serde_json::to_string(&progress).unwrap();
    assert!(json.contains("\"downloaded_bytes\":2000000000"));
    assert!(json.contains("\"total_bytes\":4000000000"));
}

// ========================================================================
// 追加テスト: ModelType edge cases
// ========================================================================

#[test]
fn test_model_type_invalid_deserialization_fails() {
    let result = serde_json::from_str::<ModelType>("\"unknown_type\"");
    assert!(result.is_err());
}

#[test]
fn test_model_type_debug_format() {
    assert_eq!(format!("{:?}", ModelType::Llm), "Llm");
    assert_eq!(format!("{:?}", ModelType::Embedding), "Embedding");
    assert_eq!(format!("{:?}", ModelType::SpeechToText), "SpeechToText");
    assert_eq!(format!("{:?}", ModelType::TextToSpeech), "TextToSpeech");
    assert_eq!(
        format!("{:?}", ModelType::ImageGeneration),
        "ImageGeneration"
    );
}

#[test]
fn test_model_type_clone() {
    let original = ModelType::Embedding;
    let cloned = original;
    assert_eq!(original, cloned);
}

// ========================================================================
// 追加テスト: ModelCapability edge cases
// ========================================================================

#[test]
fn test_model_capability_invalid_deserialization_fails() {
    let result = serde_json::from_str::<ModelCapability>("\"video_generation\"");
    assert!(result.is_err());
}

#[test]
fn test_model_capability_debug_format() {
    assert_eq!(
        format!("{:?}", ModelCapability::TextGeneration),
        "TextGeneration"
    );
    assert_eq!(format!("{:?}", ModelCapability::Embedding), "Embedding");
}

#[test]
fn test_model_capability_hash_in_set() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(ModelCapability::TextGeneration);
    set.insert(ModelCapability::TextGeneration); // duplicate
    set.insert(ModelCapability::Embedding);
    assert_eq!(set.len(), 2);
    assert!(set.contains(&ModelCapability::TextGeneration));
    assert!(set.contains(&ModelCapability::Embedding));
}

// ========================================================================
// 追加テスト: ModelCapabilities From conversions
// ========================================================================

#[test]
fn test_model_capabilities_from_empty_slice() {
    let caps: ModelCapabilities = ModelCapabilities::from(&[] as &[ModelCapability]);
    assert!(!caps.chat_completion);
    assert!(!caps.completion);
    assert!(!caps.embeddings);
    assert!(caps.inference); // inference is always true
    assert!(!caps.text_to_speech);
    assert!(!caps.speech_to_text);
    assert!(!caps.image_generation);
    assert!(!caps.fine_tune);
}

#[test]
fn test_model_capabilities_from_empty_vec() {
    let caps: ModelCapabilities = Vec::<ModelCapability>::new().into();
    assert!(!caps.chat_completion);
    assert!(caps.inference); // always true
}

#[test]
fn test_model_capabilities_from_all_capabilities() {
    let all = vec![
        ModelCapability::TextGeneration,
        ModelCapability::TextToSpeech,
        ModelCapability::SpeechToText,
        ModelCapability::ImageGeneration,
        ModelCapability::ImageInput,
        ModelCapability::Embedding,
    ];
    let caps: ModelCapabilities = all.into();
    assert!(caps.chat_completion);
    assert!(caps.completion);
    assert!(caps.embeddings);
    assert!(caps.inference);
    assert!(caps.text_to_speech);
    assert!(caps.speech_to_text);
    assert!(caps.image_generation);
    assert!(!caps.fine_tune); // always false
}

#[test]
fn test_model_capabilities_from_single_embedding() {
    let caps: ModelCapabilities = vec![ModelCapability::Embedding].into();
    assert!(!caps.chat_completion);
    assert!(!caps.completion);
    assert!(caps.embeddings);
    assert!(caps.inference);
    assert!(!caps.text_to_speech);
}

#[test]
fn test_model_capabilities_from_single_speech_to_text() {
    let caps: ModelCapabilities = vec![ModelCapability::SpeechToText].into();
    assert!(!caps.chat_completion);
    assert!(caps.speech_to_text);
    assert!(!caps.text_to_speech);
}

#[test]
fn test_model_capabilities_from_single_text_to_speech() {
    let caps: ModelCapabilities = vec![ModelCapability::TextToSpeech].into();
    assert!(caps.text_to_speech);
    assert!(!caps.speech_to_text);
    assert!(!caps.chat_completion);
}

#[test]
fn test_model_capabilities_from_single_image_generation() {
    let caps: ModelCapabilities = vec![ModelCapability::ImageGeneration].into();
    assert!(caps.image_generation);
    assert!(!caps.chat_completion);
    assert!(!caps.embeddings);
}

// ========================================================================
// 追加テスト: ModelCapabilities serde
// ========================================================================

#[test]
fn test_model_capabilities_all_true() {
    let caps = ModelCapabilities {
        chat_completion: true,
        completion: true,
        embeddings: true,
        fine_tune: true,
        inference: true,
        text_to_speech: true,
        speech_to_text: true,
        image_generation: true,
        image_input: true,
    };
    let json = serde_json::to_string(&caps).unwrap();
    let deserialized: ModelCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(caps, deserialized);
}

#[test]
fn test_model_capabilities_all_false() {
    let caps = ModelCapabilities {
        chat_completion: false,
        completion: false,
        embeddings: false,
        fine_tune: false,
        inference: false,
        text_to_speech: false,
        speech_to_text: false,
        image_generation: false,
        image_input: false,
    };
    let json = serde_json::to_string(&caps).unwrap();
    let deserialized: ModelCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(caps, deserialized);
}

#[test]
fn test_model_capabilities_debug_format() {
    let caps = ModelCapabilities::default();
    let debug_str = format!("{:?}", caps);
    assert!(debug_str.contains("ModelCapabilities"));
    assert!(debug_str.contains("chat_completion"));
}
