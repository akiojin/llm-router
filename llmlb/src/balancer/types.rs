//! バランサーモジュールの型定義
//!
//! ロードバランシングに使用する構造体・列挙型・定数を集約する。

use crate::common::protocol::{TpsApiKind, TpsSource};
use crate::types::HealthMetrics;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
        Arc,
    },
};
use uuid::Uuid;

/// メトリクスを新鮮とみなすための許容秒数
pub(crate) const METRICS_STALE_THRESHOLD_SECS: i64 = 120;
/// リクエスト履歴の保持分数
pub(crate) const REQUEST_HISTORY_WINDOW_MINUTES: i64 = 60;
/// ノードメトリクス履歴の最大保持件数
pub(crate) const METRICS_HISTORY_CAPACITY: usize = 360;

pub(crate) type TpsTrackerKey = (Uuid, String, TpsApiKind);
pub(crate) type TpsTrackerMap = HashMap<TpsTrackerKey, ModelTpsState>;

/// リクエスト結果
#[derive(Debug, Clone, Copy)]
pub enum RequestOutcome {
    /// 正常終了
    Success,
    /// エラー終了
    Error,
    /// キュー待ち
    Queued,
}

/// 待機結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitResult {
    /// ノードが利用可能になった
    Ready,
    /// タイムアウト
    Timeout,
    /// 待機キュー容量超過
    CapacityExceeded,
}

#[derive(Debug)]
pub(crate) struct QueueWaiterGuard {
    waiters: Arc<AtomicUsize>,
}

impl QueueWaiterGuard {
    pub(crate) fn new(waiters: Arc<AtomicUsize>) -> Self {
        Self { waiters }
    }
}

impl Drop for QueueWaiterGuard {
    fn drop(&mut self) {
        self.waiters.fetch_sub(1, AtomicOrdering::SeqCst);
    }
}

// SPEC-f8e3a1b7: Node依存のヘルパー関数は削除されました
// - node_spec_score, compare_spec_levels, compare_spec_by_state
// - compare_option_f32, compare_average_ms, usage_snapshot, compare_usage_levels
// 新しい負荷分散はレイテンシベース（EMA α=0.2）を使用

/// エンドポイント×モデル単位のTPS EMA状態（SPEC-4bb5b55f）
#[derive(Debug, Clone, Default)]
pub struct ModelTpsState {
    /// EMA平滑化されたTPS値（None=未計測）
    pub tps_ema: Option<f64>,
    /// リクエスト完了数
    pub request_count: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 処理時間累計（ミリ秒）
    pub total_duration_ms: u64,
}

impl ModelTpsState {
    /// TPS計測値を更新（EMA α=0.2）
    ///
    /// TPS = output_tokens / (duration_ms / 1000)
    /// EMA: new_ema = α × current_tps + (1 - α) × previous_ema
    pub fn update_tps(&mut self, output_tokens: u64, duration_ms: u64) {
        if duration_ms == 0 {
            return;
        }

        let current_tps = output_tokens as f64 / (duration_ms as f64 / 1000.0);

        const ALPHA: f64 = 0.2;
        self.tps_ema = Some(match self.tps_ema {
            Some(prev) => ALPHA * current_tps + (1.0 - ALPHA) * prev,
            None => current_tps,
        });

        self.request_count += 1;
        self.total_output_tokens += output_tokens;
        self.total_duration_ms += duration_ms;
    }
}

/// エンドポイント×モデル単位のTPS情報（API応答用）（SPEC-4bb5b55f）
#[derive(Debug, Clone, Serialize)]
pub struct ModelTpsInfo {
    /// モデルID
    pub model_id: String,
    /// API種別（chat/completions/responses）
    pub api_kind: TpsApiKind,
    /// 計測元（production / benchmark）
    pub source: TpsSource,
    /// EMA平滑化されたTPS値（None=未計測）
    pub tps: Option<f64>,
    /// リクエスト完了数
    pub request_count: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 平均処理時間（ミリ秒、None=未計測）
    pub average_duration_ms: Option<f64>,
}

