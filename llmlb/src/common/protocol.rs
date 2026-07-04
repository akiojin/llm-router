//! 通信プロトコル定義
//!
//! OpenAI互換API用のリクエスト/レスポンス型を定義します。

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

use crate::types::media::{AudioFormat, ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};

/// LLM runtimeチャットリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// モデル名
    pub model: String,
    /// メッセージ配列
    pub messages: Vec<ChatMessage>,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}

/// チャットメッセージ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    /// ロール ("user", "assistant", "system")
    pub role: String,
    /// メッセージ内容
    pub content: String,
}

/// Chat Completionsリクエスト
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    /// モデル名
    pub model: String,
    /// メッセージ配列
    pub messages: Vec<ChatMessage>,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
    /// 最大トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Generateリクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// モデル名
    pub model: String,
    /// プロンプト
    pub prompt: String,
    /// ストリーミング有効化
    #[serde(default)]
    pub stream: bool,
}

/// リクエスト/レスポンスレコード
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResponseRecord {
    /// レコードの一意識別子
    pub id: Uuid,
    /// リクエスト受信時刻
    pub timestamp: DateTime<Utc>,
    /// リクエストタイプ（Chat または Generate）
    pub request_type: RequestType,
    /// 使用されたモデル名
    pub model: String,
    /// 処理したエンドポイントのID
    pub endpoint_id: Uuid,
    /// エンドポイント名
    pub endpoint_name: String,
    /// エンドポイントのIPアドレス
    pub endpoint_ip: IpAddr,
    /// リクエスト元クライアントのIPアドレス
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<IpAddr>,
    /// リクエスト本文（JSON形式）
    pub request_body: serde_json::Value,
    /// レスポンス本文（JSON形式、エラー時はNone）
    pub response_body: Option<serde_json::Value>,
    /// 処理時間（ミリ秒）
    pub duration_ms: u64,
    /// レコードのステータス（成功 or エラー）
    pub status: RecordStatus,
    /// レスポンス完了時刻
    pub completed_at: DateTime<Utc>,
    /// 入力トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// 出力トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// 総トークン数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    /// APIキーID（api_keysテーブル参照）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_id: Option<Uuid>,
}

/// リクエストタイプ
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestType {
    /// /v1/messages エンドポイント (Anthropic Messages API)
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
    /// /v1/chat/completions エンドポイント
    Chat,
    /// /v1/responses エンドポイント (Open Responses API)
    Responses,
    /// /v1/completions エンドポイント
    Generate,
    /// /v1/embeddings エンドポイント
    Embeddings,
    /// /v1/audio/transcriptions エンドポイント (ASR)
    Transcription,
    /// /v1/audio/speech エンドポイント (TTS)
    Speech,
    /// /v1/images/generations エンドポイント
    ImageGeneration,
    /// /v1/images/edits エンドポイント
    ImageEdit,
    /// /v1/images/variations エンドポイント
    ImageVariation,
}

/// TPS計測対象のAPI種別。
///
/// 比較可能性を担保するため、TPSは API 種別ごとに分離して集計する。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TpsApiKind {
    /// /v1/chat/completions
    ChatCompletions,
    /// /v1/completions
    Completions,
    /// /v1/responses
    Responses,
}

impl TpsApiKind {
    /// RequestType から TPS API 種別を解決する。
    ///
    /// テキスト生成系以外（embeddings/audio/images）は TPS対象外として None を返す。
    pub fn from_request_type(request_type: RequestType) -> Option<Self> {
        match request_type {
            RequestType::AnthropicMessages | RequestType::Chat => Some(Self::ChatCompletions),
            RequestType::Responses => Some(Self::Responses),
            RequestType::Generate => Some(Self::Completions),
            RequestType::Embeddings
            | RequestType::Transcription
            | RequestType::Speech
            | RequestType::ImageGeneration
            | RequestType::ImageEdit
            | RequestType::ImageVariation => None,
        }
    }
}

/// TPSデータの取得元。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TpsSource {
    /// 本番リクエスト由来
    Production,
    /// ベンチマーク実行由来
    Benchmark,
}

/// レコードステータス
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RecordStatus {
    /// 正常に処理完了
    Success,
    /// エラー発生
    Error {
        /// エラーメッセージ
        message: String,
    },
}

/// 音声認識レスポンスフォーマット
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionResponseFormat {
    /// JSON形式
    #[default]
    Json,
    /// テキスト形式
    Text,
    /// SRT字幕形式
    Srt,
    /// VTT字幕形式
    Vtt,
    /// 詳細JSON形式（タイムスタンプ付き）
    VerboseJson,
}

/// 音声認識リクエスト (ASR)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionRequest {
    /// モデル名 (例: "whisper-large-v3")
    pub model: String,
    /// 音声の言語 (ISO-639-1形式、例: "ja", "en")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: TranscriptionResponseFormat,
    /// サンプリング温度 (0.0-1.0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// タイムスタンプの粒度 (segment, word)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_granularities: Option<Vec<String>>,
}

