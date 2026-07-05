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
    let (path, payload) = build_benchmark_payload("llama3", TpsApiKind::ChatCompletions, 128, 0.2);
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
