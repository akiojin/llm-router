//! ベンチマーク実行と結果集計（状態非依存の計算）
//!
//! arch-review [H6]: api/benchmarks.rs から、固定シナリオのリクエスト発行・
//! リクエスト毎 TPS 計測・サンプルの集計/パーセンタイル計算を分離。

use super::{TpsBenchmarkEndpointSummary, TpsBenchmarkRequest, TpsBenchmarkResult};
use crate::common::error::LbError;
use crate::common::protocol::{TpsApiKind, TpsSource};
use crate::token::extract_usage_from_response;
use crate::AppState;
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

const BENCHMARK_PROMPT: &str =
    "Benchmark prompt: explain the Fibonacci sequence in one short paragraph.";

struct BenchmarkSample {
    endpoint_id: Uuid,
    endpoint_name: String,
    success: bool,
    tps: Option<f64>,
}

#[derive(Debug, Default)]
struct EndpointSampleAccumulator {
    endpoint_name: String,
    requests: u64,
    successful_requests: u64,
    tps_values: Vec<f64>,
}

pub(crate) async fn execute_tps_benchmark(
    state: &AppState,
    request: &TpsBenchmarkRequest,
) -> Result<TpsBenchmarkResult, LbError> {
    let total_requests = request.total_requests;
    let concurrency = request.concurrency as usize;
    let model = request.model.clone();
    let api_kind = request.api_kind;
    let max_tokens = request.max_tokens;
    let temperature = request.temperature;

    let mut final_samples: Vec<BenchmarkSample> = Vec::with_capacity(total_requests as usize);
    let samples = stream::iter(0..total_requests)
        .map(|_| {
            let state = state.clone();
            let model = model.clone();
            async move {
                run_single_benchmark_request(&state, &model, api_kind, max_tokens, temperature)
                    .await
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<Result<BenchmarkSample, LbError>>>()
        .await;

    for sample in samples {
        final_samples.push(sample?);
    }

    Ok(build_benchmark_result(
        api_kind,
        total_requests as u64,
        final_samples,
    ))
}

async fn run_single_benchmark_request(
    state: &AppState,
    model: &str,
    api_kind: TpsApiKind,
    max_tokens: u32,
    temperature: f32,
) -> Result<BenchmarkSample, LbError> {
    let endpoint = state
        .load_manager
        .select_endpoint_round_robin_ready_for_model(model)
        .await?;

    let (path, payload) = build_benchmark_payload(model, api_kind, max_tokens, temperature);
    let url = format!("{}{}", endpoint.base_url.trim_end_matches('/'), path);
    let mut req =
        state
            .http_client
            .post(url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(
                endpoint.inference_timeout_secs as u64,
            ));
    if let Some(api_key) = &endpoint.api_key {
        req = req.bearer_auth(api_key);
    }

    let started = Instant::now();
    let response = match req.send().await {
        Ok(response) => response,
        Err(_) => {
            return Ok(BenchmarkSample {
                endpoint_id: endpoint.id,
                endpoint_name: endpoint.name,
                success: false,
                tps: None,
            })
        }
    };
    let duration = started.elapsed();
    if !response.status().is_success() {
        return Ok(BenchmarkSample {
            endpoint_id: endpoint.id,
            endpoint_name: endpoint.name,
            success: false,
            tps: None,
        });
    }

    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => {
            return Ok(BenchmarkSample {
                endpoint_id: endpoint.id,
                endpoint_name: endpoint.name,
                success: false,
                tps: None,
            })
        }
    };

    let output_tokens = extract_usage_from_response(&body)
        .and_then(|u| u.output_tokens)
        .unwrap_or(0) as u64;
    let tps = if output_tokens > 0 {
        Some(output_tokens as f64 / (duration.as_secs_f64().max(0.001)))
    } else {
        None
    };

    Ok(BenchmarkSample {
        endpoint_id: endpoint.id,
        endpoint_name: endpoint.name,
        success: true,
        tps,
    })
}

fn build_benchmark_payload(
    model: &str,
    api_kind: TpsApiKind,
    max_tokens: u32,
    temperature: f32,
) -> (&'static str, Value) {
    match api_kind {
        TpsApiKind::ChatCompletions => (
            "/v1/chat/completions",
            json!({
                "model": model,
                "messages": [{"role": "user", "content": BENCHMARK_PROMPT}],
                "stream": false,
                "max_tokens": max_tokens,
                "temperature": temperature,
            }),
        ),
        TpsApiKind::Completions => (
            "/v1/completions",
            json!({
                "model": model,
                "prompt": BENCHMARK_PROMPT,
                "stream": false,
                "max_tokens": max_tokens,
                "temperature": temperature,
            }),
        ),
        TpsApiKind::Responses => (
            "/v1/responses",
            json!({
                "model": model,
                "input": BENCHMARK_PROMPT,
                "stream": false,
                "max_output_tokens": max_tokens,
                "temperature": temperature,
            }),
        ),
    }
}

fn build_benchmark_result(
    api_kind: TpsApiKind,
    total_requests: u64,
    samples: Vec<BenchmarkSample>,
) -> TpsBenchmarkResult {
    let successful_requests = samples.iter().filter(|s| s.success).count() as u64;
    let tps_values: Vec<f64> = samples.iter().filter_map(|s| s.tps).collect();
    let measured_requests = tps_values.len() as u64;

    let mut endpoint_map: HashMap<Uuid, EndpointSampleAccumulator> = HashMap::new();
    for sample in samples {
        let entry = endpoint_map.entry(sample.endpoint_id).or_default();
        entry.endpoint_name = sample.endpoint_name;
        entry.requests += 1;
        if sample.success {
            entry.successful_requests += 1;
        }
        if let Some(tps) = sample.tps {
            entry.tps_values.push(tps);
        }
    }

    let mut per_endpoint: Vec<TpsBenchmarkEndpointSummary> = endpoint_map
        .into_iter()
        .map(|(endpoint_id, acc)| {
            let requests = acc.requests;
            let success_rate = if requests > 0 {
                acc.successful_requests as f64 / requests as f64
            } else {
                0.0
            };
            TpsBenchmarkEndpointSummary {
                endpoint_id,
                endpoint_name: acc.endpoint_name,
                requests,
                successful_requests: acc.successful_requests,
                measured_requests: acc.tps_values.len() as u64,
                success_rate,
                mean_tps: mean(&acc.tps_values),
                p50_tps: percentile(&acc.tps_values, 0.50),
                p95_tps: percentile(&acc.tps_values, 0.95),
            }
        })
        .collect();
    per_endpoint.sort_by(|a, b| a.endpoint_name.cmp(&b.endpoint_name));

    let success_rate = if total_requests > 0 {
        successful_requests as f64 / total_requests as f64
    } else {
        0.0
    };

    TpsBenchmarkResult {
        api_kind,
        source: TpsSource::Benchmark,
        total_requests,
        successful_requests,
        measured_requests,
        success_rate,
        mean_tps: mean(&tps_values),
        p50_tps: percentile(&tps_values, 0.50),
        p95_tps: percentile(&tps_values, 0.95),
        per_endpoint,
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() - 1) as f64 * percentile)
        .round()
        .clamp(0.0, (sorted.len() - 1) as f64) as usize;
    sorted.get(index).copied()
}

#[cfg(test)]
mod tests;