/// 音声認識レスポンス (ASR)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionResponse {
    /// 認識されたテキスト
    pub text: String,
    /// 検出された言語
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// 音声の長さ（秒）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// セグメント情報（verbose_json時）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<TranscriptionSegment>>,
}

/// 音声認識セグメント
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionSegment {
    /// セグメントID
    pub id: u32,
    /// 開始時間（秒）
    pub start: f64,
    /// 終了時間（秒）
    pub end: f64,
    /// セグメントテキスト
    pub text: String,
}

/// 音声合成リクエスト (TTS)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeechRequest {
    /// モデル名 (例: "vibevoice-v1", "tts-1")
    pub model: String,
    /// 読み上げテキスト
    pub input: String,
    /// ボイス名 (例: "nova", "alloy", "echo")
    #[serde(default = "default_voice")]
    pub voice: String,
    /// 出力フォーマット
    #[serde(default)]
    pub response_format: AudioFormat,
    /// 再生速度 (0.25-4.0、デフォルト1.0)
    #[serde(default = "default_speed")]
    pub speed: f64,
}

fn default_voice() -> String {
    "nova".to_string()
}

fn default_speed() -> f64 {
    1.0
}

/// 画像生成リクエスト (Text-to-Image)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageGenerationRequest {
    /// モデル名 (例: "stable-diffusion-xl", "dall-e-3")
    pub model: String,
    /// 生成プロンプト
    pub prompt: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// 品質設定
    #[serde(default)]
    pub quality: ImageQuality,
    /// スタイル
    #[serde(default)]
    pub style: ImageStyle,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
    /// ネガティブプロンプト（SD拡張）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// シード値（再現性用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// 生成ステップ数（SD拡張、デフォルト: 20）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
}

fn default_image_n() -> u8 {
    1
}

/// 画像編集リクエスト (Inpainting)
///
/// multipart/form-dataとして送信されるため、画像データは別途処理
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageEditRequest {
    /// モデル名
    pub model: String,
    /// 編集プロンプト
    pub prompt: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
}

/// 画像バリエーションリクエスト
///
/// multipart/form-dataとして送信されるため、画像データは別途処理
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageVariationRequest {
    /// モデル名
    pub model: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
}

/// 画像レスポンス (generations/edits/variations共通)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageResponse {
    /// 生成時刻 (Unix timestamp)
    pub created: i64,
    /// 生成された画像データ配列
    pub data: Vec<ImageData>,
}

/// 画像データ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ImageData {
    /// URL形式
    Url {
        /// 画像URL
        url: String,
        /// 改訂されたプロンプト（DALL-E 3等）
        #[serde(skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<String>,
    },
    /// Base64形式
    Base64 {
        /// Base64エンコードされた画像データ
        b64_json: String,
        /// 改訂されたプロンプト
        #[serde(skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<String>,
    },
}