/// エンドポイント単位のTPS概要（SPEC-4bb5b55f T023）
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EndpointTpsSummary {
    /// エンドポイントID
    pub endpoint_id: Uuid,
    /// TPS計測済みモデル数
    pub model_count: usize,
    /// 全モデルの加重平均TPS（None=全モデル未計測）
    pub aggregate_tps: Option<f64>,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// リクエスト累計
    pub total_requests: u64,
}

/// エンドポイントの負荷状態
#[derive(Debug, Clone, Default)]
pub(crate) struct EndpointLoadState {
    pub(crate) last_metrics: Option<HealthMetrics>,
    pub(crate) assigned_active: u32,
    pub(crate) total_assigned: u64,
    pub(crate) success_count: u64,
    pub(crate) error_count: u64,
    pub(crate) total_latency_ms: u128,
    pub(crate) metrics_history: VecDeque<HealthMetrics>,
    pub(crate) initializing: bool,
    pub(crate) ready_models: Option<(u8, u8)>,
    /// 入力トークン累計
    pub(crate) total_input_tokens: u64,
    /// 出力トークン累計
    pub(crate) total_output_tokens: u64,
    /// 総トークン累計
    pub(crate) total_tokens: u64,
}

// SPEC-f8e3a1b7: NodeLoadState型エイリアスは削除されました

impl EndpointLoadState {
    pub(crate) fn combined_active(&self) -> u32 {
        let heartbeat_active = self
            .last_metrics
            .as_ref()
            .map(|m| m.active_requests)
            .unwrap_or(0);
        // Avoid double counting when node heartbeat mirrors lb-assigned requests.
        heartbeat_active.max(self.assigned_active)
    }

    pub(crate) fn average_latency_ms(&self) -> Option<f32> {
        let completed = self.success_count + self.error_count;
        if completed == 0 {
            None
        } else {
            Some((self.total_latency_ms as f64 / completed as f64) as f32)
        }
    }

    pub(crate) fn last_updated(&self) -> Option<DateTime<Utc>> {
        self.last_metrics.as_ref().map(|m| m.timestamp)
    }

    pub(crate) fn is_stale(&self, now: DateTime<Utc>) -> bool {
        match self.last_updated() {
            Some(ts) => (now - ts).num_seconds() > METRICS_STALE_THRESHOLD_SECS,
            None => true,
        }
    }

    pub(crate) fn effective_average_ms(&self) -> Option<f32> {
        self.last_metrics
            .as_ref()
            .and_then(|m| m.average_response_time_ms)
            .or_else(|| self.average_latency_ms())
    }

    pub(crate) fn push_metrics(&mut self, metrics: HealthMetrics) {
        self.metrics_history.push_back(metrics);
        if self.metrics_history.len() > METRICS_HISTORY_CAPACITY {
            self.metrics_history.pop_front();
        }
    }
}

/// エンドポイント/ノードのロードスナップショット
///
/// エンドポイントの負荷スナップショット
///
/// SPEC-f8e3a1b7: Node型依存を削除し、Endpoint型を直接使用
#[derive(Debug, Clone, Serialize)]
pub struct EndpointLoadSnapshot {
    /// エンドポイントID（API互換性のためnode_idとしてシリアライズ）
    #[serde(rename = "node_id")]
    pub endpoint_id: Uuid,
    /// エンドポイント名
    pub machine_name: String,
    /// エンドポイント状態
    pub status: crate::types::endpoint::EndpointStatus,
    /// CPU使用率
    pub cpu_usage: Option<f32>,
    /// メモリ使用率
    pub memory_usage: Option<f32>,
    /// GPU使用率
    pub gpu_usage: Option<f32>,
    /// GPUメモリ使用率
    pub gpu_memory_usage: Option<f32>,
    /// GPUメモリ総容量 (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU使用メモリ (MB)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU温度 (℃)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_temperature: Option<f32>,
    /// GPUモデル名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_model_name: Option<String>,
    /// GPU計算能力
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_capability_score: Option<u32>,
    /// 処理中リクエスト数（load balancer観点+ノード自己申告）
    pub active_requests: u32,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 平均レスポンスタイム (ms)
    pub average_response_time_ms: Option<f32>,
    /// メトリクス最終更新時刻
    pub last_updated: Option<DateTime<Utc>>,
    /// メトリクスが鮮度閾値を超えているか
    pub is_stale: bool,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
}

