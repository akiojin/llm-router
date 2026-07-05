//! モデル関連型定義
//!
//! モデルタイプ、モデル能力などの定義
//!
//! # モデル識別子のフィールド命名規約（arch-review [L22]）
//!
//! コード上に `model` / `model_id` / `model_name` の 3 系統が併存するが、これは
//! 不整合ではなく、それぞれ別の外部契約に固定された意図的な使い分けである。
//! いずれかへ統一すると下記の契約が破壊されるため、名称は据え置きとする。
//!
//! - `model`: OpenAI 互換プロトコル（`common/protocol.rs`）およびダウンロード
//!   リクエスト（`DownloadModelRequest` 等）の **wire フィールド名**。OpenAI API
//!   仕様がフィールド名 `model` を要求するため変更不可。
//! - `model_id`: エンドポイントで発見済みのモデルやカタログモデルを参照する
//!   **内部識別子**（`EndpointModel`, `EndpointDailyStats` 等）。
//! - `model_name`: 監査ログ（`audit_log` テーブルの列 `model_name`）へ永続化する
//!   フィールド。列名変更には DB マイグレーションを要するため据え置き。
//! - `gpu_model_name`: エンドポイント健全性メトリクス（`HealthMetrics`）の wire
//!   フィールドで、エンドポイントが報告する GPU 名。プロトコル互換のため据え置き。
//!
//! # Capability 系型の分類（arch-review [M13]）
//!
//! 能力（capability）を表す型が 5 つ併存するが、これは重複ではなく、それぞれ別の
//! バウンデッドコンテキストと wire 表現に属する意図的な区別である。単一型へ統合すると
//! 下記の異なる wire 文字列・ドメイン境界が壊れるため、統合せず、変換が必要な箇所は
//! [`ModelCapability::supported_apis`] のような明示的な橋渡しメソッドで接続する。
//!
//! - [`ModelCapability`]（本モジュール）: モデル自身が持つ能力
//!   （`text_generation` / `embedding` / `image_input` 等）。`ModelType` から導出。
//! - `ModelCapabilities`（本モジュール, struct）: Azure 互換の wire 専用フラグ集合。
//! - `crate::types::endpoint::EndpointCapability`: エンドポイントが広告する機能
//!   （wire 文字列 `chat_completion` / `audio_transcription` 等）。
//! - `crate::types::endpoint::SupportedAPI`: 実際に叩ける API ルート面
//!   （`chat_completions` / `responses` / `embeddings` 等）。ルーティング用。
//! - `crate::sync::capabilities::Capability`: モデル同期時の判定用
//!   （wire 文字列 `chat` / `embeddings` / `image_input`）。
//!
//! 変換例: `ModelCapability::TextGeneration` は `supported_apis()` を介して
//! `SupportedAPI::{ChatCompletions, Responses}` へ写像される。

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
mod tests;
