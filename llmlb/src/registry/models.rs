//! モデル情報管理
//!
//! LLM runtimeモデルのメタデータ管理

// ModelInfo / ModelSource は中核ドメイン型として types/model.rs へ移設した
// （db → registry の逆依存/循環を避けるため）。既存の
// `crate::registry::models::{ModelInfo, ModelSource}` 参照との後方互換のため re-export する。
pub use crate::types::model::{ModelInfo, ModelSource};

// arch-review [L13]: `extract_repo_id` は純粋 util のため共有層 models/mapping.rs へ
// 移設した。既存の `registry::models::extract_repo_id` 参照との後方互換のため re-export する。
pub use crate::models::mapping::extract_repo_id;

/// HuggingFaceリポジトリ名からモデルIDを生成（階層形式）
///
/// SPEC-dcaeaec4 FR-2に準拠:
/// - `openai/gpt-oss-20b` → `openai/gpt-oss-20b`
/// - `TheBloke/Llama-2-7B-GGUF` → `thebloke/llama-2-7b-gguf`
///
/// 正規化ルール:
/// 1. 小文字に変換
/// 2. 先頭・末尾のスラッシュを除去
/// 3. 危険なパターン (`..`, `\0`) は "_latest" に変換
pub fn generate_model_id(repo: &str) -> String {
    if repo.is_empty() {
        return "_latest".into();
    }

    // 危険なパターンをチェック
    if repo.contains("..") || repo.contains('\0') {
        return "_latest".into();
    }

    // 小文字に変換し、先頭・末尾のスラッシュを除去
    let normalized = repo.to_lowercase();
    let trimmed = normalized.trim_matches('/');

    if trimmed.is_empty() {
        "_latest".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ModelCapability;

    // ===== モデルID生成テスト（階層形式） =====

    #[test]
    fn test_generate_model_id_hierarchical() {
        // 階層形式: org/model → org/model (小文字化)
        assert_eq!(
            generate_model_id("TheBloke/Llama-2-7B-GGUF"),
            "thebloke/llama-2-7b-gguf"
        );
    }

    #[test]
    fn test_generate_model_id_with_org() {
        // 組織名付き
        assert_eq!(
            generate_model_id("bartowski/gemma-2-9b-it-GGUF"),
            "bartowski/gemma-2-9b-it-gguf"
        );
    }

    #[test]
    fn test_generate_model_id_simple() {
        // シンプルなリポジトリ名
        assert_eq!(
            generate_model_id("openai/gpt-oss-20b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_generate_model_id_single_name() {
        // 単一名（組織なし）
        assert_eq!(generate_model_id("convertible-repo"), "convertible-repo");
    }

    #[test]
    fn test_generate_model_id_uppercase() {
        // 大文字を含む
        assert_eq!(
            generate_model_id("MistralAI/Mistral-7B-Instruct-v0.2-GGUF"),
            "mistralai/mistral-7b-instruct-v0.2-gguf"
        );
    }

    #[test]
    fn test_generate_model_id_empty() {
        // 空文字列
        assert_eq!(generate_model_id(""), "_latest");
    }

    #[test]
    fn test_generate_model_id_dangerous() {
        // 危険なパターン
        assert_eq!(generate_model_id("../etc/passwd"), "_latest");
        assert_eq!(generate_model_id("model/../other"), "_latest");
    }

    #[test]
    fn test_generate_model_id_trim_slashes() {
        // 先頭・末尾のスラッシュを除去
        assert_eq!(generate_model_id("/openai/gpt-oss/"), "openai/gpt-oss");
    }

    // ===== 既存テスト =====

    #[test]
    fn test_model_info_new() {
        let model = ModelInfo::new(
            "gpt-oss-20b".to_string(),
            10_000_000_000,
            "GPT-OSS 20B model".to_string(),
            16_000_000_000,
            vec!["llm".to_string(), "text".to_string()],
        );

        assert_eq!(model.name, "gpt-oss-20b");
        assert_eq!(model.size, 10_000_000_000);
        assert_eq!(model.required_memory_gb(), 14.901161193847656);
        // デフォルトは TextGeneration
        assert_eq!(model.capabilities, vec![ModelCapability::TextGeneration]);
    }

    // ===== ModelInfo capabilities テスト =====

    #[test]
    fn test_model_info_with_capabilities() {
        let caps = vec![
            ModelCapability::TextGeneration,
            ModelCapability::TextToSpeech,
        ];
        let model = ModelInfo::with_capabilities(
            "gpt-4o".to_string(),
            0,
            "GPT-4o".to_string(),
            0,
            vec![],
            caps.clone(),
        );

        assert_eq!(model.capabilities, caps);
        assert!(model.has_capability(ModelCapability::TextGeneration));
        assert!(model.has_capability(ModelCapability::TextToSpeech));
        assert!(!model.has_capability(ModelCapability::SpeechToText));
    }

    #[test]
    fn test_model_info_has_capability() {
        let model = ModelInfo::with_capabilities(
            "tts-model".to_string(),
            0,
            "TTS Model".to_string(),
            0,
            vec![],
            vec![ModelCapability::TextToSpeech],
        );

        assert!(model.has_capability(ModelCapability::TextToSpeech));
        assert!(!model.has_capability(ModelCapability::TextGeneration));
        assert!(!model.has_capability(ModelCapability::SpeechToText));
    }

    #[test]
    fn test_model_info_has_capability_backward_compat() {
        // capabilities が空の場合は TextGeneration をサポート（後方互換性）
        let mut model = ModelInfo::new(
            "legacy-model".to_string(),
            0,
            "Legacy".to_string(),
            0,
            vec![],
        );
        // 明示的に空にする
        model.capabilities = vec![];

        assert!(model.has_capability(ModelCapability::TextGeneration));
        assert!(!model.has_capability(ModelCapability::TextToSpeech));
    }

    #[test]
    fn test_model_info_get_capabilities() {
        let model = ModelInfo::with_capabilities(
            "multi-model".to_string(),
            0,
            "Multi".to_string(),
            0,
            vec![],
            vec![
                ModelCapability::TextGeneration,
                ModelCapability::TextToSpeech,
            ],
        );

        let caps = model.get_capabilities();
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&ModelCapability::TextGeneration));
        assert!(caps.contains(&ModelCapability::TextToSpeech));
    }

    #[test]
    fn test_model_info_get_capabilities_backward_compat() {
        // capabilities が空の場合は TextGeneration のみを返す（後方互換性）
        let mut model = ModelInfo::new(
            "legacy-model".to_string(),
            0,
            "Legacy".to_string(),
            0,
            vec![],
        );
        model.capabilities = vec![];

        let caps = model.get_capabilities();
        assert_eq!(caps, vec![ModelCapability::TextGeneration]);
    }

    // ===== extract_repo_id テスト =====

    #[test]
    fn test_extract_repo_id_https_url() {
        assert_eq!(
            extract_repo_id("https://huggingface.co/openai/gpt-oss-20b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_extract_repo_id_http_url() {
        assert_eq!(
            extract_repo_id("http://huggingface.co/openai/gpt-oss-20b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_extract_repo_id_with_tree_path() {
        // URLにtree/mainなどが含まれている場合もrepo_idのみを抽出
        assert_eq!(
            extract_repo_id("https://huggingface.co/openai/gpt-oss-20b/tree/main"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_extract_repo_id_www_prefix() {
        assert_eq!(
            extract_repo_id("https://www.huggingface.co/openai/gpt-oss-20b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_extract_repo_id_already_repo_format() {
        // 既にrepo_id形式の場合はそのまま返す
        assert_eq!(extract_repo_id("openai/gpt-oss-20b"), "openai/gpt-oss-20b");
    }

    #[test]
    fn test_extract_repo_id_simple_name() {
        // シンプルな名前の場合はそのまま返す
        assert_eq!(extract_repo_id("gpt-oss-20b"), "gpt-oss-20b");
    }

    // ===== 追加テスト: ModelInfo =====

    #[test]
    fn test_model_info_required_memory_mb() {
        let model = ModelInfo::new(
            "test".to_string(),
            0,
            "test".to_string(),
            1024 * 1024 * 100, // 100 MB
            vec![],
        );
        assert_eq!(model.required_memory_mb(), 100);
    }

    #[test]
    fn test_model_info_required_memory_mb_zero() {
        let model = ModelInfo::new("test".to_string(), 0, "test".to_string(), 0, vec![]);
        assert_eq!(model.required_memory_mb(), 0);
    }

    #[test]
    fn test_model_info_required_memory_gb() {
        let model = ModelInfo::new(
            "test".to_string(),
            0,
            "test".to_string(),
            1024 * 1024 * 1024 * 2, // 2 GB
            vec![],
        );
        let gb = model.required_memory_gb();
        assert!((gb - 2.0).abs() < 0.01, "expected ~2.0 GB, got {gb}");
    }

    #[test]
    fn test_model_info_required_memory_gb_zero() {
        let model = ModelInfo::new("test".to_string(), 0, "test".to_string(), 0, vec![]);
        assert_eq!(model.required_memory_gb(), 0.0);
    }

    #[test]
    fn test_model_info_new_default_capabilities() {
        let model = ModelInfo::new(
            "model".to_string(),
            100,
            "desc".to_string(),
            200,
            vec!["tag1".to_string()],
        );
        assert_eq!(model.capabilities, vec![ModelCapability::TextGeneration]);
        assert_eq!(model.source, ModelSource::Predefined);
        assert!(model.chat_template.is_none());
        assert!(model.repo.is_none());
        assert!(model.filename.is_none());
        assert!(model.last_modified.is_none());
        assert!(model.status.is_none());
    }

    #[test]
    fn test_model_info_with_capabilities_custom() {
        let caps = vec![ModelCapability::SpeechToText, ModelCapability::Embedding];
        let model = ModelInfo::with_capabilities(
            "whisper".to_string(),
            500,
            "Whisper model".to_string(),
            1000,
            vec!["audio".to_string()],
            caps.clone(),
        );
        assert_eq!(model.capabilities, caps);
        assert_eq!(model.source, ModelSource::Predefined);
    }

    #[test]
    fn test_model_info_has_capability_multiple() {
        let model = ModelInfo::with_capabilities(
            "multi".to_string(),
            0,
            "Multi".to_string(),
            0,
            vec![],
            vec![
                ModelCapability::TextGeneration,
                ModelCapability::Embedding,
                ModelCapability::ImageGeneration,
            ],
        );
        assert!(model.has_capability(ModelCapability::TextGeneration));
        assert!(model.has_capability(ModelCapability::Embedding));
        assert!(model.has_capability(ModelCapability::ImageGeneration));
        assert!(!model.has_capability(ModelCapability::SpeechToText));
        assert!(!model.has_capability(ModelCapability::TextToSpeech));
    }

    #[test]
    fn test_model_info_has_capability_single() {
        let model = ModelInfo::with_capabilities(
            "single".to_string(),
            0,
            "Single".to_string(),
            0,
            vec![],
            vec![ModelCapability::ImageGeneration],
        );
        assert!(model.has_capability(ModelCapability::ImageGeneration));
        assert!(!model.has_capability(ModelCapability::TextGeneration));
    }

    #[test]
    fn test_model_info_get_capabilities_empty_backward_compat() {
        let mut model = ModelInfo::new(
            "empty-caps".to_string(),
            0,
            "Empty caps".to_string(),
            0,
            vec![],
        );
        model.capabilities = vec![];
        let caps = model.get_capabilities();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0], ModelCapability::TextGeneration);
    }

    #[test]
    fn test_model_info_get_capabilities_non_empty() {
        let model = ModelInfo::with_capabilities(
            "multi-cap".to_string(),
            0,
            "".to_string(),
            0,
            vec![],
            vec![ModelCapability::TextToSpeech, ModelCapability::SpeechToText],
        );
        let caps = model.get_capabilities();
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&ModelCapability::TextToSpeech));
        assert!(caps.contains(&ModelCapability::SpeechToText));
    }

    // ===== ModelInfo serialization テスト =====

    #[test]
    fn test_model_info_serialize_deserialize() {
        let model = ModelInfo::new(
            "serialize-test".to_string(),
            1000,
            "Serialize Test".to_string(),
            2000,
            vec!["tag1".to_string(), "tag2".to_string()],
        );
        let json = serde_json::to_string(&model).unwrap();
        let deserialized: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }

    #[test]
    fn test_model_info_serialize_with_optional_fields() {
        let mut model = ModelInfo::new("optional-test".to_string(), 0, "".to_string(), 0, vec![]);
        model.chat_template = Some("template".to_string());
        model.repo = Some("org/repo".to_string());
        model.filename = Some("model.gguf".to_string());
        model.status = Some("available".to_string());

        let json = serde_json::to_string(&model).unwrap();
        assert!(json.contains("chat_template"));
        assert!(json.contains("template"));
        assert!(json.contains("repo"));
        assert!(json.contains("filename"));
        assert!(json.contains("status"));

        let deserialized: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.chat_template, Some("template".to_string()));
        assert_eq!(deserialized.repo, Some("org/repo".to_string()));
        assert_eq!(deserialized.filename, Some("model.gguf".to_string()));
        assert_eq!(deserialized.status, Some("available".to_string()));
    }

    #[test]
    fn test_model_info_serialize_skip_none_optional_fields() {
        let model = ModelInfo::new("no-optional".to_string(), 0, "".to_string(), 0, vec![]);
        let json = serde_json::to_string(&model).unwrap();
        // skip_serializing_if = "Option::is_none" のフィールドは含まれない
        assert!(!json.contains("chat_template"));
        assert!(!json.contains("\"repo\""));
        assert!(!json.contains("\"filename\""));
        assert!(!json.contains("last_modified"));
        assert!(!json.contains("\"status\""));
    }

    // ===== ModelSource テスト =====

    #[test]
    fn test_model_source_default() {
        let source: ModelSource = Default::default();
        assert_eq!(source, ModelSource::Predefined);
    }

    #[test]
    fn test_model_source_variants() {
        assert_eq!(ModelSource::Predefined, ModelSource::Predefined);
        assert_ne!(ModelSource::HfGguf, ModelSource::HfSafetensors);
        assert_ne!(ModelSource::HfOnnx, ModelSource::HfGguf);
    }

    #[test]
    fn test_model_source_serialize_deserialize() {
        let sources = vec![
            ModelSource::Predefined,
            ModelSource::HfGguf,
            ModelSource::HfSafetensors,
            ModelSource::HfOnnx,
        ];
        for source in sources {
            let json = serde_json::to_string(&source).unwrap();
            let deserialized: ModelSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, deserialized);
        }
    }

    #[test]
    fn test_model_source_serde_rename() {
        let json = serde_json::to_string(&ModelSource::HfGguf).unwrap();
        assert_eq!(json, "\"hf_gguf\"");

        let json = serde_json::to_string(&ModelSource::HfSafetensors).unwrap();
        assert_eq!(json, "\"hf_safetensors\"");

        let json = serde_json::to_string(&ModelSource::HfOnnx).unwrap();
        assert_eq!(json, "\"hf_onnx\"");

        let json = serde_json::to_string(&ModelSource::Predefined).unwrap();
        assert_eq!(json, "\"predefined\"");
    }

    // ===== ModelInfo PartialEq テスト =====

    #[test]
    fn test_model_info_equality() {
        let model1 = ModelInfo::new(
            "same".to_string(),
            100,
            "desc".to_string(),
            200,
            vec!["tag".to_string()],
        );
        let model2 = ModelInfo::new(
            "same".to_string(),
            100,
            "desc".to_string(),
            200,
            vec!["tag".to_string()],
        );
        assert_eq!(model1, model2);
    }

    #[test]
    fn test_model_info_inequality_name() {
        let model1 = ModelInfo::new("name1".to_string(), 100, "desc".to_string(), 200, vec![]);
        let model2 = ModelInfo::new("name2".to_string(), 100, "desc".to_string(), 200, vec![]);
        assert_ne!(model1, model2);
    }

    #[test]
    fn test_model_info_inequality_size() {
        let model1 = ModelInfo::new("name".to_string(), 100, "desc".to_string(), 200, vec![]);
        let model2 = ModelInfo::new("name".to_string(), 999, "desc".to_string(), 200, vec![]);
        assert_ne!(model1, model2);
    }

    // ===== generate_model_id 追加テスト =====

    #[test]
    fn test_generate_model_id_null_character() {
        assert_eq!(generate_model_id("model\0name"), "_latest");
    }

    #[test]
    fn test_generate_model_id_only_slashes() {
        assert_eq!(generate_model_id("///"), "_latest");
    }

    #[test]
    fn test_generate_model_id_mixed_case() {
        assert_eq!(generate_model_id("MyOrg/My-Model-V2"), "myorg/my-model-v2");
    }

    #[test]
    fn test_generate_model_id_preserves_numbers() {
        assert_eq!(generate_model_id("Meta/Llama-3.2-1B"), "meta/llama-3.2-1b");
    }

    #[test]
    fn test_generate_model_id_preserves_hyphens_underscores() {
        assert_eq!(generate_model_id("org/model-name_v2"), "org/model-name_v2");
    }

    // ===== extract_repo_id 追加テスト =====

    #[test]
    fn test_extract_repo_id_empty_string() {
        assert_eq!(extract_repo_id(""), "");
    }

    #[test]
    fn test_extract_repo_id_www_http_url() {
        assert_eq!(
            extract_repo_id("http://www.huggingface.co/org/repo"),
            "org/repo"
        );
    }

    #[test]
    fn test_extract_repo_id_with_deep_path() {
        // URL with more than 2 path components
        assert_eq!(
            extract_repo_id("https://huggingface.co/org/repo/blob/main/file.gguf"),
            "org/repo"
        );
    }

    #[test]
    fn test_extract_repo_id_single_component_after_url() {
        assert_eq!(
            extract_repo_id("https://huggingface.co/singlename"),
            "singlename"
        );
    }

    #[test]
    fn test_extract_repo_id_trailing_slash_in_url() {
        assert_eq!(
            extract_repo_id("https://huggingface.co/org/repo/"),
            "org/repo"
        );
    }

    // ===== ModelInfo Clone テスト =====

    #[test]
    fn test_model_info_clone() {
        let model = ModelInfo::new(
            "clone-test".to_string(),
            500,
            "Clone Test".to_string(),
            1000,
            vec!["tag".to_string()],
        );
        let cloned = model.clone();
        assert_eq!(model, cloned);
        assert_eq!(cloned.name, "clone-test");
        assert_eq!(cloned.size, 500);
    }

    // ===== ModelInfo with all optional fields =====

    #[test]
    fn test_model_info_full_construction() {
        let mut model = ModelInfo::with_capabilities(
            "full-model".to_string(),
            10_000_000_000,
            "Full model".to_string(),
            16_000_000_000,
            vec!["llm".to_string(), "chat".to_string()],
            vec![ModelCapability::TextGeneration, ModelCapability::Embedding],
        );
        model.source = ModelSource::HfGguf;
        model.chat_template = Some("{% for msg in messages %}...{% endfor %}".to_string());
        model.repo = Some("org/full-model".to_string());
        model.filename = Some("model-q4.gguf".to_string());
        model.status = Some("available".to_string());

        assert_eq!(model.name, "full-model");
        assert_eq!(model.size, 10_000_000_000);
        // 16_000_000_000 / (1024 * 1024) = 15258
        assert_eq!(model.required_memory_mb(), 15258);
        assert!(model.has_capability(ModelCapability::TextGeneration));
        assert!(model.has_capability(ModelCapability::Embedding));
        assert!(!model.has_capability(ModelCapability::SpeechToText));
        assert_eq!(model.source, ModelSource::HfGguf);
        assert!(model.chat_template.is_some());
        assert_eq!(model.repo.as_deref(), Some("org/full-model"));
        assert_eq!(model.filename.as_deref(), Some("model-q4.gguf"));
        assert_eq!(model.status.as_deref(), Some("available"));
    }

    // ===== ModelSource Debug テスト =====

    #[test]
    fn test_model_source_debug() {
        let debug_str = format!("{:?}", ModelSource::HfGguf);
        assert_eq!(debug_str, "HfGguf");
    }
}
