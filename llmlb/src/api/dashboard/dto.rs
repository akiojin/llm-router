//! ダッシュボード HTTP レスポンス DTO（/api/dashboard/* が返す view 型）
//!
//! arch-review [H6] round2: api/dashboard.rs からレスポンス DTO を分離。

use crate::balancer::RequestHistoryPoint;
use crate::types::endpoint::{EndpointStatus, EndpointType};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// エンドポイントのダッシュボード表示用サマリー
///
/// SPEC-e8e9326e: llmlb主導エンドポイント登録システム
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardEndpoint {
    /// エンドポイントID
    pub id: Uuid,
    /// 表示名
    pub name: String,
    /// ベースURL
    pub base_url: String,
    /// 現在の状態
    pub status: EndpointStatus,
    /// エンドポイントタイプ（xLLM/Ollama/vLLM 等）
    pub endpoint_type: EndpointType,
    /// ヘルスチェック間隔（秒）
    pub health_check_interval_secs: u32,
    /// 推論タイムアウト（秒）
    pub inference_timeout_secs: u32,
    /// レイテンシ（ミリ秒）
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
    /// 利用可能なモデル数
    pub model_count: usize,
    /// 累計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
}

/// システム統計レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardStats {
    /// 登録ランタイム総数
    #[serde(rename = "total_runtimes", alias = "total_nodes")]
    pub total_nodes: usize,
    /// オンラインランタイム数
    #[serde(rename = "online_runtimes", alias = "online_nodes")]
    pub online_nodes: usize,
    /// 承認待ちランタイム数
    #[serde(rename = "pending_runtimes", alias = "pending_nodes")]
    pub pending_nodes: usize,
    /// 登録中ランタイム数
    #[serde(rename = "registering_runtimes", alias = "registering_nodes")]
    pub registering_nodes: usize,
    /// オフラインランタイム数
    #[serde(rename = "offline_runtimes", alias = "offline_nodes")]
    pub offline_nodes: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 処理中リクエスト数
    pub total_active_requests: u32,
    /// 待機中リクエスト数
    pub queued_requests: usize,
    /// 平均レスポンスタイム
    pub average_response_time_ms: Option<f32>,
    /// 平均GPU使用率
    pub average_gpu_usage: Option<f32>,
    /// 平均GPUメモリ使用率
    pub average_gpu_memory_usage: Option<f32>,
    /// 最新メトリクス更新時刻
    pub last_metrics_updated_at: Option<DateTime<Utc>>,
    /// 最新登録日時
    pub last_registered_at: Option<DateTime<Utc>>,
    /// 最新ヘルスチェック時刻
    pub last_seen_at: Option<DateTime<Utc>>,
    /// OPENAI_API_KEY が設定されているか
    pub openai_key_present: bool,
    /// GOOGLE_API_KEY が設定されているか
    pub google_key_present: bool,
    /// ANTHROPIC_API_KEY が設定されているか
    pub anthropic_key_present: bool,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
}

/// 運用監視向けの主要状態サマリー
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardOperations {
    /// 総合状態（healthy / attention / empty）
    pub health: String,
    /// 登録エンドポイント総数
    pub total_endpoints: usize,
    /// オンラインエンドポイント数
    pub online_endpoints: usize,
    /// 承認待ちエンドポイント数
    pub pending_endpoints: usize,
    /// 登録中エンドポイント数
    pub registering_endpoints: usize,
    /// オフラインエンドポイント数
    pub offline_endpoints: usize,
    /// エラー状態エンドポイント数
    pub error_endpoints: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 成功率（0-100）
    pub success_rate: Option<f32>,
    /// 処理中リクエスト数
    pub active_requests: u32,
    /// 待機中リクエスト数
    pub queued_requests: usize,
    /// 平均応答時間
    pub average_response_time_ms: Option<f32>,
    /// 出力TPS（出力トークン数 / 生成時間秒）
    pub output_tps: Option<f64>,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
    /// 最新登録日時
    pub last_registered_at: Option<DateTime<Utc>>,
    /// 最新ヘルスチェック時刻
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// Dashboardで使う容量・能力サマリー
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardCapacity {
    /// 登録済みモデル合計
    pub total_models: usize,
    /// GPU能力がある、またはGPU情報を返したエンドポイント数
    pub gpu_capable_endpoints: usize,
    /// GPUメモリ情報を取得できたエンドポイント数
    pub gpu_telemetry_endpoints: usize,
    /// GPU総メモリ（バイト）
    pub total_gpu_memory_bytes: Option<u64>,
    /// GPU使用中メモリ（バイト）
    pub used_gpu_memory_bytes: Option<u64>,
    /// GPUメモリ使用率（0-100）
    pub gpu_memory_usage_percent: Option<f32>,
    /// GPUテレメトリ状態（available / partial / unavailable）
    pub telemetry_status: String,
}

/// Dashboard上の要対応項目
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardActionItem {
    /// severity（critical / warning / info）
    pub severity: String,
    /// 表示タイトル
    pub title: String,
    /// 補足説明
    pub detail: String,
    /// 対象件数
    pub count: usize,
}

/// ダッシュボード概要レスポンス
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardOverview {
    /// エンドポイント一覧（SPEC-e8e9326e）
    pub endpoints: Vec<DashboardEndpoint>,
    /// 運用監視向けサマリー
    pub operations: DashboardOperations,
    /// 容量・能力サマリー
    pub capacity: DashboardCapacity,
    /// 要対応項目
    pub action_items: Vec<DashboardActionItem>,
    /// リクエスト履歴
    pub history: Vec<RequestHistoryPoint>,
    /// エンドポイント別TPS概要（SPEC-4bb5b55f T023）
    pub endpoint_tps: Vec<crate::balancer::EndpointTpsSummary>,
    /// レスポンス生成時刻
    pub generated_at: DateTime<Utc>,
    /// 集計に要した時間（ミリ秒）
    pub generation_time_ms: u64,
}
