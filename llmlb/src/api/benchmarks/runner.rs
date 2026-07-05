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
mod tests {
    use super::*;

    // --- mean() tests ---

    #[test]
    fn mean_empty_returns_none() {
        assert!(mean(&[]).is_none());
    }

    #[test]
    fn mean_single_value() {
        let result = mean(&[42.0]).expect("should return some");
        assert!((result - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_multiple_values() {
        let result = mean(&[10.0, 20.0, 30.0]).expect("should return some");
        assert!((result - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_identical_values() {
        let result = mean(&[5.0, 5.0, 5.0, 5.0]).expect("should return some");
        assert!((result - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_with_decimals() {
        let result = mean(&[1.5, 2.5]).expect("should return some");
        assert!((result - 2.0).abs() < f64::EPSILON);
    }

    // --- percentile() tests ---

    #[test]
    fn percentile_empty_returns_none() {
        assert!(percentile(&[], 0.5).is_none());
    }

    #[test]
    fn percentile_single_value_returns_that_value() {
        let result = percentile(&[100.0], 0.5).expect("should return some");
        assert!((result - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p50_of_odd_count() {
        // Sorted: [10, 20, 30] -> p50 index = round(2 * 0.5) = 1 -> 20
        let result = percentile(&[30.0, 10.0, 20.0], 0.50).expect("should return some");
        assert!((result - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p50_of_even_count() {
        // Sorted: [10, 20, 30, 40] -> p50 index = round(3 * 0.5) = round(1.5) = 2 -> 30
        let result = percentile(&[40.0, 10.0, 30.0, 20.0], 0.50).expect("should return some");
        assert!((result - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p95_of_many_values() {
        let values: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        let result = percentile(&values, 0.95).expect("should return some");
        // index = round(99 * 0.95) = round(94.05) = 94 -> values[94] = 95
        assert!((result - 95.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p0_returns_min() {
        let result = percentile(&[5.0, 1.0, 10.0], 0.0).expect("should return some");
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p100_returns_max() {
        let result = percentile(&[5.0, 1.0, 10.0], 1.0).expect("should return some");
        assert!((result - 10.0).abs() < f64::EPSILON);
    }

    // --- build_benchmark_payload tests ---

    #[test]
    fn build_benchmark_payload_chat_completions() {
        let (path, payload) =
            build_benchmark_payload("llama3", TpsApiKind::ChatCompletions, 128, 0.2);
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(payload["model"], "llama3");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["max_tokens"], 128);
        assert!((payload["temperature"].as_f64().unwrap() - 0.2).abs() < 0.01);
        assert!(payload["messages"].is_array());
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"], BENCHMARK_PROMPT);
    }

    #[test]
    fn build_benchmark_payload_completions() {
        let (path, payload) = build_benchmark_payload("gpt-3.5", TpsApiKind::Completions, 64, 0.5);
        assert_eq!(path, "/v1/completions");
        assert_eq!(payload["model"], "gpt-3.5");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["max_tokens"], 64);
        assert_eq!(payload["prompt"], BENCHMARK_PROMPT);
    }

    #[test]
    fn build_benchmark_payload_responses() {
        let (path, payload) = build_benchmark_payload("gpt-4o", TpsApiKind::Responses, 256, 1.0);
        assert_eq!(path, "/v1/responses");
        assert_eq!(payload["model"], "gpt-4o");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["max_output_tokens"], 256);
        assert_eq!(payload["input"], BENCHMARK_PROMPT);
    }

    // --- build_benchmark_result tests ---

    #[test]
    fn build_benchmark_result_empty_samples() {
        let result = build_benchmark_result(TpsApiKind::ChatCompletions, 10, vec![]);
        assert_eq!(result.total_requests, 10);
        assert_eq!(result.successful_requests, 0);
        assert_eq!(result.measured_requests, 0);
        assert!((result.success_rate - 0.0).abs() < f64::EPSILON);
        assert!(result.mean_tps.is_none());
        assert!(result.p50_tps.is_none());
        assert!(result.p95_tps.is_none());
        assert!(result.per_endpoint.is_empty());
        assert_eq!(result.source, TpsSource::Benchmark);
    }

    #[test]
    fn build_benchmark_result_all_success_with_tps() {
        let ep_id = Uuid::new_v4();
        let samples = vec![
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep1".to_string(),
                success: true,
                tps: Some(50.0),
            },
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep1".to_string(),
                success: true,
                tps: Some(100.0),
            },
        ];

        let result = build_benchmark_result(TpsApiKind::ChatCompletions, 2, samples);
        assert_eq!(result.total_requests, 2);
        assert_eq!(result.successful_requests, 2);
        assert_eq!(result.measured_requests, 2);
        assert!((result.success_rate - 1.0).abs() < f64::EPSILON);
        assert!((result.mean_tps.unwrap() - 75.0).abs() < f64::EPSILON);
        assert_eq!(result.per_endpoint.len(), 1);
        assert_eq!(result.per_endpoint[0].endpoint_name, "ep1");
        assert_eq!(result.per_endpoint[0].requests, 2);
        assert_eq!(result.per_endpoint[0].successful_requests, 2);
    }

    #[test]
    fn build_benchmark_result_mixed_success_failure() {
        let ep_id = Uuid::new_v4();
        let samples = vec![
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep1".to_string(),
                success: true,
                tps: Some(80.0),
            },
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep1".to_string(),
                success: false,
                tps: None,
            },
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep1".to_string(),
                success: true,
                tps: None, // success but no TPS (0 output tokens)
            },
        ];

        let result = build_benchmark_result(TpsApiKind::Completions, 3, samples);
        assert_eq!(result.total_requests, 3);
        assert_eq!(result.successful_requests, 2);
        assert_eq!(result.measured_requests, 1);
        assert!((result.success_rate - 2.0 / 3.0).abs() < 0.01);
        assert!((result.mean_tps.unwrap() - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn build_benchmark_result_multiple_endpoints_sorted() {
        let ep1_id = Uuid::new_v4();
        let ep2_id = Uuid::new_v4();
        let samples = vec![
            BenchmarkSample {
                endpoint_id: ep2_id,
                endpoint_name: "z-endpoint".to_string(),
                success: true,
                tps: Some(60.0),
            },
            BenchmarkSample {
                endpoint_id: ep1_id,
                endpoint_name: "a-endpoint".to_string(),
                success: true,
                tps: Some(40.0),
            },
        ];

        let result = build_benchmark_result(TpsApiKind::ChatCompletions, 2, samples);
        assert_eq!(result.per_endpoint.len(), 2);
        // Sorted by endpoint_name
        assert_eq!(result.per_endpoint[0].endpoint_name, "a-endpoint");
        assert_eq!(result.per_endpoint[1].endpoint_name, "z-endpoint");
    }

    #[test]
    fn build_benchmark_result_all_failures() {
        let ep_id = Uuid::new_v4();
        let samples = vec![
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep".to_string(),
                success: false,
                tps: None,
            },
            BenchmarkSample {
                endpoint_id: ep_id,
                endpoint_name: "ep".to_string(),
                success: false,
                tps: None,
            },
        ];

        let result = build_benchmark_result(TpsApiKind::Responses, 2, samples);
        assert_eq!(result.successful_requests, 0);
        assert_eq!(result.measured_requests, 0);
        assert!((result.success_rate - 0.0).abs() < f64::EPSILON);
        assert!(result.mean_tps.is_none());
        assert_eq!(result.per_endpoint[0].success_rate, 0.0);
    }
}
