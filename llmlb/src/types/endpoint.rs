//! エンドポイント型定義
//!
//! SPEC-e8e9326e: llmlb主導エンドポイント登録システム

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

mod device;
pub use device::*;
mod download;
pub use download::{DownloadStatus, ModelDownloadTask};
mod endpoint_type;
pub use endpoint_type::*;
mod supported_api;
pub use supported_api::*;

/// エンドポイントの状態
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndpointStatus {
    /// 初期状態（未確認）
    #[default]
    Pending,
    /// 稼働中
    Online,
    /// 停止中
    Offline,
    /// エラー状態
    Error,
}

impl EndpointStatus {
    /// EndpointStatusを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Error => "error",
        }
    }
}

impl FromStr for EndpointStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "pending" => Self::Pending,
            "online" => Self::Online,
            "offline" => Self::Offline,
            "error" => Self::Error,
            _ => Self::Pending,
        })
    }
}

impl std::fmt::Display for EndpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// エンドポイントの機能タイプ
///
/// エンドポイントの機能分類
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EndpointCapability {
    /// チャット補完（LLM推論）
    ChatCompletion,
    /// 埋め込みベクトル生成
    Embeddings,
    /// 画像生成（StableDiffusion等）
    ImageGeneration,
    /// 音声認識（Whisper等）
    AudioTranscription,
    /// 音声合成（TTS）
    AudioSpeech,
}

impl EndpointCapability {
    /// 文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatCompletion => "chat_completion",
            Self::Embeddings => "embeddings",
            Self::ImageGeneration => "image_generation",
            Self::AudioTranscription => "audio_transcription",
            Self::AudioSpeech => "audio_speech",
        }
    }
}

impl std::fmt::Display for EndpointCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// エンドポイント
///
/// 推論サービスの接続先を表すエンティティ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// 一意識別子
    pub id: Uuid,
    /// 表示名（例: "本番Ollama", "開発xLLM1"）
    pub name: String,
    /// ベースURL（例: `http://192.168.1.100:11434`）
    pub base_url: String,
    /// APIキー（暗号化保存、シリアライズ時はスキップ）
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    /// 現在の状態
    pub status: EndpointStatus,
    /// エンドポイントタイプ（SPEC-e8e9326e追加要件 2026-01-26）
    /// 登録時に自動検出される。対応する5タイプのみ許可。
    pub endpoint_type: EndpointType,
    /// ヘルスチェック間隔（秒）
    pub health_check_interval_secs: u32,
    /// 推論タイムアウト（秒）
    pub inference_timeout_secs: u32,
    /// ヘルスチェック時のレイテンシ（ミリ秒）
    pub latency_ms: Option<u32>,
    /// 最終確認時刻
    pub last_seen: Option<DateTime<Utc>>,
    /// 最後のエラーメッセージ
    pub last_error: Option<String>,
    /// 連続エラー回数
    pub error_count: u32,
    /// 登録日時
    pub registered_at: DateTime<Utc>,
    /// メモ
    pub notes: Option<String>,
    /// エンドポイントの機能一覧（SPEC-e8e9326e移行用）
    /// 画像生成、音声認識等の特殊機能をサポートするかを示す
    #[serde(default)]
    pub capabilities: Vec<EndpointCapability>,
    /// GPU情報（/api/healthから取得、Phase 1.4）
    #[serde(default)]
    pub gpu_device_count: Option<u32>,
    /// GPU総メモリ（バイト）
    #[serde(default)]
    pub gpu_total_memory_bytes: Option<u64>,
    /// GPU使用中メモリ（バイト）
    #[serde(default)]
    pub gpu_used_memory_bytes: Option<u64>,
    /// GPU能力スコア
    #[serde(default)]
    pub gpu_capability_score: Option<f32>,
    /// 現在のアクティブリクエスト数
    #[serde(default)]
    pub active_requests: Option<u32>,
    /// デバイス情報（/api/systemから取得、SPEC-f8e3a1b7）
    #[serde(default)]
    pub device_info: Option<DeviceInfo>,
    /// 推論レイテンシ（EMA、ミリ秒、SPEC-f8e3a1b7）
    /// ヘルスチェックのlatency_msとは別に、実際の推論時間を追跡
    #[serde(default)]
    pub inference_latency_ms: Option<f64>,
    /// 累計リクエスト数（SPEC-8c32349f）
    #[serde(default)]
    pub total_requests: i64,
    /// 累計成功リクエスト数（SPEC-8c32349f）
    #[serde(default)]
    pub successful_requests: i64,
    /// 累計失敗リクエスト数（SPEC-8c32349f）
    #[serde(default)]
    pub failed_requests: i64,
}

