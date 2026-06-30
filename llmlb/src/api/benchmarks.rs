//! Benchmark APIs for comparable TPS measurement.
//!
//! 本番TPS（production）は監視向けに残し、比較可能な指標は固定シナリオの
//! ベンチ実行で分離して取得する。

use super::error::AppError;
use crate::common::{
    error::{CommonError, LbError},
    protocol::{TpsApiKind, TpsSource},
};
use crate::{token::extract_usage_from_response, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::LazyLock, time::Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

const DEFAULT_TOTAL_REQUESTS: u32 = 20;
const DEFAULT_CONCURRENCY: u16 = 4;
const DEFAULT_MAX_TOKENS: u32 = 128;
const DEFAULT_TEMPERATURE: f32 = 0.2;
const MAX_TOTAL_REQUESTS: u32 = 500;
const MAX_CONCURRENCY: u16 = 64;
const MAX_MAX_TOKENS: u32 = 4096;
const MAX_TPS_BENCH_RUNS: usize = 200;
const BENCHMARK_PROMPT: &str =
    "Benchmark prompt: explain the Fibonacci sequence in one short paragraph.";

static TPS_BENCH_RUNS: LazyLock<RwLock<HashMap<Uuid, TpsBenchmarkRun>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// POST /api/benchmarks/tps request body.
#[derive(Debug, Clone, Deserialize)]
pub struct StartTpsBenchmarkRequest {
    /// Target model ID.
    pub model: String,
    /// API kind to benchmark.
    #[serde(default)]
    pub api_kind: Option<TpsApiKind>,
    /// Total number of requests to execute.
    #[serde(default)]
    pub total_requests: Option<u32>,
    /// Concurrent workers.
    #[serde(default)]
    pub concurrency: Option<u16>,
    /// max_tokens / max_output_tokens
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// Normalized benchmark configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpsBenchmarkRequest {
    /// Target model ID.
    pub model: String,
    /// API kind to benchmark.
    pub api_kind: TpsApiKind,
    /// Total number of requests to execute.
    pub total_requests: u32,
    /// Concurrent workers.
    pub concurrency: u16,
    /// max_tokens / max_output_tokens
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f32,
}

impl TryFrom<StartTpsBenchmarkRequest> for TpsBenchmarkRequest {
    type Error = AppError;

    fn try_from(value: StartTpsBenchmarkRequest) -> Result<Self, Self::Error> {
        let model = value.model.trim().to_string();
        if model.is_empty() {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                "model is required".to_string(),
            ))));
        }

        let total_requests = value.total_requests.unwrap_or(DEFAULT_TOTAL_REQUESTS);
        if total_requests == 0 || total_requests > MAX_TOTAL_REQUESTS {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!(
                    "total_requests must be between 1 and {}",
                    MAX_TOTAL_REQUESTS
                ),
            ))));
        }

        let concurrency = value.concurrency.unwrap_or(DEFAULT_CONCURRENCY);
        if concurrency == 0 || concurrency > MAX_CONCURRENCY {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("concurrency must be between 1 and {}", MAX_CONCURRENCY),
            ))));
        }

        let max_tokens = value.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        if max_tokens == 0 || max_tokens > MAX_MAX_TOKENS {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("max_tokens must be between 1 and {}", MAX_MAX_TOKENS),
            ))));
        }

        let temperature = value.temperature.unwrap_or(DEFAULT_TEMPERATURE);
        if !(0.0..=2.0).contains(&temperature) {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                "temperature must be between 0.0 and 2.0".to_string(),
            ))));
        }

        Ok(Self {
            model,
            api_kind: value.api_kind.unwrap_or(TpsApiKind::ChatCompletions),
            total_requests,
            concurrency,
            max_tokens,
            temperature,
        })
    }
}

/// Accepted response for benchmark start.
#[derive(Debug, Clone, Serialize)]
pub struct TpsBenchmarkAccepted {
    /// Benchmark run ID.
    pub run_id: Uuid,
    /// Initial run status.
    pub status: TpsBenchmarkStatus,
}

/// Benchmark run status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TpsBenchmarkStatus {
    /// Run is still executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed with error.
    Failed,
}

/// Benchmark per-endpoint summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpsBenchmarkEndpointSummary {
    /// Endpoint ID.
    pub endpoint_id: Uuid,
    /// Endpoint name.
    pub endpoint_name: String,
    /// Requests sent to this endpoint.
    pub requests: u64,
    /// Successful upstream responses.
    pub successful_requests: u64,
    /// Requests with measurable TPS.
    pub measured_requests: u64,
    /// successful_requests / requests.
    pub success_rate: f64,
    /// Mean TPS for measured requests.
    pub mean_tps: Option<f64>,
    /// p50 TPS for measured requests.
    pub p50_tps: Option<f64>,
    /// p95 TPS for measured requests.
    pub p95_tps: Option<f64>,
}

