//! LoadManager のエンドポイント選択ロジック（TPS 優先 + ラウンドロビン）
//!
//! arch-review [H1]: LoadManager は TPS 追跡・負荷・履歴・受付制御・選択を集約した
//! God object。オンライン集約・直接選択・モデル別選択・候補からの選択などの
//! エンドポイント選択群を submodule の `impl LoadManager` として切り出した。
//! 子モジュールから private メソッド/フィールドを参照でき、公開 API は不変。

use super::LoadManager;
use crate::common::error::{LbError, RouterResult};
use crate::common::protocol::TpsApiKind;
use std::sync::atomic::Ordering as AtomicOrdering;

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

    /// エンドポイントを直接選択（ラウンドロビン）
    pub async fn select_endpoint_direct(&self) -> RouterResult<crate::types::endpoint::Endpoint> {
        let endpoints = self.collect_online_endpoints(None).await?;
        self.select_endpoint_round_robin_from_endpoints(endpoints)
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

    /// 指定モデルに対応するエンドポイントを直接選択（ラウンドロビン）
    pub async fn select_endpoint_direct_for_model(
        &self,
        model_id: &str,
    ) -> RouterResult<crate::types::endpoint::Endpoint> {
        let endpoints = self.collect_online_endpoints(Some(model_id)).await?;
        self.select_endpoint_round_robin_from_endpoints(endpoints)
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
}
