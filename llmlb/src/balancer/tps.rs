//! LoadManager の TPS（tokens per second）状態ライフサイクル
//!
//! arch-review [H1]: LoadManager は TPS 追跡・負荷・履歴・受付制御・選択を 1 構造体に
//! 集約した God object。まず tps_tracker/state に閉じた TPS 状態の更新・取得・破棄を
//! submodule の `impl LoadManager` として切り出し、選択ロジック等と分離する。
//! フィールドは同一クレートの子モジュールから参照でき、公開 API は不変。

use super::{EndpointTpsSummary, LoadManager, ModelTpsInfo};
use crate::common::protocol::{TpsApiKind, TpsSource};
use std::collections::HashMap;
use uuid::Uuid;

impl LoadManager {
    /// TPS計測値を更新（SPEC-4bb5b55f）
    pub async fn update_tps(
        &self,
        endpoint_id: Uuid,
        model_id: String,
        api_kind: TpsApiKind,
        output_tokens: u64,
        duration_ms: u64,
    ) {
        if duration_ms == 0 || output_tokens == 0 {
            return;
        }
        let mut tracker = self.tps_tracker.write().await;
        // オフライン/エラー遷移で clear_tps_for_endpoint された後に、in-flight リクエスト
        // 完了の遅延 update_tps が TPS エントリを再生成するのを防ぐ。
        // tps_tracker の書き込みロックを保持したまま現在のステータスを確認することで、
        // clear_tps_for_endpoint（同じロックを取得）との競合を直列化し、
        // ステータス確認と再生成の間の TOCTOU 競合を排除する。
        // レジストリ未登録（None）の場合は従来どおり更新する（テスト互換・production では
        // 削除直後の一時的な遅延更新のみが該当し、forget_endpoint が別途状態を掃除する）。
        if let Some(endpoint) = self.endpoint_registry.get(endpoint_id).await {
            if matches!(
                endpoint.status,
                crate::types::endpoint::EndpointStatus::Offline
                    | crate::types::endpoint::EndpointStatus::Error
            ) {
                return;
            }
        }
        let state = tracker
            .entry((endpoint_id, model_id, api_kind))
            .or_default();
        state.update_tps(output_tokens, duration_ms);
    }

    /// 指定エンドポイントのTPS状態をクリアする。
    ///
    /// エンドポイントがoffline/errorへ遷移した際に、復帰後は再計測から始める。
    pub async fn clear_tps_for_endpoint(&self, endpoint_id: Uuid) {
        let mut tracker = self.tps_tracker.write().await;
        tracker.retain(|(eid, _, _), _| *eid != endpoint_id);
    }

    /// エンドポイント削除時に LoadManager が保持する状態を破棄する。
    ///
    /// `EndpointRegistry::remove` はレジストリ・DB・モデルマッピングのみを掃除するため、
    /// LoadManager 側の `state`（負荷状態）と `tps_tracker`（TPS 状態）は残存しリークする。
    /// これらは `begin_request` / `update_tps` の `entry().or_default()` で挿入されるが
    /// 削除契機がないため、本メソッドを削除ハンドラから呼び出して完全に除去する。
    pub async fn forget_endpoint(&self, endpoint_id: Uuid) {
        {
            let mut state = self.state.write().await;
            state.remove(&endpoint_id);
        }
        {
            let mut tracker = self.tps_tracker.write().await;
            tracker.retain(|(eid, _, _), _| *eid != endpoint_id);
        }
    }

    /// エンドポイントのモデル別TPS情報を取得（SPEC-4bb5b55f）
    pub async fn get_model_tps(&self, endpoint_id: Uuid) -> Vec<ModelTpsInfo> {
        let tracker = self.tps_tracker.read().await;
        tracker
            .iter()
            .filter(|((eid, _, _), _)| *eid == endpoint_id)
            .map(|((_, model_id, api_kind), state)| ModelTpsInfo {
                model_id: model_id.clone(),
                api_kind: *api_kind,
                source: TpsSource::Production,
                tps: state.tps_ema,
                request_count: state.request_count,
                total_output_tokens: state.total_output_tokens,
                average_duration_ms: if state.request_count > 0 {
                    Some(state.total_duration_ms as f64 / state.request_count as f64)
                } else {
                    None
                },
            })
            .collect()
    }

    /// 全エンドポイントのTPS概要を返す（SPEC-4bb5b55f T023）
    pub async fn get_all_endpoint_tps(&self) -> Vec<EndpointTpsSummary> {
        let tracker = self.tps_tracker.read().await;
        let mut map: HashMap<Uuid, EndpointTpsSummary> = HashMap::new();
        let mut model_sets: HashMap<Uuid, std::collections::HashSet<&str>> = HashMap::new();
        let mut total_durations: HashMap<Uuid, u64> = HashMap::new();

        for ((endpoint_id, model_id, _), state) in tracker.iter() {
            let entry = map
                .entry(*endpoint_id)
                .or_insert_with(|| EndpointTpsSummary {
                    endpoint_id: *endpoint_id,
                    model_count: 0,
                    aggregate_tps: None,
                    total_output_tokens: 0,
                    total_requests: 0,
                });
            model_sets
                .entry(*endpoint_id)
                .or_default()
                .insert(model_id.as_str());
            entry.total_output_tokens += state.total_output_tokens;
            entry.total_requests += state.request_count;
            *total_durations.entry(*endpoint_id).or_default() += state.total_duration_ms;
        }

        // model_count と aggregate_tps を1エンドポイントあたり1回で確定させる（旧実装の
        // O(E×N) 全走査を排除）。集計トークン/時間は第1ループで積んだ値をそのまま使う。
        for (endpoint_id, entry) in map.iter_mut() {
            if let Some(model_set) = model_sets.get(endpoint_id) {
                entry.model_count = model_set.len();
            }
            let total_duration = total_durations.get(endpoint_id).copied().unwrap_or(0);
            if total_duration > 0 {
                entry.aggregate_tps =
                    Some(entry.total_output_tokens as f64 / (total_duration as f64 / 1000.0));
            }
        }

        map.into_values().collect()
    }
}
