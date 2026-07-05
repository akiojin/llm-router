//! LoadManager のエンドポイント選択ロジック（TPS 優先 + ラウンドロビン）
//!
//! arch-review [H1]: LoadManager は TPS 追跡・負荷・履歴・受付制御・選択を集約した
//! God object。オンライン集約・直接選択・モデル別選択・候補からの選択などの
//! エンドポイント選択群を submodule の `impl LoadManager` として切り出した。
//! 子モジュールから private メソッド/フィールドを参照でき、公開 API は不変。

use super::LoadManager;
use crate::common::error::{LbError, RouterResult};
use crate::common::protocol::TpsApiKind;
use std::collections::HashMap;
use std::sync::atomic::Ordering as AtomicOrdering;
use uuid::Uuid;

impl LoadManager {
    async fn collect_online_endpoints(
        &self,
        model_id: Option<&str>,
    ) -> RouterResult<Vec<crate::types::endpoint::Endpoint>> {
        if let Some(model_id) = model_id {
            let endpoints = self.endpoint_registry.find_by_model(model_id).await;
            if endpoints.is_empty() {
                return Err(LbError::NoCapableEndpoints(model_id.to_string()));
            }
            return Ok(endpoints);
        }

        let endpoints = self.endpoint_registry.list_online().await;
        if endpoints.is_empty() {
            return Err(LbError::NoEndpointsAvailable);
        }

        Ok(endpoints)
    }

    /// エンドポイントをTPS優先で直接選択する。
    ///
    /// `api_kind` を指定した場合、そのAPI種別の集計TPSを優先度に用いる。
    /// 未計測エンドポイントはTPS=0.0として最低優先になる。
    pub async fn select_endpoint_by_tps_direct(
        &self,
        api_kind: Option<TpsApiKind>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        let endpoints = self.collect_online_endpoints(None).await?;
        self.select_endpoint_by_tps_from_endpoints(endpoints, None, api_kind)
            .await
    }

    /// 指定モデルに対応する初期化完了エンドポイントをラウンドロビンで選択
    pub async fn select_endpoint_round_robin_ready_for_model(
        &self,
        model_id: &str,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        let endpoints = self.collect_online_endpoints(Some(model_id)).await?;
        let ready_endpoints: Vec<_> = {
            let state = self.state.read().await;
            endpoints
                .into_iter()
                .filter(|ep| {
                    state
                        .get(&ep.id)
                        .map(|load| !load.initializing)
                        .unwrap_or(true)
                })
                .collect()
        };

        self.select_endpoint_round_robin_from_endpoints(ready_endpoints)
    }

    /// 指定モデルに対応する初期化完了エンドポイントをTPS優先で選択する。
    ///
    /// 実装上は初期化中除外を共通処理で行うため、TPS優先選択の標準経路として使う。
    pub async fn select_endpoint_by_tps_ready_for_model(
        &self,
        model_id: &str,
        api_kind: Option<TpsApiKind>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        let endpoints = self.collect_online_endpoints(Some(model_id)).await?;
        self.select_endpoint_by_tps_from_endpoints(endpoints, Some(model_id), api_kind)
            .await
    }

    /// 指定済み候補から、指定モデルに対応する初期化完了エンドポイントをTPS優先で選択する。
    pub async fn select_endpoint_by_tps_ready_from_candidates(
        &self,
        endpoints: Vec<crate::types::endpoint::Endpoint>,
        model_id: &str,
        api_kind: Option<TpsApiKind>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        self.select_endpoint_by_tps_from_endpoints(endpoints, Some(model_id), api_kind)
            .await
    }

