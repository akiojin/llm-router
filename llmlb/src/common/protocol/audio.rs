//! 音声 I/O DTO: 音声認識（ASR/transcription）と音声合成（TTS）
//!
//! arch-review [H6]: common/protocol.rs から音声関連 DTO を分離。親は
//! `pub use audio::*` で再エクスポートし既存パス・テストの参照を維持する。

use crate::types::media::AudioFormat;
use serde::{Deserialize, Serialize};

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
