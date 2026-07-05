//! エンドポイントの health プローブと失敗判定。
//!
//! xLLM は /api/health→/v1/models フォールバック、それ以外は /v1/models で
//! 直接プローブし、GPU 情報の取得と連続失敗時のステータス遷移を判定する。

use super::EndpointHealthChecker;
use super::CONSECUTIVE_FAILURES_FOR_OFFLINE;
use crate::common::http::RequestBuilderBearerExt;
use crate::types::endpoint::{Endpoint, EndpointStatus, EndpointType};
use std::time::Instant;
use tracing::debug;

/// `/api/health`から取得したGPU情報
/// `/api/health`から取得したGPU情報
#[derive(Debug, Clone, Default)]
pub(crate) struct GpuInfo {
    /// GPUデバイス数
    pub gpu_device_count: Option<u32>,
    /// GPU総メモリ（バイト）
    pub gpu_total_memory_bytes: Option<u64>,
    /// GPU使用中メモリ（バイト）
    pub gpu_used_memory_bytes: Option<u64>,
    /// GPU能力スコア
    pub gpu_capability_score: Option<f32>,
    /// 現在のアクティブリクエスト数
    pub active_requests: Option<u32>,
}

impl EndpointHealthChecker {
    /// エンドポイントへ health プローブを実行し、結果タプルを返す。
    ///
    /// arch-review [M7]: check_endpoint の 205 行オーケストレーションから、
    /// レジストリ更新などの副作用を伴わないプローブ（xLLM は /api/health →
    /// /v1/models フォールバック、それ以外は /v1/models 直接）を切り出した。
    pub(super) async fn probe_endpoint(
        &self,
        endpoint: &Endpoint,
        status_before: EndpointStatus,
    ) -> (bool, Option<String>, EndpointStatus, Option<GpuInfo>, u32) {
        let is_xllm = matches!(endpoint.endpoint_type, EndpointType::Xllm);
        if is_xllm {
            let start = Instant::now();
            match self.try_v0_health(endpoint).await {
                Ok(gpu_info) => (
                    true,
                    None,
                    EndpointStatus::Online,
                    Some(gpu_info),
                    start.elapsed().as_millis() as u32,
                ),
                Err(_v0_error) => {
                    debug!(
                        endpoint_id = %endpoint.id,
                        endpoint_name = %endpoint.name,
                        "/api/health failed, falling back to /v1/models"
                    );

                    let start = Instant::now();
                    match self.try_v1_models(endpoint).await {
                        Ok(()) => (
                            true,
                            None,
                            EndpointStatus::Online,
                            None,
                            start.elapsed().as_millis() as u32,
                        ),
                        Err(e) => {
                            let error = e.to_string();
                            let new_status = self.determine_failure_status(endpoint, status_before);
                            (
                                false,
                                Some(error),
                                new_status,
                                None,
                                start.elapsed().as_millis() as u32,
                            )
                        }
                    }
                }
            }
        } else {
            debug!(
                endpoint_id = %endpoint.id,
                endpoint_name = %endpoint.name,
                "non-xLLM endpoint, using /v1/models directly"
            );
            let start = Instant::now();
            match self.try_v1_models(endpoint).await {
                Ok(()) => (
                    true,
                    None,
                    EndpointStatus::Online,
                    None,
                    start.elapsed().as_millis() as u32,
                ),
                Err(e) => {
                    let error = e.to_string();
                    let new_status = self.determine_failure_status(endpoint, status_before);
                    (
                        false,
                        Some(error),
                        new_status,
                        None,
                        start.elapsed().as_millis() as u32,
                    )
                }
            }
        }
    }

    /// `/api/health`を呼び出してGPU情報を取得
    async fn try_v0_health(
        &self,
        endpoint: &Endpoint,
    ) -> Result<GpuInfo, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/health", endpoint.base_url.trim_end_matches('/'));

        let mut request = self.client.get(&url);
        request = request.bearer_opt(endpoint.api_key.as_ref());

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()).into());
        }

        let body: serde_json::Value = response.json().await?;

        // GPU情報を抽出
        let gpu = body.get("gpu");
        let load = body.get("load");

        Ok(GpuInfo {
            gpu_device_count: gpu
                .and_then(|g| g.get("device_count"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            gpu_total_memory_bytes: gpu
                .and_then(|g| g.get("total_memory_bytes"))
                .and_then(|v| v.as_u64()),
            gpu_used_memory_bytes: gpu
                .and_then(|g| g.get("used_memory_bytes"))
                .and_then(|v| v.as_u64()),
            gpu_capability_score: gpu
                .and_then(|g| g.get("capability_score"))
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
            active_requests: load
                .and_then(|l| l.get("active_requests"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
        })
    }

    /// `/v1/models`を呼び出してヘルスチェック（フォールバック用）
    async fn try_v1_models(
        &self,
        endpoint: &Endpoint,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/models", endpoint.base_url.trim_end_matches('/'));

        let mut request = self.client.get(&url);
        request = request.bearer_opt(endpoint.api_key.as_ref());

        let response = request.send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {}", response.status()).into())
        }
    }

    /// 失敗時の新ステータスを決定
    pub(super) fn determine_failure_status(
        &self,
        endpoint: &Endpoint,
        status_before: EndpointStatus,
    ) -> EndpointStatus {
        match status_before {
            // pending状態は初回失敗で即offline
            EndpointStatus::Pending => EndpointStatus::Offline,
            // online状態は連続失敗でerror→offline
            EndpointStatus::Online => {
                if endpoint.error_count + 1 >= CONSECUTIVE_FAILURES_FOR_OFFLINE {
                    EndpointStatus::Offline
                } else {
                    EndpointStatus::Error
                }
            }
            // error状態は連続失敗でoffline
            EndpointStatus::Error => {
                if endpoint.error_count + 1 >= CONSECUTIVE_FAILURES_FOR_OFFLINE {
                    EndpointStatus::Offline
                } else {
                    EndpointStatus::Error
                }
            }
            // offline状態はそのまま
            EndpointStatus::Offline => EndpointStatus::Offline,
        }
    }
}
