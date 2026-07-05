//! エンドポイントの参照・検索（read-only アクセサ）。
//!
//! キャッシュとモデルマッピングを読み、キャパビリティ/モデル/ステータス別に
//! エンドポイントを列挙・検索する。状態変更は mutations、モデル同期は model_sync。

use super::model_map::{endpoint_model_lookup_keys, model_lookup_keys};
use super::EndpointRegistry;
use crate::db::endpoints as db;
use crate::types::endpoint::{Endpoint, EndpointCapability, EndpointStatus, SupportedAPI};
use std::collections::HashSet;
use tracing::warn;
use uuid::Uuid;

impl EndpointRegistry {
    /// エンドポイントを取得
    pub async fn get(&self, id: Uuid) -> Option<Endpoint> {
        self.endpoints.read().await.get(&id).cloned()
    }

    /// すべてのエンドポイントを取得
    pub async fn list(&self) -> Vec<Endpoint> {
        self.endpoints.read().await.values().cloned().collect()
    }

    /// オンラインのエンドポイントのみを取得
    pub async fn list_online(&self) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == EndpointStatus::Online)
            .cloned()
            .collect()
    }

    /// 特定ステータスのエンドポイントを取得
    pub async fn list_by_status(&self, status: EndpointStatus) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == status)
            .cloned()
            .collect()
    }

    /// 指定した機能を持つオンラインエンドポイントを取得
    ///
    /// 例: ImageGeneration機能を持つエンドポイント → 画像生成リクエストの転送先
    pub async fn list_online_by_capability(&self, capability: EndpointCapability) -> Vec<Endpoint> {
        self.endpoints
            .read()
            .await
            .values()
            .filter(|e| e.status == EndpointStatus::Online && e.has_capability(capability))
            .cloned()
            .collect()
    }

    /// 指定した機能を持つオンラインエンドポイントを補助指標用のレイテンシ順で取得
    ///
    /// SPEC-f8e3a1b7: 推論レイテンシ（EMA α=0.2）でソート。
    /// 複数エンドポイントがある場合、レイテンシが低いものを優先する。
    pub async fn list_online_by_capability_sorted(
        &self,
        capability: EndpointCapability,
    ) -> Vec<Endpoint> {
        let mut endpoints = self.list_online_by_capability(capability).await;
        // SPEC-f8e3a1b7: 推論レイテンシ（inference_latency_ms）でソート
        endpoints.sort_by(|a, b| {
            let a_lat = a.get_inference_latency_for_sort();
            let b_lat = b.get_inference_latency_for_sort();
            a_lat
                .partial_cmp(&b_lat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        endpoints
    }

    /// 指定した機能を持つオンラインエンドポイントが存在するか確認
    pub async fn has_capability_online(&self, capability: EndpointCapability) -> bool {
        self.endpoints
            .read()
            .await
            .values()
            .any(|e| e.status == EndpointStatus::Online && e.has_capability(capability))
    }

    /// モデルIDからエンドポイントを検索
    pub async fn find_by_model(&self, model_id: &str) -> Vec<Endpoint> {
        let model_map = self.model_to_endpoints.read().await;
        let endpoints = self.endpoints.read().await;
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();

        for lookup_key in model_lookup_keys(model_id) {
            if let Some(ids) = model_map.get(&lookup_key) {
                for id in ids {
                    if !seen.insert(*id) {
                        continue;
                    }
                    if let Some(endpoint) = endpoints.get(id) {
                        if endpoint.status == EndpointStatus::Online {
                            resolved.push(endpoint.clone());
                        }
                    }
                }
            }
        }

        resolved
    }

    /// モデルIDに対応するオンラインエンドポイントが存在するか確認
    ///
    /// `!find_by_model(model_id).is_empty()` と等価だが、該当エンドポイントを
    /// clone せず最初のオンライン一致で早期 return するため存在確認に適する。
    pub async fn has_model(&self, model_id: &str) -> bool {
        let model_map = self.model_to_endpoints.read().await;
        let endpoints = self.endpoints.read().await;

        for lookup_key in model_lookup_keys(model_id) {
            if let Some(ids) = model_map.get(&lookup_key) {
                for id in ids {
                    if let Some(endpoint) = endpoints.get(id) {
                        if endpoint.status == EndpointStatus::Online {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// モデルIDと対応APIからオンラインエンドポイントを検索
    pub async fn find_by_model_and_supported_api(
        &self,
        model_id: &str,
        required_api: SupportedAPI,
    ) -> Vec<Endpoint> {
        let endpoints = self.find_by_model(model_id).await;
        let requested_keys = model_lookup_keys(model_id);
        let mut resolved = Vec::new();

        for endpoint in endpoints {
            let Ok(models) = db::list_endpoint_models(&self.pool, endpoint.id).await else {
                warn!(
                    endpoint_id = %endpoint.id,
                    model_id = %model_id,
                    required_api = %required_api,
                    "Failed to load endpoint models while filtering by supported API"
                );
                continue;
            };

            let supports_required_api = models.iter().any(|model| {
                model.supported_apis.contains(&required_api)
                    && endpoint_model_lookup_keys(model)
                        .iter()
                        .any(|key| requested_keys.iter().any(|requested| requested == key))
            });

            if supports_required_api {
                resolved.push(endpoint);
            }
        }

        resolved
    }

    /// 補助指標用にレイテンシ順でエンドポイントをソート（低レイテンシ優先）
    ///
    /// SPEC-f8e3a1b7: 推論レイテンシ（EMA α=0.2）を使用してソート。
    /// レイテンシが同じ場合はラウンドロビンでタイブレーク。
    pub async fn find_by_model_sorted_by_latency(&self, model_id: &str) -> Vec<Endpoint> {
        let mut endpoints = self.find_by_model(model_id).await;
        // SPEC-f8e3a1b7: 推論レイテンシ（inference_latency_ms）でソート
        endpoints.sort_by(|a, b| {
            let a_lat = a.get_inference_latency_for_sort();
            let b_lat = b.get_inference_latency_for_sort();
            a_lat
                .partial_cmp(&b_lat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        endpoints
    }

    /// エンドポイント数を取得
    pub async fn count(&self) -> usize {
        self.endpoints.read().await.len()
    }
}