impl RequestResponseRecord {
    /// エンドポイント特定済みのレコードを作成する。
    ///
    /// `status` から `RecordStatus` を自動判定する。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint_id: Uuid,
        endpoint_name: String,
        endpoint_ip: IpAddr,
        model: String,
        request_type: RequestType,
        request_body: serde_json::Value,
        status: StatusCode,
        duration: std::time::Duration,
        client_ip: Option<IpAddr>,
        api_key_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type,
            model,
            endpoint_id,
            endpoint_name,
            endpoint_ip,
            client_ip,
            request_body,
            response_body: None,
            duration_ms: duration.as_millis() as u64,
            status: if status.is_success() {
                RecordStatus::Success
            } else {
                RecordStatus::Error {
                    message: format!("HTTP {}", status.as_u16()),
                }
            },
            completed_at: Utc::now(),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id,
        }
    }

    /// エンドポイント未特定のエラーレコードを作成する。
    pub fn error(
        model: String,
        request_type: RequestType,
        request_body: serde_json::Value,
        message: String,
        duration_ms: u64,
        client_ip: Option<IpAddr>,
        api_key_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type,
            model,
            endpoint_id: Uuid::nil(),
            endpoint_name: "N/A".to_string(),
            endpoint_ip: IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            client_ip,
            request_body,
            response_body: None,
            duration_ms,
            status: RecordStatus::Error { message },
            completed_at: Utc::now(),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_factory_new_success() {
        let endpoint_id = Uuid::new_v4();
        let record = RequestResponseRecord::new(
            endpoint_id,
            "test-endpoint".to_string(),
            "10.0.0.1".parse().unwrap(),
            "gpt-4".to_string(),
            RequestType::Chat,
            serde_json::json!({"messages": []}),
            StatusCode::OK,
            std::time::Duration::from_millis(150),
            Some("192.168.1.1".parse().unwrap()),
            None,
        );

        assert_eq!(record.endpoint_id, endpoint_id);
        assert_eq!(record.endpoint_name, "test-endpoint");
        assert_eq!(record.model, "gpt-4");
        assert_eq!(record.duration_ms, 150);
        assert!(matches!(record.status, RecordStatus::Success));
        assert!(record.response_body.is_none());
        assert!(record.input_tokens.is_none());
    }

    #[test]
    fn test_record_factory_new_error_status() {
        let record = RequestResponseRecord::new(
            Uuid::new_v4(),
            "ep".to_string(),
            "10.0.0.1".parse().unwrap(),
            "gpt-4".to_string(),
            RequestType::Chat,
            serde_json::json!({}),
            StatusCode::INTERNAL_SERVER_ERROR,
            std::time::Duration::from_millis(50),
            None,
            None,
        );

        assert!(matches!(record.status, RecordStatus::Error { message } if message == "HTTP 500"));
    }

    #[test]
    fn test_record_factory_error() {
        let record = RequestResponseRecord::error(
            "llama2".to_string(),
            RequestType::Generate,
            serde_json::json!({"prompt": "hello"}),
            "No endpoints available".to_string(),
            0,
            Some("192.168.1.1".parse().unwrap()),
            None,
        );

        assert_eq!(record.endpoint_id, Uuid::nil());
        assert_eq!(record.endpoint_name, "N/A");
        assert_eq!(
            record.endpoint_ip,
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
        );
        assert_eq!(record.model, "llama2");
        assert_eq!(record.duration_ms, 0);
        assert!(
            matches!(&record.status, RecordStatus::Error { message } if message == "No endpoints available")
        );
    }

    #[test]
    fn test_chat_request_default_stream_false() {
        let json = r#"{"model":"llama2","messages":[{"role":"user","content":"Hello"}]}"#;
        let request: ChatRequest = serde_json::from_str(json).unwrap();

        assert!(!request.stream);
    }

    #[test]
    fn test_request_type_serialization() {
        assert_eq!(
            serde_json::to_string(&RequestType::AnthropicMessages).unwrap(),
            "\"anthropic_messages\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::Chat).unwrap(),
            "\"chat\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::Generate).unwrap(),
            "\"generate\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::Embeddings).unwrap(),
            "\"embeddings\""
        );
        // 音声リクエストタイプ
        assert_eq!(
            serde_json::to_string(&RequestType::Transcription).unwrap(),
            "\"transcription\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::Speech).unwrap(),
            "\"speech\""
        );
    }

    #[test]
    fn test_transcription_request_serialization() {
        let request = TranscriptionRequest {
            model: "whisper-large-v3".to_string(),
            language: Some("ja".to_string()),
            response_format: TranscriptionResponseFormat::Json,
            temperature: None,
            timestamp_granularities: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"whisper-large-v3\""));
        assert!(json.contains("\"language\":\"ja\""));
        assert!(json.contains("\"response_format\":\"json\""));

        let deserialized: TranscriptionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "whisper-large-v3");
        assert_eq!(deserialized.language, Some("ja".to_string()));
    }

    #[test]
    fn test_transcription_response_format_serialization() {
        assert_eq!(
            serde_json::to_string(&TranscriptionResponseFormat::Json).unwrap(),
            "\"json\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionResponseFormat::Text).unwrap(),
            "\"text\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionResponseFormat::Srt).unwrap(),
            "\"srt\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionResponseFormat::Vtt).unwrap(),
            "\"vtt\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionResponseFormat::VerboseJson).unwrap(),
            "\"verbose_json\""
        );
    }

    #[test]
    fn test_transcription_response_serialization() {
        let response = TranscriptionResponse {
            text: "こんにちは".to_string(),
            language: Some("ja".to_string()),
            duration: Some(2.5),
            segments: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"text\":\"こんにちは\""));
        assert!(json.contains("\"language\":\"ja\""));

        let deserialized: TranscriptionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.text, "こんにちは");
    }

    #[test]
    fn test_speech_request_serialization() {
        use crate::types::media::AudioFormat;

        let request = SpeechRequest {
            model: "vibevoice-v1".to_string(),
            input: "こんにちは".to_string(),
            voice: "nova".to_string(),
            response_format: AudioFormat::Mp3,
            speed: 1.0,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"vibevoice-v1\""));
        assert!(json.contains("\"input\":\"こんにちは\""));
        assert!(json.contains("\"voice\":\"nova\""));
        assert!(json.contains("\"response_format\":\"mp3\""));
        assert!(json.contains("\"speed\":1.0") || json.contains("\"speed\":1"));

        let deserialized: SpeechRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "vibevoice-v1");
        assert_eq!(deserialized.input, "こんにちは");
        assert_eq!(deserialized.voice, "nova");
    }

    #[test]
    fn test_speech_request_defaults() {
        let json = r#"{"model":"tts-1","input":"Hello"}"#;
        let request: SpeechRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.model, "tts-1");
        assert_eq!(request.input, "Hello");
        assert_eq!(request.voice, "nova"); // default
        assert_eq!(request.speed, 1.0); // default
    }

    #[test]
    fn test_request_type_image_serialization() {
        assert_eq!(
            serde_json::to_string(&RequestType::ImageGeneration).unwrap(),
            "\"imagegeneration\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::ImageEdit).unwrap(),
            "\"imageedit\""
        );
        assert_eq!(
            serde_json::to_string(&RequestType::ImageVariation).unwrap(),
            "\"imagevariation\""
        );
    }

    #[test]
    fn test_image_generation_request_serialization() {
        use crate::types::media::{ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};

        let request = ImageGenerationRequest {
            model: "stable-diffusion-xl".to_string(),
            prompt: "A white cat sitting on a windowsill".to_string(),
            n: 2,
            size: ImageSize::Size1024,
            quality: ImageQuality::Standard,
            style: ImageStyle::Vivid,
            response_format: ImageResponseFormat::Url,
            negative_prompt: Some("blurry".to_string()),
            seed: Some(12345),
            steps: Some(2),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"stable-diffusion-xl\""));
        assert!(json.contains("\"prompt\":\"A white cat sitting on a windowsill\""));
        assert!(json.contains("\"n\":2"));
        assert!(json.contains("\"size\":\"1024x1024\""));
        assert!(json.contains("\"negative_prompt\":\"blurry\""));
        assert!(json.contains("\"steps\":2"));

        let deserialized: ImageGenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "stable-diffusion-xl");
        assert_eq!(deserialized.n, 2);
    }

    #[test]
    fn test_image_generation_request_defaults() {
        let json = r#"{"model":"sd-xl","prompt":"A cat"}"#;
        let request: ImageGenerationRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.model, "sd-xl");
        assert_eq!(request.prompt, "A cat");
        assert_eq!(request.n, 1); // default
        assert_eq!(request.size, ImageSize::Size1024); // default
        assert_eq!(request.quality, ImageQuality::Standard); // default
        assert_eq!(request.style, ImageStyle::Vivid); // default
        assert_eq!(request.response_format, ImageResponseFormat::Url); // default
        assert_eq!(request.negative_prompt, None);
        assert_eq!(request.seed, None);
        assert_eq!(request.steps, None);
    }

    #[test]
    fn test_image_edit_request_serialization() {
        let request = ImageEditRequest {
            model: "stable-diffusion-xl".to_string(),
            prompt: "Add a hat".to_string(),
            n: 1,
            size: ImageSize::Size512,
            response_format: ImageResponseFormat::B64Json,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"stable-diffusion-xl\""));
        assert!(json.contains("\"prompt\":\"Add a hat\""));
        assert!(json.contains("\"size\":\"512x512\""));
        assert!(json.contains("\"response_format\":\"b64_json\""));

        let deserialized: ImageEditRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt, "Add a hat");
    }

    #[test]
    fn test_image_variation_request_serialization() {
        let request = ImageVariationRequest {
            model: "stable-diffusion-xl".to_string(),
            n: 3,
            size: ImageSize::Size256,
            response_format: ImageResponseFormat::Url,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"stable-diffusion-xl\""));
        assert!(json.contains("\"n\":3"));
        assert!(json.contains("\"size\":\"256x256\""));

        let deserialized: ImageVariationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.n, 3);
    }

    #[test]
    fn test_image_response_serialization_url() {
        let response = ImageResponse {
            created: 1699000000,
            data: vec![ImageData::Url {
                url: "https://example.com/image.png".to_string(),
                revised_prompt: Some("A beautiful cat".to_string()),
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"created\":1699000000"));
        assert!(json.contains("\"url\":\"https://example.com/image.png\""));
        assert!(json.contains("\"revised_prompt\":\"A beautiful cat\""));

        let deserialized: ImageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.created, 1699000000);
        assert_eq!(deserialized.data.len(), 1);
    }

    #[test]
    fn test_image_response_serialization_base64() {
        let response = ImageResponse {
            created: 1699000000,
            data: vec![ImageData::Base64 {
                b64_json: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string(),
                revised_prompt: None,
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"created\":1699000000"));
        assert!(json.contains("\"b64_json\":"));

        let deserialized: ImageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.data.len(), 1);
    }

    #[test]
    fn test_request_response_record_token_fields_serialization() {
        // T-1: RequestResponseRecordのトークンフィールドシリアライズテスト
        let record = RequestResponseRecord {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type: RequestType::Chat,
            model: "gpt-3.5-turbo".to_string(),
            endpoint_id: Uuid::new_v4(),
            endpoint_name: "test-node".to_string(),
            endpoint_ip: "127.0.0.1".parse().unwrap(),
            client_ip: Some("192.168.1.1".parse().unwrap()),
            request_body: serde_json::json!({"messages": []}),
            response_body: Some(serde_json::json!({"choices": []})),
            duration_ms: 100,
            status: RecordStatus::Success,
            completed_at: Utc::now(),
            input_tokens: Some(150),
            output_tokens: Some(50),
            total_tokens: Some(200),
            api_key_id: None,
        };

        let json = serde_json::to_string(&record).unwrap();

        // トークンフィールドがシリアライズされていることを確認
        assert!(json.contains("\"input_tokens\":150"));
        assert!(json.contains("\"output_tokens\":50"));
        assert!(json.contains("\"total_tokens\":200"));

        // デシリアライズして値を確認
        let deserialized: RequestResponseRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.input_tokens, Some(150));
        assert_eq!(deserialized.output_tokens, Some(50));
        assert_eq!(deserialized.total_tokens, Some(200));
    }

    #[test]
    fn test_request_response_record_token_fields_none() {
        // トークンフィールドがNoneの場合のテスト
        let record = RequestResponseRecord {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_type: RequestType::Chat,
            model: "gpt-3.5-turbo".to_string(),
            endpoint_id: Uuid::new_v4(),
            endpoint_name: "test-node".to_string(),
            endpoint_ip: "127.0.0.1".parse().unwrap(),
            client_ip: None,
            request_body: serde_json::json!({}),
            response_body: None,
            duration_ms: 50,
            status: RecordStatus::Success,
            completed_at: Utc::now(),
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            api_key_id: None,
        };

        let json = serde_json::to_string(&record).unwrap();
        let deserialized: RequestResponseRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.input_tokens, None);
        assert_eq!(deserialized.output_tokens, None);
        assert_eq!(deserialized.total_tokens, None);
    }

    // --- 追加テスト: TpsApiKind ---

    #[test]
    fn test_tps_api_kind_serde_roundtrip() {
        for kind in [
            TpsApiKind::ChatCompletions,
            TpsApiKind::Completions,
            TpsApiKind::Responses,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let deserialized: TpsApiKind = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, kind);
        }
    }

    #[test]
    fn test_tps_api_kind_serialization_values() {
        assert_eq!(
            serde_json::to_string(&TpsApiKind::ChatCompletions).unwrap(),
            "\"chat_completions\""
        );
        assert_eq!(
            serde_json::to_string(&TpsApiKind::Completions).unwrap(),
            "\"completions\""
        );
        assert_eq!(
            serde_json::to_string(&TpsApiKind::Responses).unwrap(),
            "\"responses\""
        );
    }

    #[test]
    fn test_tps_api_kind_from_request_type() {
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::AnthropicMessages),
            Some(TpsApiKind::ChatCompletions)
        );
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::Chat),
            Some(TpsApiKind::ChatCompletions)
        );
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::Generate),
            Some(TpsApiKind::Completions)
        );
        assert_eq!(TpsApiKind::from_request_type(RequestType::Embeddings), None);
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::Transcription),
            None
        );
        assert_eq!(TpsApiKind::from_request_type(RequestType::Speech), None);
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::ImageGeneration),
            None
        );
    }

    // --- 追加テスト: TpsSource ---

    #[test]
    fn test_tps_source_serde_roundtrip() {
        for source in [TpsSource::Production, TpsSource::Benchmark] {
            let json = serde_json::to_string(&source).unwrap();
            let deserialized: TpsSource = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, source);
        }
    }

    #[test]
    fn test_tps_source_serialization_values() {
        assert_eq!(
            serde_json::to_string(&TpsSource::Production).unwrap(),
            "\"production\""
        );
        assert_eq!(
            serde_json::to_string(&TpsSource::Benchmark).unwrap(),
            "\"benchmark\""
        );
    }

    // ========================================================================
    // 追加テスト: ChatRequest
    // ========================================================================

    #[test]
    fn test_chat_request_with_stream_true() {
        let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"Hi"}],"stream":true}"#;
        let request: ChatRequest = serde_json::from_str(json).unwrap();
        assert!(request.stream);
        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.messages.len(), 1);
    }

    #[test]
    fn test_chat_request_empty_messages() {
        let json = r#"{"model":"llama2","messages":[]}"#;
        let request: ChatRequest = serde_json::from_str(json).unwrap();
        assert!(request.messages.is_empty());
        assert_eq!(request.model, "llama2");
    }

    #[test]
    fn test_chat_request_serde_roundtrip() {
        let request = ChatRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "You are helpful.".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: "Hello!".to_string(),
                },
            ],
            stream: true,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "gpt-4o");
        assert_eq!(deserialized.messages.len(), 2);
        assert!(deserialized.stream);
    }

    // ========================================================================
    // 追加テスト: ChatMessage
    // ========================================================================

    #[test]
    fn test_chat_message_serde_roundtrip() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: "Hello!".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, msg);
    }

    #[test]
    fn test_chat_message_empty_content() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, "");
    }

    #[test]
    fn test_chat_message_equality() {
        let msg1 = ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let msg2 = ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let msg3 = ChatMessage {
            role: "assistant".to_string(),
            content: "hi".to_string(),
        };
        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }

    // ========================================================================
    // 追加テスト: ChatCompletionRequest
    // ========================================================================

    #[test]
    fn test_chat_completion_request_defaults() {
        let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"test"}]}"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(!request.stream); // default false
        assert!(request.max_tokens.is_none()); // default None
    }

    #[test]
    fn test_chat_completion_request_with_max_tokens() {
        let json =
            r#"{"model":"gpt-4","messages":[{"role":"user","content":"test"}],"max_tokens":1024}"#;
        let request: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.max_tokens, Some(1024));
    }

    #[test]
    fn test_chat_completion_request_serde_roundtrip() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            stream: true,
            max_tokens: Some(2048),
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_chat_completion_request_max_tokens_skip_serializing_none() {
        let request = ChatCompletionRequest {
            model: "m".to_string(),
            messages: vec![],
            stream: false,
            max_tokens: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("max_tokens"));
    }

    // ========================================================================
    // 追加テスト: GenerateRequest
    // ========================================================================

    #[test]
    fn test_generate_request_defaults() {
        let json = r#"{"model":"llama2","prompt":"Once upon a time"}"#;
        let request: GenerateRequest = serde_json::from_str(json).unwrap();
        assert!(!request.stream);
        assert_eq!(request.prompt, "Once upon a time");
    }

    #[test]
    fn test_generate_request_serde_roundtrip() {
        let request = GenerateRequest {
            model: "codellama".to_string(),
            prompt: "def hello():".to_string(),
            stream: true,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: GenerateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model, "codellama");
        assert_eq!(deserialized.prompt, "def hello():");
        assert!(deserialized.stream);
    }

    #[test]
    fn test_generate_request_empty_prompt() {
        let json = r#"{"model":"m","prompt":""}"#;
        let request: GenerateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.prompt, "");
    }

    // ========================================================================
    // 追加テスト: RequestType
    // ========================================================================

    #[test]
    fn test_request_type_serde_roundtrip() {
        for rt in [
            RequestType::AnthropicMessages,
            RequestType::Chat,
            RequestType::Generate,
            RequestType::Embeddings,
            RequestType::Transcription,
            RequestType::Speech,
            RequestType::ImageGeneration,
            RequestType::ImageEdit,
            RequestType::ImageVariation,
        ] {
            let json = serde_json::to_string(&rt).unwrap();
            let deserialized: RequestType = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, rt);
        }
    }

    #[test]
    fn test_request_type_invalid_deserialization_fails() {
        let result = serde_json::from_str::<RequestType>("\"unknown\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_request_type_debug_format() {
        assert_eq!(
            format!("{:?}", RequestType::AnthropicMessages),
            "AnthropicMessages"
        );
        assert_eq!(format!("{:?}", RequestType::Chat), "Chat");
        assert_eq!(format!("{:?}", RequestType::Generate), "Generate");
        assert_eq!(format!("{:?}", RequestType::Embeddings), "Embeddings");
        assert_eq!(format!("{:?}", RequestType::Transcription), "Transcription");
        assert_eq!(format!("{:?}", RequestType::Speech), "Speech");
    }

    // ========================================================================
    // 追加テスト: RecordStatus
    // ========================================================================

    #[test]
    fn test_record_status_success_serde_roundtrip() {
        let status = RecordStatus::Success;
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        let deserialized: RecordStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, RecordStatus::Success));
    }

    #[test]
    fn test_record_status_error_serde_roundtrip() {
        let status = RecordStatus::Error {
            message: "timeout".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"timeout\""));
        let deserialized: RecordStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, RecordStatus::Error { message } if message == "timeout"));
    }

    #[test]
    fn test_record_status_error_empty_message() {
        let status = RecordStatus::Error {
            message: "".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: RecordStatus = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, RecordStatus::Error { message } if message.is_empty()));
    }

    // ========================================================================
    // 追加テスト: TranscriptionResponseFormat
    // ========================================================================

    #[test]
    fn test_transcription_response_format_default() {
        let fmt: TranscriptionResponseFormat = Default::default();
        assert_eq!(fmt, TranscriptionResponseFormat::Json);
    }

    #[test]
    fn test_transcription_response_format_serde_roundtrip() {
        for fmt in [
            TranscriptionResponseFormat::Json,
            TranscriptionResponseFormat::Text,
            TranscriptionResponseFormat::Srt,
            TranscriptionResponseFormat::Vtt,
            TranscriptionResponseFormat::VerboseJson,
        ] {
            let json = serde_json::to_string(&fmt).unwrap();
            let deserialized: TranscriptionResponseFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, fmt);
        }
    }

    #[test]
    fn test_transcription_response_format_invalid_fails() {
        let result = serde_json::from_str::<TranscriptionResponseFormat>("\"xml\"");
        assert!(result.is_err());
    }

    // ========================================================================
    // 追加テスト: TranscriptionRequest
    // ========================================================================

    #[test]
    fn test_transcription_request_minimal() {
        let json = r#"{"model":"whisper-1"}"#;
        let request: TranscriptionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model, "whisper-1");
        assert!(request.language.is_none());
        assert_eq!(request.response_format, TranscriptionResponseFormat::Json);
        assert!(request.temperature.is_none());
        assert!(request.timestamp_granularities.is_none());
    }

    #[test]
    fn test_transcription_request_serde_roundtrip() {
        let request = TranscriptionRequest {
            model: "whisper-large-v3".to_string(),
            language: Some("en".to_string()),
            response_format: TranscriptionResponseFormat::VerboseJson,
            temperature: Some(0.5),
            timestamp_granularities: Some(vec!["segment".to_string(), "word".to_string()]),
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: TranscriptionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    // ========================================================================
    // 追加テスト: TranscriptionResponse
    // ========================================================================

    #[test]
    fn test_transcription_response_minimal() {
        let json = r#"{"text":"Hello world"}"#;
        let response: TranscriptionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.text, "Hello world");
        assert!(response.language.is_none());
        assert!(response.duration.is_none());
        assert!(response.segments.is_none());
    }

    #[test]
    fn test_transcription_response_with_segments() {
        let response = TranscriptionResponse {
            text: "Full text".to_string(),
            language: Some("ja".to_string()),
            duration: Some(10.5),
            segments: Some(vec![
                TranscriptionSegment {
                    id: 0,
                    start: 0.0,
                    end: 5.0,
                    text: "First half".to_string(),
                },
                TranscriptionSegment {
                    id: 1,
                    start: 5.0,
                    end: 10.5,
                    text: "Second half".to_string(),
                },
            ]),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: TranscriptionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.segments.as_ref().unwrap().len(), 2);
        assert_eq!(deserialized.segments.as_ref().unwrap()[0].id, 0);
        assert_eq!(deserialized.segments.as_ref().unwrap()[1].start, 5.0);
    }

    // ========================================================================
    // 追加テスト: TranscriptionSegment
    // ========================================================================

    #[test]
    fn test_transcription_segment_serde_roundtrip() {
        let segment = TranscriptionSegment {
            id: 42,
            start: 1.5,
            end: 3.75,
            text: "Test segment".to_string(),
        };
        let json = serde_json::to_string(&segment).unwrap();
        let deserialized: TranscriptionSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, segment);
    }

    #[test]
    fn test_transcription_segment_zero_duration() {
        let segment = TranscriptionSegment {
            id: 0,
            start: 0.0,
            end: 0.0,
            text: "".to_string(),
        };
        let json = serde_json::to_string(&segment).unwrap();
        let deserialized: TranscriptionSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.start, 0.0);
        assert_eq!(deserialized.end, 0.0);
    }

    // ========================================================================
    // 追加テスト: SpeechRequest
    // ========================================================================

    #[test]
    fn test_speech_request_serde_roundtrip() {
        use crate::types::media::AudioFormat;
        let request = SpeechRequest {
            model: "tts-1-hd".to_string(),
            input: "Hello world".to_string(),
            voice: "echo".to_string(),
            response_format: AudioFormat::Wav,
            speed: 1.5,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: SpeechRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_speech_request_empty_input() {
        let json = r#"{"model":"tts-1","input":""}"#;
        let request: SpeechRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.input, "");
        assert_eq!(request.voice, "nova");
        assert_eq!(request.speed, 1.0);
    }

    // ========================================================================
    // 追加テスト: ImageGenerationRequest edge cases
    // ========================================================================

    #[test]
    fn test_image_generation_request_serde_roundtrip() {
        use crate::types::media::{ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};
        let request = ImageGenerationRequest {
            model: "dall-e-3".to_string(),
            prompt: "A sunset over mountains".to_string(),
            n: 4,
            size: ImageSize::Size512,
            quality: ImageQuality::Hd,
            style: ImageStyle::Natural,
            response_format: ImageResponseFormat::B64Json,
            negative_prompt: Some("blurry, low quality".to_string()),
            seed: Some(42),
            steps: Some(30),
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ImageGenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    // ========================================================================
    // 追加テスト: ImageEditRequest
    // ========================================================================

    #[test]
    fn test_image_edit_request_defaults() {
        let json = r#"{"model":"sd-xl","prompt":"Add wings"}"#;
        let request: ImageEditRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.n, 1);
        assert_eq!(request.size, ImageSize::Size1024);
        assert_eq!(request.response_format, ImageResponseFormat::Url);
    }

    #[test]
    fn test_image_edit_request_serde_roundtrip() {
        let request = ImageEditRequest {
            model: "sd-xl".to_string(),
            prompt: "Make it red".to_string(),
            n: 2,
            size: ImageSize::Size256,
            response_format: ImageResponseFormat::B64Json,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ImageEditRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    // ========================================================================
    // 追加テスト: ImageVariationRequest
    // ========================================================================

    #[test]
    fn test_image_variation_request_defaults() {
        let json = r#"{"model":"sd-xl"}"#;
        let request: ImageVariationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.n, 1);
        assert_eq!(request.size, ImageSize::Size1024);
        assert_eq!(request.response_format, ImageResponseFormat::Url);
    }

    #[test]
    fn test_image_variation_request_serde_roundtrip() {
        let request = ImageVariationRequest {
            model: "sd-xl".to_string(),
            n: 5,
            size: ImageSize::Size512,
            response_format: ImageResponseFormat::B64Json,
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: ImageVariationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, request);
    }

    // ========================================================================
    // 追加テスト: ImageResponse / ImageData
    // ========================================================================

    #[test]
    fn test_image_response_empty_data() {
        let response = ImageResponse {
            created: 0,
            data: vec![],
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: ImageResponse = serde_json::from_str(&json).unwrap();
        assert!(deserialized.data.is_empty());
        assert_eq!(deserialized.created, 0);
    }

    #[test]
    fn test_image_response_multiple_items() {
        let response = ImageResponse {
            created: 1700000000,
            data: vec![
                ImageData::Url {
                    url: "https://example.com/img1.png".to_string(),
                    revised_prompt: None,
                },
                ImageData::Url {
                    url: "https://example.com/img2.png".to_string(),
                    revised_prompt: Some("Revised prompt".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: ImageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.data.len(), 2);
    }

    #[test]
    fn test_image_data_url_without_revised_prompt_skips() {
        let data = ImageData::Url {
            url: "https://example.com/img.png".to_string(),
            revised_prompt: None,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.contains("revised_prompt"));
    }

    #[test]
    fn test_image_data_base64_without_revised_prompt_skips() {
        let data = ImageData::Base64 {
            b64_json: "abc123".to_string(),
            revised_prompt: None,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.contains("revised_prompt"));
    }

    // ========================================================================
    // 追加テスト: TpsApiKind edge cases
    // ========================================================================

    #[test]
    fn test_tps_api_kind_invalid_deserialization_fails() {
        let result = serde_json::from_str::<TpsApiKind>("\"unknown\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_tps_api_kind_from_image_edit_returns_none() {
        assert_eq!(TpsApiKind::from_request_type(RequestType::ImageEdit), None);
    }

    #[test]
    fn test_tps_api_kind_from_image_variation_returns_none() {
        assert_eq!(
            TpsApiKind::from_request_type(RequestType::ImageVariation),
            None
        );
    }

    #[test]
    fn test_tps_api_kind_debug_format() {
        assert_eq!(
            format!("{:?}", TpsApiKind::ChatCompletions),
            "ChatCompletions"
        );
        assert_eq!(format!("{:?}", TpsApiKind::Completions), "Completions");
        assert_eq!(format!("{:?}", TpsApiKind::Responses), "Responses");
    }

    #[test]
    fn test_tps_api_kind_hash_in_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TpsApiKind::ChatCompletions);
        set.insert(TpsApiKind::ChatCompletions); // duplicate
        set.insert(TpsApiKind::Completions);
        assert_eq!(set.len(), 2);
    }

    // ========================================================================
    // 追加テスト: TpsSource edge cases
    // ========================================================================

    #[test]
    fn test_tps_source_invalid_deserialization_fails() {
        let result = serde_json::from_str::<TpsSource>("\"manual\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_tps_source_debug_format() {
        assert_eq!(format!("{:?}", TpsSource::Production), "Production");
        assert_eq!(format!("{:?}", TpsSource::Benchmark), "Benchmark");
    }

    // ========================================================================
    // 追加テスト: RequestResponseRecord
    // ========================================================================

    #[test]
    fn test_request_response_record_new_with_various_statuses() {
        // Test with various success status codes
        for status_code in [StatusCode::OK, StatusCode::CREATED, StatusCode::NO_CONTENT] {
            let record = RequestResponseRecord::new(
                Uuid::new_v4(),
                "ep".to_string(),
                "10.0.0.1".parse().unwrap(),
                "model".to_string(),
                RequestType::Chat,
                serde_json::json!({}),
                status_code,
                std::time::Duration::from_millis(10),
                None,
                None,
            );
            assert!(matches!(record.status, RecordStatus::Success));
        }
    }

    #[test]
    fn test_request_response_record_new_with_error_statuses() {
        for status_code in [
            StatusCode::BAD_REQUEST,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let record = RequestResponseRecord::new(
                Uuid::new_v4(),
                "ep".to_string(),
                "10.0.0.1".parse().unwrap(),
                "model".to_string(),
                RequestType::Chat,
                serde_json::json!({}),
                status_code,
                std::time::Duration::from_millis(10),
                None,
                None,
            );
            assert!(matches!(record.status, RecordStatus::Error { .. }));
        }
    }

    #[test]
    fn test_request_response_record_error_uses_nil_uuid() {
        let record = RequestResponseRecord::error(
            "model".to_string(),
            RequestType::Embeddings,
            serde_json::json!({}),
            "No endpoint".to_string(),
            100,
            None,
            None,
        );
        assert_eq!(record.endpoint_id, Uuid::nil());
        assert_eq!(record.endpoint_name, "N/A");
    }

    #[test]
    fn test_request_response_record_with_api_key_id() {
        let api_key_id = Uuid::new_v4();
        let record = RequestResponseRecord::new(
            Uuid::new_v4(),
            "ep".to_string(),
            "10.0.0.1".parse().unwrap(),
            "model".to_string(),
            RequestType::Chat,
            serde_json::json!({}),
            StatusCode::OK,
            std::time::Duration::from_millis(10),
            None,
            Some(api_key_id),
        );
        assert_eq!(record.api_key_id, Some(api_key_id));
    }

    #[test]
    fn test_request_response_record_error_with_api_key_id() {
        let api_key_id = Uuid::new_v4();
        let record = RequestResponseRecord::error(
            "model".to_string(),
            RequestType::Chat,
            serde_json::json!({}),
            "err".to_string(),
            0,
            None,
            Some(api_key_id),
        );
        assert_eq!(record.api_key_id, Some(api_key_id));
    }

    #[test]
    fn test_request_response_record_ipv6_client() {
        let record = RequestResponseRecord::new(
            Uuid::new_v4(),
            "ep".to_string(),
            "10.0.0.1".parse().unwrap(),
            "model".to_string(),
            RequestType::Chat,
            serde_json::json!({}),
            StatusCode::OK,
            std::time::Duration::from_millis(10),
            Some("::1".parse().unwrap()),
            None,
        );
        assert_eq!(record.client_ip, Some("::1".parse().unwrap()));
    }
}
