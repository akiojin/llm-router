//! モデル関連型定義
//!
//! モデルタイプ、モデル能力などの定義

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// モデルのソース種別
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    /// 事前定義モデル
    #[default]
    Predefined,
    /// HFのGGUFモデル
    HfGguf,
    /// HFのsafetensorsモデル
    HfSafetensors,
    /// HFのONNXモデル（Whisper等）
    HfOnnx,
}

/// LLM runtimeモデル情報
///
/// ドメインの中核エンティティ。永続化層(db)からドメインサービス層(registry)への
/// 逆依存を避けるため、データ型は最下層の types/ に置く。registry::models は
/// これを re-export する。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// モデル名（例: "gpt-oss-20b", "llama3.2"）
    pub name: String,
    /// モデルサイズ（バイト）
    pub size: u64,
    /// モデルの説明
    pub description: String,
    /// 必要なGPUメモリ（バイト）
    pub required_memory: u64,
    /// タグ（例: ["tools", "thinking"]）
    pub tags: Vec<String>,
    /// モデルの能力（対応するAPI）
    /// 未設定の場合はModelType::Llm（テキスト生成）として扱う
    #[serde(default)]
    pub capabilities: Vec<ModelCapability>,
    /// ソース種別
    #[serde(default)]
    pub source: ModelSource,
    /// 外部から提供されるchat_template（GGUFに含まれない場合の補助）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template: Option<String>,
    /// HFリポジトリ名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// HFファイル名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// 最終更新
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<DateTime<Utc>>,
    /// ステータス（available/registered等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl ModelInfo {
    /// 新しいModelInfoを作成
    ///
    /// capabilities が空の場合は、デフォルトで TextGeneration を設定
    pub fn new(
        name: String,
        size: u64,
        description: String,
        required_memory: u64,
        tags: Vec<String>,
    ) -> Self {
        Self {
            name,
            size,
            description,
            required_memory,
            tags,
            // デフォルトは TextGeneration（LLMモデル）
            capabilities: vec![ModelCapability::TextGeneration],
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        }
    }

    /// 指定した capabilities で新しい ModelInfo を作成
    pub fn with_capabilities(
        name: String,
        size: u64,
        description: String,
        required_memory: u64,
        tags: Vec<String>,
        capabilities: Vec<ModelCapability>,
    ) -> Self {
        Self {
            name,
            size,
            description,
            required_memory,
            tags,
            capabilities,
            source: ModelSource::Predefined,
            chat_template: None,
            repo: None,
            filename: None,
            last_modified: None,
            status: None,
        }
    }

    /// 必要メモリをMB単位で取得
    pub fn required_memory_mb(&self) -> u64 {
        self.required_memory / (1024 * 1024)
    }

    /// 必要メモリをGB単位で取得
    pub fn required_memory_gb(&self) -> f64 {
        self.required_memory as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// モデルが指定した capability をサポートしているか確認
    ///
    /// capabilities が空の場合は TextGeneration をサポートしているとみなす（後方互換性）
    pub fn has_capability(&self, capability: ModelCapability) -> bool {
        if self.capabilities.is_empty() {
            // 後方互換性: capabilities 未設定のモデルは TextGeneration のみサポート
            capability == ModelCapability::TextGeneration
        } else {
            self.capabilities.contains(&capability)
        }
    }

    /// モデルの capabilities を取得
    ///
    /// capabilities が空の場合は TextGeneration のみを返す（後方互換性）
    pub fn get_capabilities(&self) -> Vec<ModelCapability> {
        if self.capabilities.is_empty() {
            vec![ModelCapability::TextGeneration]
        } else {
            self.capabilities.clone()
        }
    }
}

/// モデル同期状態
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    /// 同期待機
    Idle,
    /// 同期中
    Running,
    /// 同期成功
    Success,
    /// 同期失敗
    Failed,
}

/// モデル同期の進捗情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncProgress {
    /// 対象モデルID
    pub model_id: String,
    /// 対象ファイル名
    pub file: String,
    /// ダウンロード済みバイト数
    pub downloaded_bytes: u64,
    /// 総バイト数
    pub total_bytes: u64,
}

/// モデルタイプ
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    /// 言語モデル（デフォルト）
    #[default]
    Llm,
    /// Embeddingモデル
    Embedding,
    /// 音声認識モデル (ASR: Speech-to-Text)
    #[serde(rename = "speech_to_text")]
    SpeechToText,
    /// 音声合成モデル (TTS: Text-to-Speech)
    #[serde(rename = "text_to_speech")]
    TextToSpeech,
    /// 画像生成モデル (Text-to-Image)
    #[serde(rename = "image_generation")]
    ImageGeneration,
}