    fn select_endpoint_round_robin_from_endpoints(
        &self,
        endpoints: Vec<crate::types::endpoint::Endpoint>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        if endpoints.is_empty() {
            return Err(LbError::NoEndpointsAvailable);
        }

        let cursor = self.round_robin.fetch_add(1, AtomicOrdering::SeqCst);
        let index = cursor % endpoints.len();

        Ok(endpoints[index].clone())
    }
    async fn compute_endpoint_tps_scores(
        &self,
        endpoints: &[crate::types::endpoint::Endpoint],
        model_id: Option<&str>,
        api_kind: Option<TpsApiKind>,
    ) -> HashMap<Uuid, f64> {
        let tracker = self.tps_tracker.read().await;
        let mut scores = HashMap::with_capacity(endpoints.len());

        for endpoint in endpoints {
            let score = if let Some(model_id) = model_id {
                let Some(api_kind) = api_kind else {
                    scores.insert(endpoint.id, 0.0);
                    continue;
                };

                tracker
                    .iter()
                    .filter(|((eid, mid, kind), _)| {
                        *eid == endpoint.id && mid == model_id && *kind == api_kind
                    })
                    .filter_map(|(_, state)| state.tps_ema)
                    .fold(0.0, f64::max)
            } else {
                let (total_tokens, total_duration_ms) = tracker
                    .iter()
                    .filter(|((eid, _, kind), _)| {
                        *eid == endpoint.id && api_kind.is_none_or(|expected| *kind == expected)
                    })
                    .fold((0u64, 0u64), |(tokens, duration), (_, state)| {
                        (
                            tokens.saturating_add(state.total_output_tokens),
                            duration.saturating_add(state.total_duration_ms),
                        )
                    });

                if total_duration_ms > 0 {
                    total_tokens as f64 / (total_duration_ms as f64 / 1000.0)
                } else {
                    0.0
                }
            };

            scores.insert(endpoint.id, score);
        }

        scores
    }

    async fn select_endpoint_by_tps_from_endpoints(
        &self,
        endpoints: Vec<crate::types::endpoint::Endpoint>,
        model_id: Option<&str>,
        api_kind: Option<TpsApiKind>,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        if endpoints.is_empty() {
            return Err(match model_id {
                Some(model_id) => LbError::NoCapableEndpoints(model_id.to_string()),
                None => LbError::NoEndpointsAvailable,
            });
        }

        let candidates: Vec<_> = {
            let state = self.state.read().await;
            endpoints
                .into_iter()
                .filter(|ep| {
                    state
                        .get(&ep.id)
                        .map(|load| !load.initializing)
                        .unwrap_or(true)
                })
                .collect()
        };

        if candidates.is_empty() {
            return Err(LbError::NoEndpointsAvailable);
        }

        let scores = self
            .compute_endpoint_tps_scores(&candidates, model_id, api_kind)
            .await;
        let round_robin_cursor = self.round_robin.fetch_add(1, AtomicOrdering::SeqCst);
        let round_robin_start = round_robin_cursor % candidates.len().max(1);
        let round_robin_priority =
            compute_round_robin_priority_for_endpoints(&candidates, round_robin_start);

        let mut ordered = candidates;
        ordered.sort_by(|a, b| {
            let a_score = scores.get(&a.id).copied().unwrap_or(0.0);
            let b_score = scores.get(&b.id).copied().unwrap_or(0.0);

            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_rank = round_robin_priority
                        .get(&a.id)
                        .copied()
                        .unwrap_or(usize::MAX);
                    let b_rank = round_robin_priority
                        .get(&b.id)
                        .copied()
                        .unwrap_or(usize::MAX);
                    a_rank.cmp(&b_rank)
                })
        });

        Ok(ordered
            .into_iter()
            .next()
            .expect("candidates checked as non-empty"))
    }
}

pub(crate) fn compute_round_robin_priority_for_endpoints(
    endpoints: &[crate::types::endpoint::Endpoint],
    start_index: usize,
) -> HashMap<Uuid, usize> {
    let len = endpoints.len();
    let mut priority = HashMap::with_capacity(len);
    if len == 0 {
        return priority;
    }

    for offset in 0..len {
        let idx = (start_index + offset) % len;
        priority.insert(endpoints[idx].id, offset);
    }

    priority
}
