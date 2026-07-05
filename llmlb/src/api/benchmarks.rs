//! Benchmark APIs for comparable TPS measurement.
//!
//! 本番TPS（production）は監視向けに残し、比較可能な指標は固定シナリオの
//! ベンチ実行で分離して取得する。

use super::error::AppError;
use crate::common::{
    error::{CommonError, LbError},
    protocol::{TpsApiKind, TpsSource},
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::LazyLock};
use tokio::sync::RwLock;

mod runner;
use runner::execute_tps_benchmark;
use uuid::Uuid;

const DEFAULT_TOTAL_REQUESTS: u32 = 20;
const DEFAULT_CONCURRENCY: u16 = 4;
const DEFAULT_MAX_TOKENS: u32 = 128;
const DEFAULT_TEMPERATURE: f32 = 0.2;
const MAX_TOTAL_REQUESTS: u32 = 500;
const MAX_CONCURRENCY: u16 = 64;
const MAX_MAX_TOKENS: u32 = 4096;
const MAX_TPS_BENCH_RUNS: usize = 200;

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

#[cfg(test)]
mod tests;