/// モデルの能力（対応するAPI）
///
/// モデルが対応する API エンドポイントを表す。
/// 1つのモデルが複数の能力を持つ場合がある（例: GPT-4o は TextGeneration + TextToSpeech）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    /// テキスト生成 (/v1/chat/completions, /v1/completions)
    TextGeneration,
    /// 音声合成 (/v1/audio/speech)
    TextToSpeech,
    /// 音声認識 (/v1/audio/transcriptions)
    SpeechToText,
    /// 画像生成 (/v1/images/generations)
    ImageGeneration,
    /// 画像入力/視覚理解（/v1/chat/completions の image_url 等）
    ImageInput,
    /// 埋め込み生成 (/v1/embeddings)
    Embedding,
}

impl ModelCapability {
    /// ModelType から推定されるデフォルトの capabilities を返す
    pub fn from_model_type(model_type: ModelType) -> Vec<Self> {
        match model_type {
            ModelType::Llm => vec![Self::TextGeneration],
            ModelType::Embedding => vec![Self::Embedding],
            ModelType::SpeechToText => vec![Self::SpeechToText],
            ModelType::TextToSpeech => vec![Self::TextToSpeech],
            ModelType::ImageGeneration => vec![Self::ImageGeneration],
        }
    }

    /// この capability が対応する endpoint 層の [`crate::types::endpoint::SupportedAPI`] を返す。
    ///
    /// arch-review [M13]: ModelCapability→SupportedAPI の対応表が呼び出し側に
    /// インライン展開されていたため、型付き変換としてここへ集約する（自由文字列を
    /// 経由しない直接変換）。音声系(TextToSpeech/SpeechToText)は SupportedAPI に
    /// 対応する種別が無いため空を返す。
    pub fn supported_apis(&self) -> Vec<crate::types::endpoint::SupportedAPI> {
        use crate::types::endpoint::SupportedAPI;
        match self {
            Self::TextGeneration => vec![SupportedAPI::ChatCompletions, SupportedAPI::Responses],
            Self::Embedding => vec![SupportedAPI::Embeddings],
            Self::ImageInput => vec![SupportedAPI::ImageInput],
            Self::ImageGeneration => vec![SupportedAPI::ImageGeneration],
            Self::TextToSpeech | Self::SpeechToText => Vec::new(),
        }
    }
}

/// モデルの能力（Azure OpenAI 形式）
///
/// arch-review [M13]: 本 struct は Azure OpenAI 互換の boolean object 形式で、
/// `/v1/models` レスポンス**専用のワイヤ表現**である。内部ロジックでは列挙型の
/// [`ModelCapability`] を正準とし、本 struct への変換は表示直前に局所化する。
///
/// Azure OpenAI API 互換の boolean object 形式で capabilities を表現。
/// `/v1/models` レスポンスで使用。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    /// チャット補完対応 (/v1/chat/completions)
    pub chat_completion: bool,
    /// テキスト補完対応 (/v1/completions)
    pub completion: bool,
    /// 埋め込み生成対応 (/v1/embeddings)
    pub embeddings: bool,
    /// ファインチューニング対応（未実装）
    pub fine_tune: bool,
    /// 推論対応（常に true）
    pub inference: bool,
    /// 音声合成対応 (/v1/audio/speech)
    pub text_to_speech: bool,
    /// 音声認識対応 (/v1/audio/transcriptions)
    pub speech_to_text: bool,
    /// 画像生成対応 (/v1/images/generations)
    pub image_generation: bool,
    /// 画像入力対応（chat request 内の image_url 等）
    #[serde(default)]
    pub image_input: bool,
}

impl From<&[ModelCapability]> for ModelCapabilities {
    fn from(caps: &[ModelCapability]) -> Self {
        ModelCapabilities {
            chat_completion: caps.contains(&ModelCapability::TextGeneration),
            completion: caps.contains(&ModelCapability::TextGeneration),
            embeddings: caps.contains(&ModelCapability::Embedding),
            inference: true, // 全モデル対応
            text_to_speech: caps.contains(&ModelCapability::TextToSpeech),
            speech_to_text: caps.contains(&ModelCapability::SpeechToText),
            image_generation: caps.contains(&ModelCapability::ImageGeneration),
            image_input: caps.contains(&ModelCapability::ImageInput),
            fine_tune: false, // 未対応
        }
    }
}

impl From<Vec<ModelCapability>> for ModelCapabilities {
    fn from(caps: Vec<ModelCapability>) -> Self {
        ModelCapabilities::from(caps.as_slice())
    }
}

#[cfg(test)]
mod tests {
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
}