/// Comparable benchmark result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpsBenchmarkResult {
    /// API kind used during this run.
    pub api_kind: TpsApiKind,
    /// Result source (`benchmark`).
    pub source: TpsSource,
    /// Requested total request count.
    pub total_requests: u64,
    /// Successful upstream responses.
    pub successful_requests: u64,
    /// Requests with measurable TPS.
    pub measured_requests: u64,
    /// successful_requests / total_requests.
    pub success_rate: f64,
    /// Mean TPS.
    pub mean_tps: Option<f64>,
    /// p50 TPS.
    pub p50_tps: Option<f64>,
    /// p95 TPS.
    pub p95_tps: Option<f64>,
    /// Per-endpoint breakdown.
    pub per_endpoint: Vec<TpsBenchmarkEndpointSummary>,
}

/// Benchmark run record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TpsBenchmarkRun {
    /// Run ID.
    pub run_id: Uuid,
    /// Current status.
    pub status: TpsBenchmarkStatus,
    /// Requested time.
    pub requested_at: DateTime<Utc>,
    /// Completed time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Normalized request settings.
    pub request: TpsBenchmarkRequest,
    /// Result when completed.
    pub result: Option<TpsBenchmarkResult>,
    /// Error text when failed.
    pub error: Option<String>,
}

impl TpsBenchmarkRun {
    fn new(run_id: Uuid, request: TpsBenchmarkRequest) -> Self {
        Self {
            run_id,
            status: TpsBenchmarkStatus::Running,
            requested_at: Utc::now(),
            completed_at: None,
            request,
            result: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
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

/// POST /api/benchmarks/tps
pub async fn start_tps_benchmark(
    axum::Extension(claims): axum::Extension<crate::common::auth::Claims>,
    State(state): State<AppState>,
    Json(payload): Json<StartTpsBenchmarkRequest>,
) -> Result<(StatusCode, Json<TpsBenchmarkAccepted>), AppError> {
    // ベンチマーク起動は admin のみ（Viewer の権限昇格を防ぐ）
    if claims.role != crate::common::auth::UserRole::Admin {
        return Err(AppError::from(LbError::Authorization(
            "Only admin can start benchmarks".to_string(),
        )));
    }
    let request = TpsBenchmarkRequest::try_from(payload)?;
    let run_id = Uuid::new_v4();

    {
        let mut runs = TPS_BENCH_RUNS.write().await;
        runs.insert(run_id, TpsBenchmarkRun::new(run_id, request.clone()));
        prune_tps_benchmark_runs(&mut runs);
    }

    tokio::spawn(async move {
        finalize_tps_benchmark_run(state, run_id, request).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(TpsBenchmarkAccepted {
            run_id,
            status: TpsBenchmarkStatus::Running,
        }),
    ))
}

/// GET /api/benchmarks/tps/{run_id}
pub async fn get_tps_benchmark(
    Path(run_id): Path<Uuid>,
) -> Result<Json<TpsBenchmarkRun>, AppError> {
    let runs = TPS_BENCH_RUNS.read().await;
    let run = runs
        .get(&run_id)
        .cloned()
        .ok_or_else(|| AppError::from(LbError::NotFound(format!("benchmark run {}", run_id))))?;
    Ok(Json(run))
}

async fn finalize_tps_benchmark_run(state: AppState, run_id: Uuid, request: TpsBenchmarkRequest) {
    let result = execute_tps_benchmark(&state, &request).await;
    let completed_at = Utc::now();
    let mut runs = TPS_BENCH_RUNS.write().await;
    let Some(run) = runs.get_mut(&run_id) else {
        return;
    };
    run.completed_at = Some(completed_at);
    match result {
        Ok(result) => {
            run.status = TpsBenchmarkStatus::Completed;
            run.result = Some(result);
            run.error = None;
        }
        Err(err) => {
            run.status = TpsBenchmarkStatus::Failed;
            run.result = None;
            run.error = Some(err.external_message().to_string());
        }
    }
    prune_tps_benchmark_runs(&mut runs);
}

fn prune_tps_benchmark_runs(runs: &mut HashMap<Uuid, TpsBenchmarkRun>) {
    prune_tps_benchmark_runs_with_limit(runs, MAX_TPS_BENCH_RUNS);
}

fn prune_tps_benchmark_runs_with_limit(runs: &mut HashMap<Uuid, TpsBenchmarkRun>, max_runs: usize) {
    if runs.len() <= max_runs {
        return;
    }

    let mut overflow = runs.len() - max_runs;

    // Prefer pruning old finished runs before touching active ones.
    let mut completed_candidates: Vec<(Uuid, DateTime<Utc>)> = runs
        .iter()
        .filter_map(|(run_id, run)| {
            if run.status == TpsBenchmarkStatus::Running {
                None
            } else {
                let sort_key = run
                    .completed_at
                    .as_ref()
                    .unwrap_or(&run.requested_at)
                    .to_owned();
                Some((*run_id, sort_key))
            }
        })
        .collect();
    completed_candidates.sort_by_key(|(_, sort_key)| sort_key.to_owned());

    for (run_id, _) in completed_candidates {
        if overflow == 0 {
            break;
        }
        if runs.remove(&run_id).is_some() {
            overflow -= 1;
        }
    }

    if overflow == 0 {
        return;
    }

    // Fallback: when only active runs exist, prune the oldest running runs.
    let mut running_candidates: Vec<(Uuid, DateTime<Utc>)> = runs
        .iter()
        .filter_map(|(run_id, run)| {
            if run.status == TpsBenchmarkStatus::Running {
                Some((*run_id, run.requested_at))
            } else {
                None
            }
        })
        .collect();
    running_candidates.sort_by_key(|(_, requested_at)| *requested_at);

    for (run_id, _) in running_candidates.into_iter().take(overflow) {
        runs.remove(&run_id);
    }
}

async fn execute_tps_benchmark(
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
    use chrono::Duration;

    fn sample_request() -> TpsBenchmarkRequest {
        TpsBenchmarkRequest {
            model: "test-model".to_string(),
            api_kind: TpsApiKind::ChatCompletions,
            total_requests: 10,
            concurrency: 2,
            max_tokens: 64,
            temperature: 0.2,
        }
    }

    fn build_run(
        requested_at: DateTime<Utc>,
        status: TpsBenchmarkStatus,
        completed_at: Option<DateTime<Utc>>,
    ) -> (Uuid, TpsBenchmarkRun) {
        let run_id = Uuid::new_v4();
        let mut run = TpsBenchmarkRun::new(run_id, sample_request());
        run.requested_at = requested_at;
        run.status = status;
        run.completed_at = completed_at;
        (run_id, run)
    }

    #[test]
    fn prune_prefers_completed_runs_before_running_runs() {
        let now = Utc::now();
        let mut runs = HashMap::new();

        let (completed_id, completed_run) = build_run(
            now - Duration::minutes(3),
            TpsBenchmarkStatus::Completed,
            Some(now - Duration::minutes(2)),
        );
        let (running_old_id, running_old_run) = build_run(
            now - Duration::minutes(2),
            TpsBenchmarkStatus::Running,
            None,
        );
        let (running_new_id, running_new_run) = build_run(
            now - Duration::minutes(1),
            TpsBenchmarkStatus::Running,
            None,
        );

        runs.insert(completed_id, completed_run);
        runs.insert(running_old_id, running_old_run);
        runs.insert(running_new_id, running_new_run);

        prune_tps_benchmark_runs_with_limit(&mut runs, 2);

        assert_eq!(runs.len(), 2);
        assert!(!runs.contains_key(&completed_id));
        assert!(runs.contains_key(&running_old_id));
        assert!(runs.contains_key(&running_new_id));
    }

    #[test]
    fn prune_removes_oldest_running_when_all_runs_are_running() {
        let now = Utc::now();
        let mut runs = HashMap::new();

        let (oldest_id, oldest_run) = build_run(
            now - Duration::minutes(3),
            TpsBenchmarkStatus::Running,
            None,
        );
        let (middle_id, middle_run) = build_run(
            now - Duration::minutes(2),
            TpsBenchmarkStatus::Running,
            None,
        );
        let (newest_id, newest_run) = build_run(
            now - Duration::minutes(1),
            TpsBenchmarkStatus::Running,
            None,
        );

        runs.insert(oldest_id, oldest_run);
        runs.insert(middle_id, middle_run);
        runs.insert(newest_id, newest_run);

        prune_tps_benchmark_runs_with_limit(&mut runs, 2);

        assert_eq!(runs.len(), 2);
        assert!(!runs.contains_key(&oldest_id));
        assert!(runs.contains_key(&middle_id));
        assert!(runs.contains_key(&newest_id));
    }

    // --- TpsBenchmarkRequest validation tests (TryFrom) ---

    #[test]
    fn try_from_defaults_applied_correctly() {
        let start = StartTpsBenchmarkRequest {
            model: "llama3".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.model, "llama3");
        assert_eq!(req.api_kind, TpsApiKind::ChatCompletions);
        assert_eq!(req.total_requests, DEFAULT_TOTAL_REQUESTS);
        assert_eq!(req.concurrency, DEFAULT_CONCURRENCY);
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(req.temperature, DEFAULT_TEMPERATURE);
    }

    #[test]
    fn try_from_custom_values_preserved() {
        let start = StartTpsBenchmarkRequest {
            model: "gpt-4".to_string(),
            api_kind: Some(TpsApiKind::Responses),
            total_requests: Some(50),
            concurrency: Some(8),
            max_tokens: Some(256),
            temperature: Some(0.8),
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.api_kind, TpsApiKind::Responses);
        assert_eq!(req.total_requests, 50);
        assert_eq!(req.concurrency, 8);
        assert_eq!(req.max_tokens, 256);
        assert!((req.temperature - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn try_from_empty_model_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_whitespace_model_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "   ".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_model_name_trimmed() {
        let start = StartTpsBenchmarkRequest {
            model: "  llama3  ".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.model, "llama3");
    }

    #[test]
    fn try_from_total_requests_zero_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: Some(0),
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_total_requests_over_max_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: Some(MAX_TOTAL_REQUESTS + 1),
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_total_requests_at_max_succeeds() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: Some(MAX_TOTAL_REQUESTS),
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.total_requests, MAX_TOTAL_REQUESTS);
    }

    #[test]
    fn try_from_concurrency_zero_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: Some(0),
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_concurrency_over_max_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: Some(MAX_CONCURRENCY + 1),
            max_tokens: None,
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_concurrency_at_max_succeeds() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: Some(MAX_CONCURRENCY),
            max_tokens: None,
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.concurrency, MAX_CONCURRENCY);
    }

    #[test]
    fn try_from_max_tokens_zero_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: Some(0),
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_max_tokens_over_max_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: Some(MAX_MAX_TOKENS + 1),
            temperature: None,
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_max_tokens_at_max_succeeds() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: Some(MAX_MAX_TOKENS),
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.max_tokens, MAX_MAX_TOKENS);
    }