/// システム全体の統計サマリー
#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemSummary {
    /// 登録ノード総数
    pub total_nodes: usize,
    /// オンラインノード数
    pub online_nodes: usize,
    /// 承認待ちノード数
    pub pending_nodes: usize,
    /// 登録中ノード数（モデル同期中）
    pub registering_nodes: usize,
    /// オフラインノード数
    pub offline_nodes: usize,
    /// 累積リクエスト数
    pub total_requests: u64,
    /// 成功リクエスト数
    pub successful_requests: u64,
    /// 失敗リクエスト数
    pub failed_requests: u64,
    /// 平均レスポンスタイム (ms)
    pub average_response_time_ms: Option<f32>,
    /// 平均GPU使用率 (0-100)
    pub average_gpu_usage: Option<f32>,
    /// 平均GPUメモリ使用率 (0-100)
    pub average_gpu_memory_usage: Option<f32>,
    /// 処理中リクエスト総数
    pub total_active_requests: u32,
    /// 待機中リクエスト総数
    pub queued_requests: usize,
    /// 最新メトリクス更新時刻
    pub last_metrics_updated_at: Option<DateTime<Utc>>,
    /// 入力トークン累計
    pub total_input_tokens: u64,
    /// 出力トークン累計
    pub total_output_tokens: u64,
    /// 総トークン累計
    pub total_tokens: u64,
}

/// ハートビートから記録するメトリクス値
#[derive(Debug, Clone)]
pub struct MetricsUpdate {
    /// 対象エンドポイントのID
    pub endpoint_id: Uuid,
    /// CPU使用率（パーセンテージ）
    pub cpu_usage: f32,
    /// メモリ使用率（パーセンテージ）
    pub memory_usage: f32,
    /// GPU使用率（パーセンテージ）
    pub gpu_usage: Option<f32>,
    /// GPUメモリ使用率（パーセンテージ）
    pub gpu_memory_usage: Option<f32>,
    /// GPUメモリ総容量 (MB)
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU使用メモリ (MB)
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU温度 (℃)
    pub gpu_temperature: Option<f32>,
    /// GPUモデル名
    pub gpu_model_name: Option<String>,
    /// GPU計算能力
    pub gpu_compute_capability: Option<String>,
    /// GPU能力スコア
    pub gpu_capability_score: Option<u32>,
    /// アクティブなリクエスト数
    pub active_requests: u32,
    /// 平均レスポンスタイム（ミリ秒）
    pub average_response_time_ms: Option<f32>,
    /// 初期化中フラグ
    pub initializing: bool,
    /// 起動済みモデル数/総数
    pub ready_models: Option<(u8, u8)>,
}

