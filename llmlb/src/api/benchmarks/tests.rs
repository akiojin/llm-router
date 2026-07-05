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