    #[test]
    fn try_from_temperature_negative_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: Some(-0.1),
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_temperature_above_2_fails() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: Some(2.1),
        };
        assert!(TpsBenchmarkRequest::try_from(start).is_err());
    }

    #[test]
    fn try_from_temperature_zero_succeeds() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: Some(0.0),
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert!((req.temperature - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn try_from_temperature_two_succeeds() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: None,
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: Some(2.0),
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert!((req.temperature - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn try_from_api_kind_completions() {
        let start = StartTpsBenchmarkRequest {
            model: "test".to_string(),
            api_kind: Some(TpsApiKind::Completions),
            total_requests: None,
            concurrency: None,
            max_tokens: None,
            temperature: None,
        };
        let req = TpsBenchmarkRequest::try_from(start).expect("should succeed");
        assert_eq!(req.api_kind, TpsApiKind::Completions);
    }

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

    // --- TpsBenchmarkRun::new tests ---

    #[test]
    fn tps_benchmark_run_new_fields() {
        let run_id = Uuid::new_v4();
        let req = sample_request();
        let run = TpsBenchmarkRun::new(run_id, req.clone());
        assert_eq!(run.run_id, run_id);
        assert_eq!(run.status, TpsBenchmarkStatus::Running);
        assert!(run.completed_at.is_none());
        assert!(run.result.is_none());
        assert!(run.error.is_none());
        assert_eq!(run.request.model, "test-model");
    }

    // --- TpsBenchmarkStatus serde tests ---

    #[test]
    fn tps_benchmark_status_serde_roundtrip() {
        for status in [
            TpsBenchmarkStatus::Running,
            TpsBenchmarkStatus::Completed,
            TpsBenchmarkStatus::Failed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: TpsBenchmarkStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, status);
        }
    }

    #[test]
    fn tps_benchmark_status_serialization_values() {
        assert_eq!(
            serde_json::to_string(&TpsBenchmarkStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&TpsBenchmarkStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&TpsBenchmarkStatus::Failed).unwrap(),
            "\"failed\""
        );
    }

    // --- prune edge cases ---

    #[test]
    fn prune_no_op_when_under_limit() {
        let mut runs = HashMap::new();
        let (id1, run1) = build_run(
            Utc::now() - Duration::minutes(1),
            TpsBenchmarkStatus::Completed,
            Some(Utc::now()),
        );
        runs.insert(id1, run1);

        prune_tps_benchmark_runs_with_limit(&mut runs, 5);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn prune_no_op_when_at_limit() {
        let mut runs = HashMap::new();
        let (id1, run1) = build_run(
            Utc::now() - Duration::minutes(1),
            TpsBenchmarkStatus::Completed,
            Some(Utc::now()),
        );
        let (id2, run2) = build_run(Utc::now(), TpsBenchmarkStatus::Running, None);
        runs.insert(id1, run1);
        runs.insert(id2, run2);

        prune_tps_benchmark_runs_with_limit(&mut runs, 2);
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn prune_removes_failed_before_running() {
        let now = Utc::now();
        let mut runs = HashMap::new();

        let (failed_id, failed_run) = build_run(
            now - Duration::minutes(5),
            TpsBenchmarkStatus::Failed,
            Some(now - Duration::minutes(4)),
        );
        let (running_id, running_run) = build_run(
            now - Duration::minutes(3),
            TpsBenchmarkStatus::Running,
            None,
        );
        let (new_id, new_run) = build_run(now, TpsBenchmarkStatus::Running, None);

        runs.insert(failed_id, failed_run);
        runs.insert(running_id, running_run);
        runs.insert(new_id, new_run);

        prune_tps_benchmark_runs_with_limit(&mut runs, 2);
        assert_eq!(runs.len(), 2);
        assert!(!runs.contains_key(&failed_id));
        assert!(runs.contains_key(&running_id));
        assert!(runs.contains_key(&new_id));
    }

    // --- StartTpsBenchmarkRequest deserialization tests ---

    #[test]
    fn start_request_deserialization_minimal() {
        let json = r#"{"model": "llama3"}"#;
        let req: StartTpsBenchmarkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "llama3");
        assert!(req.api_kind.is_none());
        assert!(req.total_requests.is_none());
        assert!(req.concurrency.is_none());
        assert!(req.max_tokens.is_none());
        assert!(req.temperature.is_none());
    }

    #[test]
    fn start_request_deserialization_full() {
        let json = r#"{
            "model": "gpt-4o",
            "api_kind": "responses",
            "total_requests": 100,
            "concurrency": 16,
            "max_tokens": 512,
            "temperature": 1.0
        }"#;
        let req: StartTpsBenchmarkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.api_kind, Some(TpsApiKind::Responses));
        assert_eq!(req.total_requests, Some(100));
        assert_eq!(req.concurrency, Some(16));
        assert_eq!(req.max_tokens, Some(512));
        assert!((req.temperature.unwrap() - 1.0).abs() < f32::EPSILON);
    }

    // --- TpsBenchmarkResult / TpsBenchmarkEndpointSummary serde tests ---

    #[test]
    fn tps_benchmark_result_serialization() {
        let result = TpsBenchmarkResult {
            api_kind: TpsApiKind::ChatCompletions,
            source: TpsSource::Benchmark,
            total_requests: 20,
            successful_requests: 18,
            measured_requests: 15,
            success_rate: 0.9,
            mean_tps: Some(75.5),
            p50_tps: Some(70.0),
            p95_tps: Some(90.0),
            per_endpoint: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"api_kind\":\"chat_completions\""));
        assert!(json.contains("\"source\":\"benchmark\""));
        assert!(json.contains("\"total_requests\":20"));
        assert!(json.contains("\"mean_tps\":75.5"));

        let deserialized: TpsBenchmarkResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_requests, 20);
        assert_eq!(deserialized.successful_requests, 18);
    }

    #[test]
    fn tps_benchmark_endpoint_summary_serialization() {
        let summary = TpsBenchmarkEndpointSummary {
            endpoint_id: Uuid::nil(),
            endpoint_name: "test-ep".to_string(),
            requests: 10,
            successful_requests: 9,
            measured_requests: 8,
            success_rate: 0.9,
            mean_tps: Some(50.0),
            p50_tps: Some(48.0),
            p95_tps: Some(55.0),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"endpoint_name\":\"test-ep\""));
        assert!(json.contains("\"requests\":10"));

        let deserialized: TpsBenchmarkEndpointSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.requests, 10);
        assert_eq!(deserialized.mean_tps, Some(50.0));
    }

    // --- TpsBenchmarkAccepted serde test ---

    #[test]
    fn tps_benchmark_accepted_serialization() {
        let accepted = TpsBenchmarkAccepted {
            run_id: Uuid::nil(),
            status: TpsBenchmarkStatus::Running,
        };
        let json = serde_json::to_string(&accepted).unwrap();
        assert!(json.contains("\"status\":\"running\""));

        // Verify run_id is serialized
        assert!(json.contains("\"run_id\""));
    }
}