/// リクエスト履歴ポイント
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct RequestHistoryPoint {
    /// 分単位のタイムスタンプ
    pub minute: DateTime<Utc>,
    /// 成功数
    pub success: u64,
    /// 失敗数
    pub error: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashSet;

    // ── ModelTpsState tests ──

    #[test]
    fn model_tps_state_default_is_none() {
        let s = ModelTpsState::default();
        assert!(s.tps_ema.is_none());
        assert_eq!(s.request_count, 0);
        assert_eq!(s.total_output_tokens, 0);
        assert_eq!(s.total_duration_ms, 0);
    }

    #[test]
    fn update_tps_first_call_sets_ema_to_current() {
        let mut s = ModelTpsState::default();
        // 100 tokens / 1000ms = 100 tokens/s
        s.update_tps(100, 1000);
        assert!((s.tps_ema.unwrap() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn update_tps_second_call_applies_ema() {
        let mut s = ModelTpsState::default();
        // First: 100 t / 1s = 100 tps -> ema = 100
        s.update_tps(100, 1000);
        // Second: 200 t / 1s = 200 tps -> ema = 0.2*200 + 0.8*100 = 120
        s.update_tps(200, 1000);
        assert!((s.tps_ema.unwrap() - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn update_tps_zero_duration_is_noop() {
        let mut s = ModelTpsState::default();
        s.update_tps(100, 0);
        assert!(s.tps_ema.is_none());
        assert_eq!(s.request_count, 0);
        assert_eq!(s.total_output_tokens, 0);
        assert_eq!(s.total_duration_ms, 0);
    }

    #[test]
    fn update_tps_accumulates_counters() {
        let mut s = ModelTpsState::default();
        s.update_tps(50, 500);
        s.update_tps(150, 1500);
        assert_eq!(s.request_count, 2);
        assert_eq!(s.total_output_tokens, 200);
        assert_eq!(s.total_duration_ms, 2000);
    }

    // ── EndpointLoadState tests ──

    fn make_metrics(active: u32, ts: DateTime<Utc>) -> HealthMetrics {
        HealthMetrics {
            endpoint_id: Uuid::nil(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: active,
            total_requests: 0,
            average_response_time_ms: None,
            timestamp: ts,
        }
    }

    #[test]
    fn combined_active_no_metrics_uses_assigned() {
        let s = EndpointLoadState {
            assigned_active: 5,
            ..Default::default()
        };
        assert_eq!(s.combined_active(), 5);
    }

    #[test]
    fn combined_active_takes_max_of_heartbeat_and_assigned() {
        let now = Utc::now();
        let s = EndpointLoadState {
            last_metrics: Some(make_metrics(10, now)),
            assigned_active: 3,
            ..Default::default()
        };
        assert_eq!(s.combined_active(), 10);

        let s2 = EndpointLoadState {
            last_metrics: Some(make_metrics(2, now)),
            assigned_active: 7,
            ..Default::default()
        };
        assert_eq!(s2.combined_active(), 7);
    }

    #[test]
    fn average_latency_ms_no_completed_returns_none() {
        let s = EndpointLoadState::default();
        assert!(s.average_latency_ms().is_none());
    }

    #[test]
    fn average_latency_ms_with_completed() {
        let s = EndpointLoadState {
            success_count: 3,
            error_count: 1,
            total_latency_ms: 800,
            ..Default::default()
        };
        // 800 / 4 = 200
        assert!((s.average_latency_ms().unwrap() - 200.0).abs() < 0.01);
    }

    #[test]
    fn is_stale_no_metrics_returns_true() {
        let s = EndpointLoadState::default();
        assert!(s.is_stale(Utc::now()));
    }

    #[test]
    fn is_stale_fresh_metrics_returns_false() {
        let now = Utc::now();
        let s = EndpointLoadState {
            last_metrics: Some(make_metrics(0, now)),
            ..Default::default()
        };
        assert!(!s.is_stale(now));
    }

    #[test]
    fn is_stale_old_metrics_returns_true() {
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(METRICS_STALE_THRESHOLD_SECS + 1);
        let s = EndpointLoadState {
            last_metrics: Some(make_metrics(0, old)),
            ..Default::default()
        };
        assert!(s.is_stale(now));
    }

    #[test]
    fn push_metrics_respects_capacity() {
        let mut s = EndpointLoadState::default();
        let now = Utc::now();
        for i in 0..METRICS_HISTORY_CAPACITY + 5 {
            s.push_metrics(make_metrics(i as u32, now));
        }
        assert_eq!(s.metrics_history.len(), METRICS_HISTORY_CAPACITY);
    }

    #[test]
    fn effective_average_ms_prefers_heartbeat() {
        let now = Utc::now();
        let mut metrics = make_metrics(0, now);
        metrics.average_response_time_ms = Some(42.0);
        let s = EndpointLoadState {
            last_metrics: Some(metrics),
            success_count: 2,
            total_latency_ms: 200,
            ..Default::default()
        };
        // Should use heartbeat value (42.0), not computed average (100.0)
        assert!((s.effective_average_ms().unwrap() - 42.0).abs() < 0.01);
    }

    #[test]
    fn effective_average_ms_falls_back_to_computed() {
        let now = Utc::now();
        let s = EndpointLoadState {
            last_metrics: Some(make_metrics(0, now)),
            success_count: 4,
            total_latency_ms: 400,
            ..Default::default()
        };
        // heartbeat has no average_response_time_ms, falls back to 400/4=100
        assert!((s.effective_average_ms().unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn last_updated_returns_timestamp() {
        let now = Utc::now();
        let s = EndpointLoadState {
            last_metrics: Some(make_metrics(0, now)),
            ..Default::default()
        };
        assert_eq!(s.last_updated(), Some(now));

        let s2 = EndpointLoadState::default();
        assert!(s2.last_updated().is_none());
    }

    // ── Serialization / type tests ──

    #[test]
    fn system_summary_default_values() {
        let s = SystemSummary::default();
        assert_eq!(s.total_nodes, 0);
        assert_eq!(s.online_nodes, 0);
        assert_eq!(s.total_requests, 0);
        assert!(s.average_response_time_ms.is_none());
        assert!(s.last_metrics_updated_at.is_none());
    }

    #[test]
    fn endpoint_tps_summary_partial_eq() {
        let id = Uuid::new_v4();
        let a = EndpointTpsSummary {
            endpoint_id: id,
            model_count: 2,
            aggregate_tps: Some(50.0),
            total_output_tokens: 1000,
            total_requests: 10,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn request_history_point_hash_and_eq() {
        let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let p1 = RequestHistoryPoint {
            minute: ts,
            success: 10,
            error: 2,
        };
        let p2 = p1.clone();
        assert_eq!(p1, p2);

        let mut set = HashSet::new();
        set.insert(p1.clone());
        set.insert(p2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn model_tps_info_serialization() {
        let info = ModelTpsInfo {
            model_id: "test-model".to_string(),
            api_kind: TpsApiKind::ChatCompletions,
            source: TpsSource::Production,
            tps: Some(42.5),
            request_count: 100,
            total_output_tokens: 5000,
            average_duration_ms: Some(120.0),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["model_id"], "test-model");
        assert_eq!(json["request_count"], 100);
        assert_eq!(json["tps"], 42.5);
    }

    #[test]
    fn endpoint_load_snapshot_serialization() {
        let snap = EndpointLoadSnapshot {
            endpoint_id: Uuid::nil(),
            machine_name: "test-node".to_string(),
            status: crate::types::endpoint::EndpointStatus::Online,
            cpu_usage: Some(50.0),
            memory_usage: Some(60.0),
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 3,
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
            average_response_time_ms: Some(150.0),
            last_updated: None,
            is_stale: false,
            total_input_tokens: 1000,
            total_output_tokens: 2000,
            total_tokens: 3000,
        };
        let json = serde_json::to_value(&snap).unwrap();
        // endpoint_id is renamed to node_id for API compatibility
        assert!(json.get("node_id").is_some());
        assert!(json.get("endpoint_id").is_none());
        assert_eq!(json["machine_name"], "test-node");
        assert_eq!(json["active_requests"], 3);
    }
}
