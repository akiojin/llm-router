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
