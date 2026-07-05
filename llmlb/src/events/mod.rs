//! ダッシュボードイベントバス
//!
//! エンドポイント登録・状態変化・メトリクス更新などのイベントを
//! WebSocketクライアントにブロードキャストするための基盤

use crate::types::endpoint::EndpointStatus;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// イベントバスのチャネル容量
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// ダッシュボードイベント
///
/// WebSocketクライアントに送信されるイベントの種類
#[derive(Debug, Clone, Serialize)]
// arch-review [M15]: 内部語彙を endpoint_* / Endpoint* に統一する。
// JSON ワイヤ契約（type タグ・フィールド名）は SPA 互換のため `#[serde(rename)]` で
// 従来値（NodeRegistered / NodeRemoved / endpoint_id / endpoint_name）を厳密に維持する。
#[serde(tag = "type", content = "data")]
pub enum DashboardEvent {
    /// エンドポイント登録イベント
    #[serde(rename = "NodeRegistered")]
    EndpointRegistered {
        /// エンドポイントID
        #[serde(rename = "runtime_id")]
        endpoint_id: Uuid,
        /// エンドポイント名
        #[serde(rename = "machine_name")]
        endpoint_name: String,
        /// IPアドレス
        ip_address: String,
        /// ステータス
        status: EndpointStatus,
    },
    /// エンドポイント状態変化イベント
    EndpointStatusChanged {
        /// エンドポイントID
        #[serde(rename = "runtime_id")]
        endpoint_id: Uuid,
        /// 旧ステータス
        old_status: EndpointStatus,
        /// 新ステータス
        new_status: EndpointStatus,
    },
    /// メトリクス更新イベント
    MetricsUpdated {
        /// エンドポイントID
        #[serde(rename = "runtime_id")]
        endpoint_id: Uuid,
        /// CPU使用率
        cpu_usage: Option<f32>,
        /// メモリ使用率
        memory_usage: Option<f32>,
        /// GPU使用率
        gpu_usage: Option<f32>,
    },
    /// エンドポイント削除イベント
    #[serde(rename = "NodeRemoved")]
    EndpointRemoved {
        /// エンドポイントID
        #[serde(rename = "runtime_id")]
        endpoint_id: Uuid,
    },
    /// アップデート状態変更イベント
    ///
    /// アップデートチェック・適用・ロールバック・スケジュール操作後に発行
    UpdateStateChanged,
    /// TPS更新イベント（SPEC-4bb5b55f）
    TpsUpdated {
        /// エンドポイントID
        endpoint_id: Uuid,
        /// モデルID
        model_id: String,
        /// TPS（tokens/sec）
        tps: f64,
        /// 出力トークン数
        output_tokens: u32,
        /// 処理時間（ミリ秒）
        duration_ms: u64,
    },
}

/// ダッシュボードイベントバス
///
/// ノード状態変化などのイベントをWebSocketクライアントにブロードキャストする
#[derive(Clone)]
pub struct DashboardEventBus {
    sender: broadcast::Sender<DashboardEvent>,
}

impl Default for DashboardEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardEventBus {
    /// 新しいイベントバスを作成
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self { sender }
    }

    /// イベントバスを購読
    ///
    /// WebSocketハンドラーがイベントを受信するために使用
    pub fn subscribe(&self) -> broadcast::Receiver<DashboardEvent> {
        self.sender.subscribe()
    }

    /// イベントを発行
    ///
    /// 購読者がいない場合でもエラーにはならない
    pub fn publish(&self, event: DashboardEvent) {
        // 購読者がいない場合は送信に失敗するが、無視する
        let _ = self.sender.send(event);
    }

    /// 現在の購読者数を取得
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// Arc でラップされたイベントバス
pub type SharedEventBus = Arc<DashboardEventBus>;

/// 共有可能なイベントバスを作成
pub fn create_shared_event_bus() -> SharedEventBus {
    Arc::new(DashboardEventBus::new())
}

#[cfg(test)]
mod tests;
