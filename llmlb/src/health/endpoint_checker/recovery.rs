//! ヘルスチェック成功後の回復系サイドエフェクト。
//!
//! offline→online 復帰時のエンドポイントタイプ再検出と、成功時のモデル
//! 自動同期を担う。

use super::*;
use crate::detection::detect_endpoint_type_with_client;
use crate::sync;

impl EndpointHealthChecker {
    /// offline→online 復帰時にエンドポイントタイプを再検出し、auto-sync に用いるタイプを返す。
    ///
    /// arch-review [M7]: 型再検出フェーズを check_endpoint から分離。
    pub(super) async fn redetect_type_on_recovery(
        &self,
        endpoint: &Endpoint,
        status_before: EndpointStatus,
        success: bool,
    ) -> EndpointType {
        let mut endpoint_type_for_auto_sync = endpoint.endpoint_type;
        let was_offline = matches!(
            status_before,
            EndpointStatus::Offline | EndpointStatus::Error
        );
        if success && was_offline {
            match detect_endpoint_type_with_client(
                &self.client,
                &endpoint.base_url,
                endpoint.api_key.as_deref(),
            )
            .await
            {
                Ok(result) => {
                    endpoint_type_for_auto_sync = result.endpoint_type;
                    if result.endpoint_type != endpoint.endpoint_type {
                        info!(
                            endpoint_id = %endpoint.id,
                            endpoint_name = %endpoint.name,
                            detected_type = %result.endpoint_type.as_str(),
                            "Endpoint type re-detected on health check"
                        );
                        if let Err(e) = self
                            .registry
                            .update_endpoint_type(endpoint.id, result.endpoint_type)
                            .await
                        {
                            warn!(
                                endpoint_id = %endpoint.id,
                                error = %e,
                                "Failed to update endpoint type"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        endpoint_id = %endpoint.id,
                        error = %e,
                        "Endpoint type re-detection failed; keeping previous type"
                    );
                }
            }
        }
        endpoint_type_for_auto_sync
    }

    pub(super) async fn maybe_auto_sync_models(
        &self,
        endpoint: &Endpoint,
        new_status: EndpointStatus,
        endpoint_type: EndpointType,
    ) {
        if new_status != EndpointStatus::Online {
            return;
        }

        // Auto model sync runs on successful health checks, but is throttled per endpoint.
        let now = Instant::now();
        {
            let mut last = self.last_auto_sync_models.write().await;
            if let Some(prev) = last.get(&endpoint.id) {
                if prev.elapsed() < self.auto_sync_models_interval {
                    return;
                }
            }
            last.insert(endpoint.id, now);
        }

        let endpoint_id = endpoint.id;
        let endpoint_name = endpoint.name.clone();
        let base_url = endpoint.base_url.clone();
        let api_key = endpoint.api_key.clone();
        let timeout_secs = endpoint.inference_timeout_secs as u64;

        let pool = self.registry.pool().clone();
        let registry = self.registry.clone();
        let client = self.client.clone();
        let last_auto_sync_models = self.last_auto_sync_models.clone();

        tokio::spawn(async move {
            match sync::sync_models_with_type(
                &pool,
                &client,
                endpoint_id,
                &base_url,
                api_key.as_deref(),
                timeout_secs,
                Some(endpoint_type),
            )
            .await
            {
                Ok(result) => {
                    match registry.refresh_model_mappings(endpoint_id).await {
                        Ok(()) => {
                            // Update timestamp on successful completion.
                            last_auto_sync_models
                                .write()
                                .await
                                .insert(endpoint_id, Instant::now());
                            info!(
                                endpoint_id = %endpoint_id,
                                endpoint_name = %endpoint_name,
                                added = result.added,
                                removed = result.removed,
                                updated = result.updated,
                                "Auto model sync completed on health check"
                            );
                        }
                        Err(e) => {
                            // Don't keep throttling when model mappings weren't refreshed successfully.
                            last_auto_sync_models.write().await.remove(&endpoint_id);
                            warn!(
                                endpoint_id = %endpoint_id,
                                endpoint_name = %endpoint_name,
                                error = %e,
                                "Failed to refresh model mappings after auto model sync"
                            );
                        }
                    }
                }
                Err(e) => {
                    // Don't keep throttling when sync failed - allow retry on the next successful health check.
                    last_auto_sync_models.write().await.remove(&endpoint_id);
                    warn!(
                        endpoint_id = %endpoint_id,
                        endpoint_name = %endpoint_name,
                        error = %e,
                        "Auto model sync failed on health check"
                    );
                }
            }
        });
    }
}