impl Endpoint {
    /// 新しいエンドポイントを作成
    ///
    /// `endpoint_type` は登録時に自動検出された結果を指定する。
    pub fn new(name: String, base_url: String, endpoint_type: EndpointType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            base_url,
            api_key: None,
            status: EndpointStatus::Pending,
            endpoint_type,
            health_check_interval_secs: 30,
            inference_timeout_secs: endpoint_type.recommended_inference_timeout_secs(),
            latency_ms: None,
            last_seen: None,
            last_error: None,
            error_count: 0,
            registered_at: Utc::now(),
            notes: None,
            capabilities: vec![EndpointCapability::ChatCompletion], // デフォルトはチャット機能
            gpu_device_count: None,
            gpu_total_memory_bytes: None,
            gpu_used_memory_bytes: None,
            gpu_capability_score: None,
            active_requests: None,
            device_info: None,
            inference_latency_ms: None,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
        }
    }

    /// 指定した機能をサポートしているか確認
    pub fn has_capability(&self, cap: EndpointCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// 推論レイテンシを更新（EMA α=0.2）（SPEC-f8e3a1b7）
    ///
    /// 新しい計測値を指数移動平均で反映する。
    /// 初回計測時はその値をそのまま設定。
    pub fn update_inference_latency(&mut self, new_latency_ms: f64) {
        const ALPHA: f64 = 0.2;
        self.inference_latency_ms = Some(match self.inference_latency_ms {
            Some(current) if current.is_finite() => {
                ALPHA * new_latency_ms + (1.0 - ALPHA) * current
            }
            _ => new_latency_ms,
        });
    }

    /// オフライン時にレイテンシを無限大にリセット（SPEC-f8e3a1b7）
    ///
    /// エンドポイントがオフラインになった場合、負荷分散で最低優先度になるよう
    /// レイテンシを無限大に設定する。
    pub fn reset_inference_latency(&mut self) {
        self.inference_latency_ms = Some(f64::INFINITY);
    }

    /// 推論レイテンシを取得（ソート用、未計測時は無限大）
    pub fn get_inference_latency_for_sort(&self) -> f64 {
        self.inference_latency_ms.unwrap_or(f64::INFINITY)
    }
}

/// エンドポイントで利用可能なモデル情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointModel {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// モデル識別子
    pub model_id: String,
    /// 能力（chat, embeddings等）
    pub capabilities: Option<Vec<String>>,
    /// 最大トークン数（SPEC-e8e9326e追加要件 2026-01-26）
    pub max_tokens: Option<u32>,
    /// 最終確認時刻
    pub last_checked: Option<DateTime<Utc>>,
    /// サポートするAPI一覧（SPEC-0f1de549）
    #[serde(default = "EndpointModel::default_supported_apis")]
    pub supported_apis: Vec<SupportedAPI>,
    /// 正規名（HFリポ名）- モデル名統一化用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
}

impl EndpointModel {
    /// デフォルトのサポートAPI（Chat Completionsのみ）
    fn default_supported_apis() -> Vec<SupportedAPI> {
        vec![SupportedAPI::ChatCompletions]
    }
}

/// ヘルスチェック履歴
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointHealthCheck {
    /// 自動インクリメントID
    pub id: i64,
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// チェック実行時刻
    pub checked_at: DateTime<Utc>,
    /// 成功/失敗
    pub success: bool,
    /// レイテンシ（成功時のみ）
    pub latency_ms: Option<u32>,
    /// エラーメッセージ（失敗時のみ）
    pub error_message: Option<String>,
    /// チェック前の状態
    pub status_before: EndpointStatus,
    /// チェック後の状態
    pub status_after: EndpointStatus,
}

/// エンドポイント日次集計レコード（SPEC-8c32349f）
///
/// エンドポイント×モデル×日付の粒度で集計されたリクエスト統計。
/// 永続保存され、トレンド分析とモデル別分析の基盤となる。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDailyStats {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// モデルID
    pub model_id: String,
    /// 日付（YYYY-MM-DD形式、サーバーローカル時間）
    pub date: String,
    /// 当日のリクエスト合計数
    pub total_requests: i64,
    /// 当日の成功リクエスト数
    pub successful_requests: i64,
    /// 当日の失敗リクエスト数
    pub failed_requests: i64,
}

#[cfg(test)]
mod tests;
