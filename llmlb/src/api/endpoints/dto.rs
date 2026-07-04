//! エンドポイント管理 API のリクエスト／レスポンス DTO
//!
//! arch-review [H6] 対応。api/endpoints.rs が 2200 行超の god-object 化していた
//! ため、ハンドラのロジックと、リクエスト／レスポンスの DTO 型（および
//! `Endpoint` 系ドメイン型からの From 変換）をこのサブモジュールへ分離した。
//! 既存の参照パス（api::endpoints::CreateEndpointRequest 等）を維持するため、
//! 親モジュールは `pub use dto::*` で再エクスポートする。

use crate::types::endpoint::{Endpoint, EndpointCapability, EndpointModel, ModelDownloadTask};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Option<Option<T>>のデシリアライズヘルパー
/// - フィールドなし → None
/// - フィールドがnull → Some(None)
/// - フィールドに値あり → Some(Some(value))
fn deserialize_optional_field<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

/// エンドポイント登録リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateEndpointRequest {
    /// 表示名
    pub name: String,
    /// ベースURL
    pub base_url: String,
    /// APIキー（任意）
    #[serde(default)]
    pub api_key: Option<String>,
    /// ヘルスチェック間隔（秒）
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u32,
    /// 推論タイムアウト（秒）
    #[serde(default)]
    pub inference_timeout_secs: Option<u32>,
    /// メモ
    #[serde(default)]
    pub notes: Option<String>,
    /// エンドポイントの機能一覧（画像生成、音声認識等）
    #[serde(default)]
    pub capabilities: Vec<EndpointCapability>,
}

pub(crate) fn default_health_check_interval() -> u32 {
    30
}

/// エンドポイント更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateEndpointRequest {
    /// 表示名
    #[serde(default)]
    pub name: Option<String>,
    /// ベースURL
    #[serde(default)]
    pub base_url: Option<String>,
    /// APIキー
    #[serde(default)]
    pub api_key: Option<String>,
    /// ヘルスチェック間隔（秒）
    #[serde(default)]
    pub health_check_interval_secs: Option<u32>,
    /// 推論タイムアウト（秒）
    #[serde(default)]
    pub inference_timeout_secs: Option<u32>,
    /// メモ（None=未指定, Some(None)=削除, Some(Some(v))=設定）
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub notes: Option<Option<String>>,
}

/// エンドポイントレスポンス
#[derive(Debug, Serialize)]
pub struct EndpointResponse {
    /// 一意識別子
    pub id: Uuid,
    /// 表示名
    pub name: String,
    /// ベースURL
    pub base_url: String,
    /// 現在の状態
    pub status: String,
    /// エンドポイントタイプ（SPEC-e8e9326e）
    pub endpoint_type: String,
    /// ヘルスチェック間隔（秒）
    pub health_check_interval_secs: u32,
    /// 推論タイムアウト（秒）
    pub inference_timeout_secs: u32,
    /// レイテンシ（ミリ秒）
    pub latency_ms: Option<u32>,
    /// 最終確認時刻
    pub last_seen: Option<String>,
    /// 最後のエラーメッセージ
    pub last_error: Option<String>,
    /// 連続エラー回数
    pub error_count: u32,
    /// 登録日時
    pub registered_at: String,
    /// メモ
    pub notes: Option<String>,
    /// デバイス情報（SPEC-f8e3a1b7: /api/systemから取得）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<crate::types::endpoint::DeviceInfo>,
    /// モデル数（一覧取得時）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_count: Option<usize>,
    /// 関連モデル一覧（詳細取得時のみ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<EndpointModelResponse>>,
}

impl From<Endpoint> for EndpointResponse {
    fn from(ep: Endpoint) -> Self {
        EndpointResponse {
            id: ep.id,
            name: ep.name,
            base_url: ep.base_url,
            status: ep.status.as_str().to_string(),
            endpoint_type: ep.endpoint_type.as_str().to_string(),
            health_check_interval_secs: ep.health_check_interval_secs,
            inference_timeout_secs: ep.inference_timeout_secs,
            latency_ms: ep.latency_ms,
            last_seen: ep.last_seen.map(|dt| dt.to_rfc3339()),
            last_error: ep.last_error,
            error_count: ep.error_count,
            registered_at: ep.registered_at.to_rfc3339(),
            notes: ep.notes,
            device_info: ep.device_info,
            model_count: None,
            models: None,
        }
    }
}

/// エンドポイント一覧レスポンス
#[derive(Debug, Serialize)]
pub struct ListEndpointsResponse {
    /// エンドポイント一覧
    pub endpoints: Vec<EndpointResponse>,
    /// 総数
    pub total: usize,
}

