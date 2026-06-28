//! llama.cpp System Info Retrieval
//!
//! SPEC-e8e9326e: Fetch GPU/device information from llama.cpp via /slots or /metrics
//!
//! llama.cpp does not expose /api/system endpoint (unlike xLLM/Ollama).
//! Instead, we use:
//! 1. GET /slots - Slot information including concurrent capacity
//! 2. GET /metrics - Prometheus metrics with memory usage (fallback)

use crate::types::endpoint::{DeviceInfo, DeviceType, GpuDevice};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::debug;

/// /slots API response
#[derive(Debug, Deserialize)]
struct SlotsResponse {
    /// List of available slots
    slots: Vec<Slot>,
}

/// Individual slot information
#[derive(Debug, Deserialize)]
struct Slot {
    /// Context size for this slot
    n_ctx: u32,
}

/// Fetch system information from llama.cpp /slots endpoint
///
/// Returns device information based on slot count and context sizes.
/// Falls back to /metrics if /slots is not available.
pub async fn get_system_info(
    client: &Client,
    base_url: &str,
    _api_key: Option<&str>,
) -> Option<DeviceInfo> {
    // Strategy 1: Try /slots endpoint (preferred)
    if let Some(info) = get_slots_info(client, base_url).await {
        return Some(info);
    }

    // Strategy 2: Fall back to /metrics (less detailed)
    debug!("/slots not available, attempting /metrics fallback");
    get_metrics_info(client, base_url).await
}

/// Fetch system information from /slots endpoint
///
/// llama.cpp provides slot information that indicates:
/// - Number of slots = parallel inference capacity
/// - n_ctx per slot = context window size (related to memory)
async fn get_slots_info(client: &Client, base_url: &str) -> Option<DeviceInfo> {
    let url = format!("{}/slots", base_url.trim_end_matches('/'));

    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            match response.json::<SlotsResponse>().await {
                Ok(slots_resp) if !slots_resp.slots.is_empty() => {
                    debug!(
                        slot_count = slots_resp.slots.len(),
                        "Retrieved llama.cpp slot information"
                    );

                    // Create a virtual GPU device based on slots
                    // Treat each slot as a parallel processing unit
                    let slot_count = slots_resp.slots.len() as u64;
                    let avg_context =
                        slots_resp.slots.iter().map(|s| s.n_ctx as u64).sum::<u64>() / slot_count;

                    // Estimate VRAM usage: assume ~100MB per token in context window
                    // This is a rough approximation; actual usage depends on model size
                    let estimated_vram = avg_context * 100 * 1024 * 1024; // 100MB per context token

                    Some(DeviceInfo {
                        device_type: DeviceType::Gpu,
                        gpu_devices: vec![GpuDevice {
                            name: format!("llama.cpp (slots: {})", slot_count),
                            total_memory_bytes: estimated_vram,
                            used_memory_bytes: estimated_vram / 2, // Assume 50% utilization
                        }],
                    })
                }
                Ok(_) => {
                    debug!("/slots returned empty or invalid response");
                    None
                }
                Err(e) => {
                    debug!(error = %e, "/slots JSON parsing failed");
                    None
                }
            }
        }
        Ok(response) => {
            debug!(
                status = %response.status(),
                "/slots endpoint not available"
            );
            None
        }
        Err(e) => {
            debug!(error = %e, "/slots request failed");
            None
        }
    }
}

/// Fetch system information from /metrics endpoint (Prometheus format)
///
/// Falls back to /metrics when /slots is unavailable.
/// Extracts memory usage and KV cache information.
async fn get_metrics_info(client: &Client, base_url: &str) -> Option<DeviceInfo> {
    let url = format!("{}/metrics", base_url.trim_end_matches('/'));

    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            match response.text().await {
                Ok(metrics_text) => {
                    // Parse Prometheus metrics format
                    // Look for process_resident_memory_bytes and llama_kv_cache_usage_ratio
                    let mut total_memory = 0u64;
                    let mut cache_usage_ratio = 0.0f32;

                    for line in metrics_text.lines() {
                        if let Some(value_str) = line.strip_prefix("process_resident_memory_bytes ")
                        {
                            if let Ok(bytes) = value_str.parse::<u64>() {
                                total_memory = bytes;
                            }
                        } else if let Some(value_str) =
                            line.strip_prefix("llama_kv_cache_usage_ratio ")
                        {
                            if let Ok(ratio) = value_str.parse::<f32>() {
                                cache_usage_ratio = ratio;
                            }
                        }
                    }

                    if total_memory > 0 {
                        let used_memory = (total_memory as f32 * cache_usage_ratio) as u64;
                        debug!(
                            total_memory_bytes = total_memory,
                            used_memory_bytes = used_memory,
                            "Retrieved llama.cpp metrics information"
                        );

                        return Some(DeviceInfo {
                            device_type: DeviceType::Gpu,
                            gpu_devices: vec![GpuDevice {
                                name: "llama.cpp (metrics)".to_string(),
                                total_memory_bytes: total_memory,
                                used_memory_bytes: used_memory,
                            }],
                        });
                    }

                    debug!("Could not extract memory metrics from /metrics");
                    None
                }
                Err(e) => {
                    debug!(error = %e, "/metrics response text parsing failed");
                    None
                }
            }
        }
        Ok(response) => {
            debug!(
                status = %response.status(),
                "/metrics endpoint not available"
            );
            None
        }
        Err(e) => {
            debug!(error = %e, "/metrics request failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn get_slots_info_returns_device_info() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/slots"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "slots": [
                    {
                        "id": 0,
                        "active": true,
                        "n_ctx": 2048,
                        "n_past": 100,
                        "n_remaining": 1948,
                        "n_tokens": 128
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let info = get_slots_info(&client, &server.uri()).await;
        assert!(info.is_some());

        let device_info = info.unwrap();
        assert_eq!(device_info.device_type, DeviceType::Gpu);
        assert_eq!(device_info.gpu_devices.len(), 1);
        assert!(device_info.gpu_devices[0]
            .name
            .contains("llama.cpp (slots: 1)"));
    }

    #[tokio::test]
    async fn get_system_info_returns_slots_when_available() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/slots"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "slots": [
                    {
                        "id": 0,
                        "active": true,
                        "n_ctx": 2048
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let info = get_system_info(&client, &server.uri(), None).await;
        assert!(info.is_some());
        assert!(info.unwrap().gpu_devices[0].name.contains("slots"));
    }

    #[tokio::test]
    async fn get_system_info_falls_back_to_metrics() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/slots"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/metrics"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "process_resident_memory_bytes 1234567890\nllama_kv_cache_usage_ratio 0.5",
            ))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let info = get_system_info(&client, &server.uri(), None).await;
        assert!(info.is_some());
        assert!(info.unwrap().gpu_devices[0].name.contains("metrics"));
    }

    #[tokio::test]
    async fn get_system_info_returns_none_when_both_endpoints_fail() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/slots"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/metrics"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let info = get_system_info(&client, &server.uri(), None).await;
        assert!(info.is_none());
    }
}
