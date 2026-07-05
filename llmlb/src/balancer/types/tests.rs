use super::*;
use chrono::TimeZone;
use std::collections::HashSet;

// ── ModelTpsState tests ──

#[test]
fn model_tps_state_default_is_none() {
    let s = ModelTpsState::default();
    assert!(s.tps_ema.is_none());
    assert_eq!(s.request_count, 0);
    assert_eq!(s.total_output_tokens, 0);
    assert_eq!(s.total_duration_ms, 0);
}

#[test]
fn update_tps_first_call_sets_ema_to_current() {
    let mut s = ModelTpsState::default();
    // 100 tokens / 1000ms = 100 tokens/s
    s.update_tps(100, 1000);
    assert!((s.tps_ema.unwrap() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn update_tps_second_call_applies_ema() {
    let mut s = ModelTpsState::default();
    // First: 100 t / 1s = 100 tps -> ema = 100
    s.update_tps(100, 1000);
    // Second: 200 t / 1s = 200 tps -> ema = 0.2*200 + 0.8*100 = 120
    s.update_tps(200, 1000);
    assert!((s.tps_ema.unwrap() - 120.0).abs() < f64::EPSILON);
}

#[test]
fn update_tps_zero_duration_is_noop() {
    let mut s = ModelTpsState::default();
    s.update_tps(100, 0);
    assert!(s.tps_ema.is_none());
    assert_eq!(s.request_count, 0);
    assert_eq!(s.total_output_tokens, 0);
    assert_eq!(s.total_duration_ms, 0);
}

#[test]
fn update_tps_accumulates_counters() {
    let mut s = ModelTpsState::default();
    s.update_tps(50, 500);
    s.update_tps(150, 1500);
    assert_eq!(s.request_count, 2);
    assert_eq!(s.total_output_tokens, 200);
    assert_eq!(s.total_duration_ms, 2000);
}

// ── EndpointLoadState tests ──

fn make_metrics(active: u32, ts: DateTime<Utc>) -> HealthMetrics {
    HealthMetrics {
        endpoint_id: Uuid::nil(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        gpu_usage: None,
        gpu_memory_usage: None,
        gpu_memory_total_mb: None,
        gpu_memory_used_mb: None,
        gpu_temperature: None,
        gpu_model_name: None,
        gpu_compute_capability: None,
        gpu_capability_score: None,
        active_requests: active,
        total_requests: 0,
        average_response_time_ms: None,
        timestamp: ts,
    }
}

#[test]
fn combined_active_no_metrics_uses_assigned() {
    let s = EndpointLoadState {
        assigned_active: 5,
        ..Default::default()
    };
    assert_eq!(s.combined_active(), 5);
}

#[test]
fn combined_active_takes_max_of_heartbeat_and_assigned() {
    let now = Utc::now();
    let s = EndpointLoadState {
        last_metrics: Some(make_metrics(10, now)),
        assigned_active: 3,
        ..Default::default()
    };
    assert_eq!(s.combined_active(), 10);

    let s2 = EndpointLoadState {
        last_metrics: Some(make_metrics(2, now)),
        assigned_active: 7,
        ..Default::default()
    };
    assert_eq!(s2.combined_active(), 7);
}

#[test]
fn average_latency_ms_no_completed_returns_none() {
    let s = EndpointLoadState::default();
    assert!(s.average_latency_ms().is_none());
}

#[test]
fn average_latency_ms_with_completed() {
    let s = EndpointLoadState {
        success_count: 3,
        error_count: 1,
        total_latency_ms: 800,
        ..Default::default()
    };
    // 800 / 4 = 200
    assert!((s.average_latency_ms().unwrap() - 200.0).abs() < 0.01);
}

#[test]
fn is_stale_no_metrics_returns_true() {
    let s = EndpointLoadState::default();
    assert!(s.is_stale(Utc::now()));
}

#[test]
fn is_stale_fresh_metrics_returns_false() {
    let now = Utc::now();
    let s = EndpointLoadState {
        last_metrics: Some(make_metrics(0, now)),
        ..Default::default()
    };
    assert!(!s.is_stale(now));
}

#[test]
fn is_stale_old_metrics_returns_true() {
    let now = Utc::now();
    let old = now - chrono::Duration::seconds(METRICS_STALE_THRESHOLD_SECS + 1);
    let s = EndpointLoadState {
        last_metrics: Some(make_metrics(0, old)),
        ..Default::default()
    };
    assert!(s.is_stale(now));
}

#[test]
fn push_metrics_respects_capacity() {
    let mut s = EndpointLoadState::default();
    let now = Utc::now();
    for i in 0..METRICS_HISTORY_CAPACITY + 5 {
        s.push_metrics(make_metrics(i as u32, now));
    }
    assert_eq!(s.metrics_history.len(), METRICS_HISTORY_CAPACITY);
}

#[test]
fn effective_average_ms_prefers_heartbeat() {
    let now = Utc::now();
    let mut metrics = make_metrics(0, now);
    metrics.average_response_time_ms = Some(42.0);
    let s = EndpointLoadState {
        last_metrics: Some(metrics),
        success_count: 2,
        total_latency_ms: 200,
        ..Default::default()
    };
    // Should use heartbeat value (42.0), not computed average (100.0)
    assert!((s.effective_average_ms().unwrap() - 42.0).abs() < 0.01);
}

#[test]
fn effective_average_ms_falls_back_to_computed() {
    let now = Utc::now();
    let s = EndpointLoadState {
        last_metrics: Some(make_metrics(0, now)),
        success_count: 4,
        total_latency_ms: 400,
        ..Default::default()
    };
    // heartbeat has no average_response_time_ms, falls back to 400/4=100
    assert!((s.effective_average_ms().unwrap() - 100.0).abs() < 0.01);
}

#[test]
fn last_updated_returns_timestamp() {
    let now = Utc::now();
    let s = EndpointLoadState {
        last_metrics: Some(make_metrics(0, now)),
        ..Default::default()
    };
    assert_eq!(s.last_updated(), Some(now));

    let s2 = EndpointLoadState::default();
    assert!(s2.last_updated().is_none());
}

// ── Serialization / type tests ──

#[test]
fn system_summary_default_values() {
    let s = SystemSummary::default();
    assert_eq!(s.total_nodes, 0);
    assert_eq!(s.online_nodes, 0);
    assert_eq!(s.total_requests, 0);
    assert!(s.average_response_time_ms.is_none());
    assert!(s.last_metrics_updated_at.is_none());
}

#[test]
fn endpoint_tps_summary_partial_eq() {
    let id = Uuid::new_v4();
    let a = EndpointTpsSummary {
        endpoint_id: id,
        model_count: 2,
        aggregate_tps: Some(50.0),
        total_output_tokens: 1000,
        total_requests: 10,
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn request_history_point_hash_and_eq() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let p1 = RequestHistoryPoint {
        minute: ts,
        success: 10,
        error: 2,
    };
    let p2 = p1.clone();
    assert_eq!(p1, p2);

    let mut set = HashSet::new();
    set.insert(p1.clone());
    set.insert(p2);
    assert_eq!(set.len(), 1);
}

#[test]
fn model_tps_info_serialization() {
    let info = ModelTpsInfo {
        model_id: "test-model".to_string(),
        api_kind: TpsApiKind::ChatCompletions,
        source: TpsSource::Production,
        tps: Some(42.5),
        request_count: 100,
        total_output_tokens: 5000,
        average_duration_ms: Some(120.0),
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["model_id"], "test-model");
    assert_eq!(json["request_count"], 100);
    assert_eq!(json["tps"], 42.5);
}

#[test]
fn endpoint_load_snapshot_serialization() {
    let snap = EndpointLoadSnapshot {
        endpoint_id: Uuid::nil(),
        machine_name: "test-node".to_string(),
        status: crate::types::endpoint::EndpointStatus::Online,
        cpu_usage: Some(50.0),
        memory_usage: Some(60.0),
        gpu_usage: None,
        gpu_memory_usage: None,
        gpu_memory_total_mb: None,
        gpu_memory_used_mb: None,
        gpu_temperature: None,
        gpu_model_name: None,
        gpu_compute_capability: None,
        gpu_capability_score: None,
        active_requests: 3,
        total_requests: 100,
        successful_requests: 90,
        failed_requests: 10,
        average_response_time_ms: Some(150.0),
        last_updated: None,
        is_stale: false,
        total_input_tokens: 1000,
        total_output_tokens: 2000,
        total_tokens: 3000,
    };
    let json = serde_json::to_value(&snap).unwrap();
    // endpoint_id is renamed to node_id for API compatibility
    assert!(json.get("node_id").is_some());
    assert!(json.get("endpoint_id").is_none());
    assert_eq!(json["machine_name"], "test-node");
    assert_eq!(json["active_requests"], 3);
}
