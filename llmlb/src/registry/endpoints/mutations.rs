//! エンドポイントの write-through 変更（追加・更新・削除）。
//!
//! DB とメモリキャッシュの双方を更新する。参照は queries、モデル同期は model_sync。

use super::EndpointRegistry;
use crate::db::endpoints as db;
use crate::types::endpoint::{Endpoint, EndpointStatus, EndpointType};
use uuid::Uuid;

impl EndpointRegistry {
    /// エンドポイントを追加（DBとキャッシュ両方に保存）
    pub async fn add(&self, endpoint: Endpoint) -> Result<(), sqlx::Error> {
        // DBに保存
        db::create_endpoint(&self.pool, &endpoint).await?;

        // キャッシュに追加
        self.endpoints.write().await.insert(endpoint.id, endpoint);

        Ok(())
    }

    /// エンドポイントをキャッシュのみに追加（DBは更新しない）
    ///
    /// 外部でDB保存が完了した後にキャッシュを同期するために使用する。
    pub async fn add_to_cache(&self, endpoint: Endpoint) {
        self.endpoints.write().await.insert(endpoint.id, endpoint);
    }

    /// エンドポイントを更新（DBとキャッシュ両方）
    pub async fn update(&self, endpoint: Endpoint) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint(&self.pool, &endpoint).await?;

        if updated {
            // キャッシュを更新
            self.endpoints.write().await.insert(endpoint.id, endpoint);
        }

        Ok(updated)
    }

    /// エンドポイントのステータスを更新
    pub async fn update_status(
        &self,
        id: Uuid,
        status: EndpointStatus,
        latency_ms: Option<u32>,
        error: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint_status(&self.pool, id, status, latency_ms, error).await?;

        if updated {
            // キャッシュを更新
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.status = status;
                if let Some(v) = latency_ms {
                    endpoint.latency_ms = Some(v);
                }
                // DBと同様に、last_error は成功時にクリアし、error_count は status=error のときのみ加算する。
                endpoint.last_error = error.map(String::from);
                endpoint.error_count = if status == EndpointStatus::Error {
                    endpoint.error_count.saturating_add(1)
                } else {
                    0
                };
                endpoint.last_seen = Some(chrono::Utc::now());
            }
        }

        Ok(updated)
    }

    /// エンドポイントのGPU情報を更新（キャッシュのみ、DBには保存しない）
    ///
    /// `/api/health`から取得したGPU情報をキャッシュに反映する。
    /// GPU情報は頻繁に変化するため、DBには保存せずメモリ上でのみ管理する。
    pub async fn update_gpu_info(
        &self,
        id: Uuid,
        gpu_device_count: Option<u32>,
        gpu_total_memory_bytes: Option<u64>,
        gpu_used_memory_bytes: Option<u64>,
        gpu_capability_score: Option<f32>,
        active_requests: Option<u32>,
    ) -> bool {
        let mut endpoints = self.endpoints.write().await;
        if let Some(endpoint) = endpoints.get_mut(&id) {
            endpoint.gpu_device_count = gpu_device_count;
            endpoint.gpu_total_memory_bytes = gpu_total_memory_bytes;
            endpoint.gpu_used_memory_bytes = gpu_used_memory_bytes;
            endpoint.gpu_capability_score = gpu_capability_score;
            endpoint.active_requests = active_requests;
            true
        } else {
            false
        }
    }

    /// エンドポイントのタイプを更新（DBとキャッシュ両方）（SPEC-e8e9326e）
    ///
    /// ヘルスチェック時のoffline→online遷移時に再検出して更新する。
    pub async fn update_endpoint_type(
        &self,
        id: Uuid,
        endpoint_type: EndpointType,
    ) -> Result<bool, sqlx::Error> {
        // DBを更新
        let updated = db::update_endpoint_type(&self.pool, id, endpoint_type).await?;

        if updated {
            // キャッシュを更新
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.endpoint_type = endpoint_type;
            }
        }

        Ok(updated)
    }

    /// エンドポイントの推論レイテンシを更新（DBとキャッシュ両方）（SPEC-f8e3a1b7）
    ///
    /// 推論リクエスト完了時に呼び出し、EMA（α=0.2）で平均レイテンシを計算する。
    /// オフライン時は`reset_inference_latency`を呼び出す。
    pub async fn update_inference_latency(
        &self,
        id: Uuid,
        new_latency_ms: f64,
    ) -> Result<bool, sqlx::Error> {
        // キャッシュを更新（EMA計算はEndpoint内で行う）
        let inference_latency_ms = {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.update_inference_latency(new_latency_ms);
                endpoint.inference_latency_ms
            } else {
                return Ok(false);
            }
        };

        // DBを更新
        db::update_inference_latency(&self.pool, id, inference_latency_ms).await
    }

    /// エンドポイントの推論レイテンシをリセット（オフライン時）（SPEC-f8e3a1b7）
    ///
    /// エンドポイントがオフラインになったときに呼び出し、レイテンシをINFINITYに設定。
    pub async fn reset_inference_latency(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        // キャッシュを更新
        {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.reset_inference_latency();
            }
        }

        // DBを更新（INFINITYを保存）
        db::update_inference_latency(&self.pool, id, Some(f64::INFINITY)).await
    }

    /// エンドポイントのデバイス情報を更新（DBとキャッシュ両方）（SPEC-f8e3a1b7）
    ///
    /// /api/system APIから取得したデバイス情報を保存する。
    pub async fn update_device_info(
        &self,
        id: Uuid,
        device_info: Option<crate::types::endpoint::DeviceInfo>,
    ) -> Result<bool, sqlx::Error> {
        // キャッシュを更新
        {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.device_info = device_info.clone();
            } else {
                return Ok(false);
            }
        }

        // DBを更新
        db::update_device_info(&self.pool, id, device_info.as_ref()).await
    }

    /// エンドポイントのリクエストカウンタをインクリメント（DBとキャッシュ両方）
    pub async fn increment_request_counters(
        &self,
        id: Uuid,
        success: bool,
    ) -> Result<bool, sqlx::Error> {
        let updated = db::increment_request_counters(&self.pool, id, success).await?;

        if updated {
            let mut endpoints = self.endpoints.write().await;
            if let Some(endpoint) = endpoints.get_mut(&id) {
                endpoint.total_requests += 1;
                if success {
                    endpoint.successful_requests += 1;
                } else {
                    endpoint.failed_requests += 1;
                }
            }
        }

        Ok(updated)
    }

    /// エンドポイントを削除（DBとキャッシュ両方）
    pub async fn remove(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        // モデルマッピングから削除
        {
            let mut model_map = self.model_to_endpoints.write().await;
            for endpoints in model_map.values_mut() {
                endpoints.retain(|eid| *eid != id);
            }
            // 空になったエントリを削除
            model_map.retain(|_, v| !v.is_empty());
        }

        // DBから削除
        let deleted = db::delete_endpoint(&self.pool, id).await?;

        if deleted {
            // キャッシュから削除
            self.endpoints.write().await.remove(&id);
        }

        Ok(deleted)
    }
}