/// エンドポイント一覧クエリパラメータ
#[derive(Debug, Deserialize)]
pub struct ListEndpointsQuery {
    /// ステータスでフィルタ（pending, online, offline, error）
    #[serde(default)]
    pub status: Option<String>,
    /// タイプでフィルタ（xllm, ollama, vllm, openai_compatible, unknown）
    /// SPEC-e8e9326e
    #[serde(default, rename = "type")]
    pub endpoint_type: Option<String>,
}

/// モデル一覧レスポンス
#[derive(Debug, Serialize)]
pub struct EndpointModelsResponse {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// モデル一覧
    pub models: Vec<EndpointModelResponse>,
}

/// モデル同期レスポンス
#[derive(Debug, Serialize)]
pub struct SyncModelsResponse {
    /// 同期されたモデル一覧
    pub synced_models: Vec<EndpointModelResponse>,
    /// 追加されたモデル数
    pub added: usize,
    /// 削除されたモデル数
    pub removed: usize,
    /// 更新されたモデル数
    pub updated: usize,
}

/// モデルレスポンス
#[derive(Debug, Serialize)]
pub struct EndpointModelResponse {
    /// モデルID
    pub model_id: String,
    /// 能力（chat, embeddings等）
    pub capabilities: Option<Vec<String>>,
    /// 最大トークン数（xLLM/Ollamaで取得される場合がある）
    pub max_tokens: Option<u32>,
    /// 最終確認時刻
    pub last_checked: Option<String>,
    /// 正規名（HFリポ名）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
}

impl From<EndpointModel> for EndpointModelResponse {
    fn from(m: EndpointModel) -> Self {
        EndpointModelResponse {
            model_id: m.model_id,
            capabilities: m.capabilities,
            max_tokens: m.max_tokens,
            last_checked: m.last_checked.map(|dt| dt.to_rfc3339()),
            canonical_name: m.canonical_name,
        }
    }
}

/// 接続テストのエンドポイント情報
#[derive(Debug, Serialize)]
pub struct EndpointTestInfo {
    /// 発見されたモデル数
    pub model_count: usize,
}

/// 接続テスト結果
#[derive(Debug, Serialize)]
pub struct TestConnectionResponse {
    /// 成功フラグ
    pub success: bool,
    /// レイテンシ（ミリ秒）
    pub latency_ms: Option<u32>,
    /// エラーメッセージ
    pub error: Option<String>,
    /// 発見されたモデル一覧
    pub models_found: Option<Vec<String>>,
    /// エンドポイント情報（成功時のみ）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_info: Option<EndpointTestInfo>,
}

// --- SPEC-e8e9326e: ダウンロード・メタデータ関連型 ---

/// ダウンロードリクエスト（SPEC-e8e9326e）
#[derive(Debug, Deserialize)]
pub struct DownloadModelRequest {
    /// ダウンロードするモデル名
    pub model: String,
    /// HuggingFaceリポジトリ（LM Studio用、任意）
    #[serde(default)]
    pub hf_repo: Option<String>,
    /// 量子化タイプ（LM Studio用、任意。デフォルト: "Q4_K_M"）
    #[serde(default)]
    pub quantization: Option<String>,
}

/// ダウンロードタスクレスポンス（SPEC-e8e9326e）
#[derive(Debug, Serialize)]
pub struct DownloadTaskResponse {
    /// タスクID
    pub task_id: String,
    /// モデル名
    pub model: String,
    /// ステータス
    pub status: String,
    /// 進捗（0.0-100.0）
    pub progress: f64,
    /// ダウンロード速度（MB/s）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<f64>,
    /// 残り時間（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<u32>,
    /// エラーメッセージ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl From<ModelDownloadTask> for DownloadTaskResponse {
    fn from(task: ModelDownloadTask) -> Self {
        DownloadTaskResponse {
            task_id: task.id,
            model: task.model,
            status: task.status.as_str().to_string(),
            progress: task.progress,
            speed_mbps: task.speed_mbps,
            eta_seconds: task.eta_seconds,
            error_message: task.error_message,
        }
    }
}

/// ダウンロード進捗一覧レスポンス（SPEC-e8e9326e）
#[derive(Debug, Serialize)]
pub struct DownloadProgressResponse {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// ダウンロードタスク一覧
    pub tasks: Vec<DownloadTaskResponse>,
}

/// モデル情報レスポンス（SPEC-e8e9326e）
#[derive(Debug, Serialize)]
pub struct ModelInfoResponse {
    /// モデルID
    pub model_id: String,
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// 最大トークン数（コンテキスト長）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 最終確認時刻
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<String>,
}

/// エラーレスポンス
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// エラーメッセージ
    pub error: String,
    /// エラーコード
    pub code: String,
}

/// モデル削除リクエストボディ
#[derive(Debug, Deserialize)]
pub struct DeleteModelRequest {
    /// 削除するモデル名
    pub model: String,
}

/// モデル情報取得のパスパラメータ
#[derive(Debug, Deserialize)]
pub struct ModelInfoPath {
    /// エンドポイントID
    pub id: Uuid,
    /// モデルID
    pub model: String,
}
