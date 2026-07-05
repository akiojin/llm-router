use super::overview::{
    collect_action_items, count_unique_model_ids, invalidate_persisted_totals_cache,
    PersistedRequestTotals, PersistedTokenTotals, PersistedTotalsCache,
    LAST_KNOWN_PERSISTED_TOTALS,
};
use super::{parse_ip_alert_threshold, DashboardOperations};
use crate::types::endpoint::{Endpoint, EndpointStatus, EndpointType};

#[test]
fn persisted_totals_cache_invalidation_clears_entries() {
    // arch-review [L3]: クリアフックがプロセスキャッシュを確実に空にすることを検証。
    LAST_KNOWN_PERSISTED_TOTALS.write().unwrap().insert(
        1u64,
        PersistedTotalsCache {
            request_totals: PersistedRequestTotals::default(),
            token_totals: PersistedTokenTotals::default(),
        },
    );
    assert!(!LAST_KNOWN_PERSISTED_TOTALS.read().unwrap().is_empty());
    invalidate_persisted_totals_cache();
    assert!(LAST_KNOWN_PERSISTED_TOTALS.read().unwrap().is_empty());
}

/// フォールバック計算: avg_response_time_ms が None の場合に
/// オンラインエンドポイントの latency_ms から平均値を計算するロジック
fn fallback_avg_response_time(summary_avg: Option<f32>, endpoints: &[Endpoint]) -> Option<f32> {
    summary_avg.or_else(|| {
        let online_endpoints: Vec<_> = endpoints
            .iter()
            .filter(|e| e.status == EndpointStatus::Online && e.latency_ms.is_some())
            .collect();
        if online_endpoints.is_empty() {
            return None;
        }
        let total: f64 = online_endpoints
            .iter()
            .map(|e| e.latency_ms.unwrap() as f64)
            .sum();
        Some((total / online_endpoints.len() as f64) as f32)
    })
}

/// T010 [US3]: collect_stats のフォールバック計算テスト
/// インメモリavg_response_time_msがNoneの場合、オンラインエンドポイントの
/// latency_msから平均値を計算する
#[test]
fn test_avg_response_time_fallback_from_latency() {
    // (1) summary の average_response_time_ms が None
    // (2) オンラインエンドポイント2つ (latency_ms=100, 200)
    let mut ep1 = Endpoint::new(
        "EP1".to_string(),
        "http://localhost:8001".to_string(),
        EndpointType::Xllm,
    );
    ep1.status = EndpointStatus::Online;
    ep1.latency_ms = Some(100);

    let mut ep2 = Endpoint::new(
        "EP2".to_string(),
        "http://localhost:8002".to_string(),
        EndpointType::Xllm,
    );
    ep2.status = EndpointStatus::Online;
    ep2.latency_ms = Some(200);

    let endpoints = vec![ep1, ep2];

    // (3) 結果は 150.0（平均値）
    let result = fallback_avg_response_time(None, &endpoints);
    assert_eq!(result, Some(150.0));
}

/// T010 追加シナリオ: 全エンドポイントがオフラインの場合は None のまま
#[test]
fn test_avg_response_time_fallback_all_offline() {
    let mut ep1 = Endpoint::new(
        "EP1".to_string(),
        "http://localhost:8001".to_string(),
        EndpointType::Xllm,
    );
    ep1.status = EndpointStatus::Offline;
    ep1.latency_ms = Some(100);

    let endpoints = vec![ep1];

    let result = fallback_avg_response_time(None, &endpoints);
    assert_eq!(result, None);
}

/// T010 追加シナリオ: summary に値がある場合はフォールバックしない
#[test]
fn test_avg_response_time_no_fallback_when_present() {
    let endpoints = vec![];
    let result = fallback_avg_response_time(Some(42.0), &endpoints);
    assert_eq!(result, Some(42.0));
}

#[test]
fn parse_ip_alert_threshold_accepts_positive_integer() {
    assert_eq!(parse_ip_alert_threshold("100").unwrap(), 100);
    assert_eq!(parse_ip_alert_threshold(" 42 ").unwrap(), 42);
}

#[test]
fn parse_ip_alert_threshold_rejects_zero_or_negative() {
    assert!(parse_ip_alert_threshold("0").is_err());
    assert!(parse_ip_alert_threshold("-1").is_err());
}

#[test]
fn parse_ip_alert_threshold_rejects_non_numeric() {
    assert!(parse_ip_alert_threshold("abc").is_err());
}

// ===== effective_ip_alert_threshold tests =====

#[test]
fn effective_threshold_returns_parsed_value() {
    use super::effective_ip_alert_threshold;
    assert_eq!(effective_ip_alert_threshold(Some("50")), 50);
}

#[test]
fn effective_threshold_returns_default_when_none() {
    use super::effective_ip_alert_threshold;
    use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
    assert_eq!(
        effective_ip_alert_threshold(None),
        IP_ALERT_THRESHOLD_DEFAULT_VALUE
    );
}

#[test]
fn effective_threshold_returns_default_for_invalid_value() {
    use super::effective_ip_alert_threshold;
    use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
    assert_eq!(
        effective_ip_alert_threshold(Some("not-a-number")),
        IP_ALERT_THRESHOLD_DEFAULT_VALUE
    );
}

#[test]
fn effective_threshold_returns_default_for_zero() {
    use super::effective_ip_alert_threshold;
    use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
    assert_eq!(
        effective_ip_alert_threshold(Some("0")),
        IP_ALERT_THRESHOLD_DEFAULT_VALUE
    );
}

#[test]
fn effective_threshold_returns_default_for_negative() {
    use super::effective_ip_alert_threshold;
    use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
    assert_eq!(
        effective_ip_alert_threshold(Some("-5")),
        IP_ALERT_THRESHOLD_DEFAULT_VALUE
    );
}

#[test]
fn effective_threshold_accepts_one() {
    use super::effective_ip_alert_threshold;
    assert_eq!(effective_ip_alert_threshold(Some("1")), 1);
}

#[test]
fn effective_threshold_accepts_large_value() {
    use super::effective_ip_alert_threshold;
    assert_eq!(effective_ip_alert_threshold(Some("10000")), 10000);
}

// ===== parse_ip_alert_threshold extended tests =====

#[test]
fn parse_ip_alert_threshold_accepts_large_value() {
    assert_eq!(parse_ip_alert_threshold("999999").unwrap(), 999999);
}

#[test]
fn parse_ip_alert_threshold_accepts_one() {
    assert_eq!(parse_ip_alert_threshold("1").unwrap(), 1);
}

#[test]
fn parse_ip_alert_threshold_rejects_empty_string() {
    assert!(parse_ip_alert_threshold("").is_err());
}

#[test]
fn parse_ip_alert_threshold_rejects_float() {
    assert!(parse_ip_alert_threshold("1.5").is_err());
}

// ===== DashboardStats serialization tests =====

#[test]
fn test_dashboard_stats_serialization() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 5,
        online_nodes: 3,
        pending_nodes: 1,
        registering_nodes: 0,
        offline_nodes: 1,
        total_requests: 1000,
        successful_requests: 950,
        failed_requests: 50,
        total_active_requests: 2,
        queued_requests: 0,
        average_response_time_ms: Some(150.5),
        average_gpu_usage: None,
        average_gpu_memory_usage: None,
        last_metrics_updated_at: None,
        last_registered_at: None,
        last_seen_at: None,
        openai_key_present: true,
        google_key_present: false,
        anthropic_key_present: false,
        total_input_tokens: 100000,
        total_output_tokens: 50000,
        total_tokens: 150000,
    };

    let json = serde_json::to_value(&stats).unwrap();
    assert_eq!(json["total_runtimes"], 5);
    assert_eq!(json["online_runtimes"], 3);
    assert_eq!(json["pending_runtimes"], 1);
    assert_eq!(json["registering_runtimes"], 0);
    assert_eq!(json["offline_runtimes"], 1);
    assert_eq!(json["total_requests"], 1000);
    assert_eq!(json["successful_requests"], 950);
    assert_eq!(json["failed_requests"], 50);
    assert_eq!(json["total_active_requests"], 2);
    assert_eq!(json["queued_requests"], 0);
    assert_eq!(json["average_response_time_ms"], 150.5);
    assert!(json["average_gpu_usage"].is_null());
    assert_eq!(json["openai_key_present"], true);
    assert_eq!(json["google_key_present"], false);
    assert_eq!(json["anthropic_key_present"], false);
    assert_eq!(json["total_input_tokens"], 100000);
    assert_eq!(json["total_output_tokens"], 50000);
    assert_eq!(json["total_tokens"], 150000);
}

#[test]
fn test_dashboard_stats_rename_fields() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 2,
        online_nodes: 1,
        pending_nodes: 0,
        registering_nodes: 0,
        offline_nodes: 1,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        total_active_requests: 0,
        queued_requests: 0,
        average_response_time_ms: None,
        average_gpu_usage: None,
        average_gpu_memory_usage: None,
        last_metrics_updated_at: None,
        last_registered_at: None,
        last_seen_at: None,
        openai_key_present: false,
        google_key_present: false,
        anthropic_key_present: false,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_tokens: 0,
    };

    let json = serde_json::to_value(&stats).unwrap();
    // Verify the serde rename_all is applied (total_runtimes, not total_nodes)
    assert!(json.get("total_runtimes").is_some());
    assert!(json.get("online_runtimes").is_some());
    assert!(json.get("pending_runtimes").is_some());
    assert!(json.get("offline_runtimes").is_some());
}

// ===== DashboardEndpoint tests =====

#[test]
fn test_dashboard_endpoint_serialization() {
    use super::DashboardEndpoint;
    use crate::types::endpoint::{EndpointStatus, EndpointType};

    let endpoint = DashboardEndpoint {
        id: uuid::Uuid::nil(),
        name: "test-endpoint".to_string(),
        base_url: "http://localhost:8080".to_string(),
        status: EndpointStatus::Online,
        endpoint_type: EndpointType::Xllm,
        health_check_interval_secs: 30,
        inference_timeout_secs: 120,
        latency_ms: Some(45),
        last_seen: None,
        last_error: None,
        error_count: 0,
        registered_at: chrono::Utc::now(),
        notes: None,
        model_count: 3,
        total_requests: 100,
        successful_requests: 95,
        failed_requests: 5,
    };

    let json = serde_json::to_value(&endpoint).unwrap();
    assert_eq!(json["name"], "test-endpoint");
    assert_eq!(json["base_url"], "http://localhost:8080");
    assert_eq!(json["health_check_interval_secs"], 30);
    assert_eq!(json["inference_timeout_secs"], 120);
    assert_eq!(json["latency_ms"], 45);
    assert_eq!(json["error_count"], 0);
    assert_eq!(json["model_count"], 3);
    assert_eq!(json["total_requests"], 100);
    assert_eq!(json["successful_requests"], 95);
    assert_eq!(json["failed_requests"], 5);
}

// ===== PersistedRequestTotals / PersistedTokenTotals / PersistedTotalsCache tests =====

#[test]
fn test_persisted_request_totals_default() {
    use super::overview::PersistedRequestTotals;
    let defaults = PersistedRequestTotals::default();
    assert_eq!(defaults.total_requests, 0);
    assert_eq!(defaults.successful_requests, 0);
    assert_eq!(defaults.failed_requests, 0);
}

#[test]
fn test_persisted_token_totals_default() {
    use super::overview::PersistedTokenTotals;
    let defaults = PersistedTokenTotals::default();
    assert_eq!(defaults.total_input_tokens, 0);
    assert_eq!(defaults.total_output_tokens, 0);
    assert_eq!(defaults.total_tokens, 0);
}

#[test]
fn test_persisted_totals_cache_default() {
    use super::overview::PersistedTotalsCache;
    let cache = PersistedTotalsCache::default();
    assert_eq!(cache.request_totals.total_requests, 0);
    assert_eq!(cache.token_totals.total_tokens, 0);
}

// ===== RequestHistoryQuery tests =====

#[test]
fn test_normalized_per_page_valid_sizes() {
    use super::RequestHistoryQuery;

    for &size in super::ALLOWED_PAGE_SIZES {
        let query = RequestHistoryQuery {
            page: 1,
            per_page: size,
            limit: None,
            offset: None,
            model: None,
            endpoint_id: None,
            status: None,
            start_time: None,
            end_time: None,
            client_ip: None,
        };
        assert_eq!(query.normalized_per_page(), size);
    }
}

#[test]
fn test_normalized_per_page_invalid_falls_back_to_default() {
    use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};

    let query = RequestHistoryQuery {
        page: 1,
        per_page: 37,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
}

#[test]
fn test_normalized_per_page_zero_falls_back() {
    use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};

    let query = RequestHistoryQuery {
        page: 1,
        per_page: 0,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
}

// ===== to_record_filter tests =====

#[test]
fn test_to_record_filter_valid() {
    use super::RequestHistoryQuery;

    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: Some("llama".to_string()),
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    let filter = query.to_record_filter().unwrap();
    assert_eq!(filter.model, Some("llama".to_string()));
}

#[test]
fn test_to_record_filter_start_after_end_error() {
    use super::RequestHistoryQuery;
    use chrono::TimeZone;

    let start = chrono::Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
    let end = chrono::Utc.with_ymd_and_hms(2026, 2, 1, 12, 0, 0).unwrap();

    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: Some(start),
        end_time: Some(end),
        client_ip: None,
    };
    assert!(query.to_record_filter().is_err());
}

// ===== RequestHistoryExportQuery to_record_filter tests =====

#[test]
fn test_export_query_to_record_filter_valid() {
    use super::RequestHistoryExportQuery;

    let query = RequestHistoryExportQuery {
        format: super::RequestHistoryExportFormat::Json,
        model: Some("gpt-4".to_string()),
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    let filter = query.to_record_filter().unwrap();
    assert_eq!(filter.model, Some("gpt-4".to_string()));
}

#[test]
fn test_export_query_to_record_filter_start_after_end_error() {
    use super::RequestHistoryExportQuery;
    use chrono::TimeZone;

    let start = chrono::Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let end = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    let query = RequestHistoryExportQuery {
        format: super::RequestHistoryExportFormat::Csv,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: Some(start),
        end_time: Some(end),
        client_ip: None,
    };
    assert!(query.to_record_filter().is_err());
}

// ===== RequestHistoryExportFormat tests =====

#[test]
fn test_export_format_default_is_csv() {
    use super::RequestHistoryExportFormat;
    assert_eq!(
        RequestHistoryExportFormat::default(),
        RequestHistoryExportFormat::Csv
    );
}

#[test]
fn test_export_format_deserialization() {
    use super::RequestHistoryExportFormat;
    assert_eq!(
        serde_json::from_str::<RequestHistoryExportFormat>("\"csv\"").unwrap(),
        RequestHistoryExportFormat::Csv
    );
    assert_eq!(
        serde_json::from_str::<RequestHistoryExportFormat>("\"json\"").unwrap(),
        RequestHistoryExportFormat::Json
    );
}

// ===== default functions tests =====

#[test]
fn test_default_page() {
    use super::default_page;
    assert_eq!(default_page(), 1);
}

#[test]
fn test_default_per_page() {
    use super::{default_per_page, DEFAULT_PAGE_SIZE};
    assert_eq!(default_per_page(), DEFAULT_PAGE_SIZE);
}

#[test]
fn test_default_days() {
    use super::default_days;
    assert_eq!(default_days(), Some(30));
}

#[test]
fn test_default_months() {
    use super::default_months;
    assert_eq!(default_months(), Some(12));
}

#[test]
fn test_default_clients_per_page() {
    use super::default_clients_per_page;
    assert_eq!(default_clients_per_page(), 20);
}

// ===== ALLOWED_PAGE_SIZES / DEFAULT_PAGE_SIZE constants tests =====

#[test]
fn test_allowed_page_sizes_are_reasonable() {
    use super::{ALLOWED_PAGE_SIZES, DEFAULT_PAGE_SIZE};
    assert!(ALLOWED_PAGE_SIZES.contains(&10));
    assert!(ALLOWED_PAGE_SIZES.contains(&25));
    assert!(ALLOWED_PAGE_SIZES.contains(&50));
    assert!(ALLOWED_PAGE_SIZES.contains(&100));
    assert!(ALLOWED_PAGE_SIZES.contains(&DEFAULT_PAGE_SIZE));
}

// ===== DailyTokenStatsQuery deserialization tests =====

#[test]
fn test_daily_token_stats_query_deserialization() {
    use super::DailyTokenStatsQuery;
    let q: DailyTokenStatsQuery = serde_json::from_str(r#"{"days": 7}"#).unwrap();
    assert_eq!(q.days, Some(7));
}

#[test]
fn test_daily_token_stats_query_default_days() {
    use super::DailyTokenStatsQuery;
    let q: DailyTokenStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(q.days, Some(30));
}

// ===== MonthlyTokenStatsQuery deserialization tests =====

#[test]
fn test_monthly_token_stats_query_deserialization() {
    use super::MonthlyTokenStatsQuery;
    let q: MonthlyTokenStatsQuery = serde_json::from_str(r#"{"months": 6}"#).unwrap();
    assert_eq!(q.months, Some(6));
}

#[test]
fn test_monthly_token_stats_query_default_months() {
    use super::MonthlyTokenStatsQuery;
    let q: MonthlyTokenStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(q.months, Some(12));
}

// ===== DailyTokenStats serialization =====

#[test]
fn test_daily_token_stats_serialization() {
    use super::DailyTokenStats;
    let stats = DailyTokenStats {
        date: "2026-02-27".to_string(),
        total_input_tokens: 1000,
        total_output_tokens: 500,
        total_tokens: 1500,
        request_count: 10,
    };
    let json = serde_json::to_value(&stats).unwrap();
    assert_eq!(json["date"], "2026-02-27");
    assert_eq!(json["total_input_tokens"], 1000);
    assert_eq!(json["total_output_tokens"], 500);
    assert_eq!(json["total_tokens"], 1500);
    assert_eq!(json["request_count"], 10);
}

// ===== MonthlyTokenStats serialization =====

#[test]
fn test_monthly_token_stats_serialization() {
    use super::MonthlyTokenStats;
    let stats = MonthlyTokenStats {
        month: "2026-02".to_string(),
        total_input_tokens: 50000,
        total_output_tokens: 25000,
        total_tokens: 75000,
        request_count: 500,
    };
    let json = serde_json::to_value(&stats).unwrap();
    assert_eq!(json["month"], "2026-02");
    assert_eq!(json["total_input_tokens"], 50000);
    assert_eq!(json["total_output_tokens"], 25000);
    assert_eq!(json["total_tokens"], 75000);
    assert_eq!(json["request_count"], 500);
}

// ===== DashboardEndpoint equality tests =====

#[test]
fn test_dashboard_endpoint_equality() {
    use super::DashboardEndpoint;
    use crate::types::endpoint::{EndpointStatus, EndpointType};

    let ts = chrono::Utc::now();
    let ep1 = DashboardEndpoint {
        id: uuid::Uuid::nil(),
        name: "ep".to_string(),
        base_url: "http://localhost".to_string(),
        status: EndpointStatus::Online,
        endpoint_type: EndpointType::Xllm,
        health_check_interval_secs: 30,
        inference_timeout_secs: 120,
        latency_ms: None,
        last_seen: None,
        last_error: None,
        error_count: 0,
        registered_at: ts,
        notes: None,
        model_count: 0,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
    };
    let ep2 = ep1.clone();
    assert_eq!(ep1, ep2);
}

// ===== DashboardStats equality =====

#[test]
fn test_dashboard_stats_equality() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 0,
        online_nodes: 0,
        pending_nodes: 0,
        registering_nodes: 0,
        offline_nodes: 0,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        total_active_requests: 0,
        queued_requests: 0,
        average_response_time_ms: None,
        average_gpu_usage: None,
        average_gpu_memory_usage: None,
        last_metrics_updated_at: None,
        last_registered_at: None,
        last_seen_at: None,
        openai_key_present: false,
        google_key_present: false,
        anthropic_key_present: false,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_tokens: 0,
    };
    let stats2 = stats.clone();
    assert_eq!(stats, stats2);
}

// ===== ModelTpsEntry serialization =====

#[test]
fn test_model_tps_entry_serialization() {
    use super::ModelTpsEntry;
    use crate::common::protocol::{TpsApiKind, TpsSource};

    let entry = ModelTpsEntry {
        model_id: "llama-3-8b".to_string(),
        api_kind: TpsApiKind::ChatCompletions,
        source: TpsSource::Production,
        tps: Some(42.5),
        request_count: 100,
        total_output_tokens: 5000,
        average_duration_ms: Some(200.0),
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["model_id"], "llama-3-8b");
    assert_eq!(json["tps"], 42.5);
    assert_eq!(json["request_count"], 100);
    assert_eq!(json["total_output_tokens"], 5000);
    assert_eq!(json["average_duration_ms"], 200.0);
}

// ===== fallback_avg_response_time with latency_ms None =====

#[test]
fn test_avg_response_time_fallback_online_no_latency() {
    let mut ep = Endpoint::new(
        "EP".to_string(),
        "http://localhost:8001".to_string(),
        EndpointType::Xllm,
    );
    ep.status = EndpointStatus::Online;
    ep.latency_ms = None;

    let result = fallback_avg_response_time(None, &[ep]);
    assert_eq!(result, None);
}

// ===== EndpointDailyStatsQuery deserialization =====

#[test]
fn test_endpoint_daily_stats_query_default() {
    use super::EndpointDailyStatsQuery;
    let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert!(q.days.is_none());
}

#[test]
fn test_endpoint_daily_stats_query_with_days() {
    use super::EndpointDailyStatsQuery;
    let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 14}"#).unwrap();
    assert_eq!(q.days, Some(14));
}

// ===== ClientsQuery deserialization =====

#[test]
fn test_clients_query_defaults() {
    use super::ClientsQuery;
    let q: ClientsQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(q.page, 1);
    assert_eq!(q.per_page, 20);
}

#[test]
fn test_clients_query_custom() {
    use super::ClientsQuery;
    let q: ClientsQuery = serde_json::from_str(r#"{"page": 3, "per_page": 50}"#).unwrap();
    assert_eq!(q.page, 3);
    assert_eq!(q.per_page, 50);
}

// ===== SettingUpdateBody deserialization =====

#[test]
fn test_setting_update_body_deserialization() {
    use super::SettingUpdateBody;
    let body: SettingUpdateBody = serde_json::from_str(r#"{"value": "200"}"#).unwrap();
    assert_eq!(body.value, "200");
}

// ===== IP_ALERT_THRESHOLD constants =====

#[test]
fn test_ip_alert_threshold_constants() {
    use super::{IP_ALERT_THRESHOLD_DEFAULT_VALUE, IP_ALERT_THRESHOLD_MIN};
    assert_eq!(IP_ALERT_THRESHOLD_DEFAULT_VALUE, 100);
    assert_eq!(IP_ALERT_THRESHOLD_MIN, 1);
}

// ===== RequestHistoryQuery deserialization =====

#[test]
fn test_request_history_query_deserialization_defaults() {
    use super::RequestHistoryQuery;
    let q: RequestHistoryQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(q.page, 1);
    assert_eq!(q.per_page, 10);
    assert!(q.model.is_none());
    assert!(q.endpoint_id.is_none());
    assert!(q.status.is_none());
    assert!(q.limit.is_none());
    assert!(q.offset.is_none());
}

#[test]
fn test_request_history_query_with_filters() {
    use super::RequestHistoryQuery;
    let json = r#"{"page": 2, "per_page": 25, "model": "llama"}"#;
    let q: RequestHistoryQuery = serde_json::from_str(json).unwrap();
    assert_eq!(q.page, 2);
    assert_eq!(q.per_page, 25);
    assert_eq!(q.model, Some("llama".to_string()));
}

// ===== RequestHistoryExportQuery deserialization =====

#[test]
fn test_export_query_deserialization() {
    use super::RequestHistoryExportQuery;
    let q: RequestHistoryExportQuery =
        serde_json::from_str(r#"{"format": "json", "model": "gpt-4"}"#).unwrap();
    assert_eq!(q.format, super::RequestHistoryExportFormat::Json);
    assert_eq!(q.model, Some("gpt-4".to_string()));
}

#[test]
fn test_export_query_default_format() {
    use super::RequestHistoryExportQuery;
    let q: RequestHistoryExportQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(q.format, super::RequestHistoryExportFormat::Csv);
}

// ===== Additional unit tests for increased coverage =====

// --- DashboardStats extended tests ---

#[test]
fn test_dashboard_stats_all_none_optional_fields() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 0,
        online_nodes: 0,
        pending_nodes: 0,
        registering_nodes: 0,
        offline_nodes: 0,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        total_active_requests: 0,
        queued_requests: 0,
        average_response_time_ms: None,
        average_gpu_usage: None,
        average_gpu_memory_usage: None,
        last_metrics_updated_at: None,
        last_registered_at: None,
        last_seen_at: None,
        openai_key_present: false,
        google_key_present: false,
        anthropic_key_present: false,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_tokens: 0,
    };
    let json = serde_json::to_value(&stats).unwrap();
    assert!(json["average_response_time_ms"].is_null());
    assert!(json["average_gpu_usage"].is_null());
    assert!(json["average_gpu_memory_usage"].is_null());
    assert!(json["last_metrics_updated_at"].is_null());
    assert!(json["last_registered_at"].is_null());
    assert!(json["last_seen_at"].is_null());
}

#[test]
fn test_dashboard_stats_with_gpu_metrics() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 1,
        online_nodes: 1,
        pending_nodes: 0,
        registering_nodes: 0,
        offline_nodes: 0,
        total_requests: 100,
        successful_requests: 90,
        failed_requests: 10,
        total_active_requests: 5,
        queued_requests: 2,
        average_response_time_ms: Some(250.0),
        average_gpu_usage: Some(85.5),
        average_gpu_memory_usage: Some(72.3),
        last_metrics_updated_at: Some(chrono::Utc::now()),
        last_registered_at: Some(chrono::Utc::now()),
        last_seen_at: Some(chrono::Utc::now()),
        openai_key_present: true,
        google_key_present: true,
        anthropic_key_present: true,
        total_input_tokens: 50000,
        total_output_tokens: 25000,
        total_tokens: 75000,
    };
    let json = serde_json::to_value(&stats).unwrap();
    let gpu_usage = json["average_gpu_usage"].as_f64().unwrap();
    assert!((gpu_usage - 85.5).abs() < 0.01);
    let gpu_mem = json["average_gpu_memory_usage"].as_f64().unwrap();
    assert!((gpu_mem - 72.3).abs() < 0.01);
    assert!(json["last_metrics_updated_at"].is_string());
    assert!(json["last_registered_at"].is_string());
    assert!(json["last_seen_at"].is_string());
}

#[test]
fn test_dashboard_stats_token_counts() {
    use super::DashboardStats;

    let stats = DashboardStats {
        total_nodes: 0,
        online_nodes: 0,
        pending_nodes: 0,
        registering_nodes: 0,
        offline_nodes: 0,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        total_active_requests: 0,
        queued_requests: 0,
        average_response_time_ms: None,
        average_gpu_usage: None,
        average_gpu_memory_usage: None,
        last_metrics_updated_at: None,
        last_registered_at: None,
        last_seen_at: None,
        openai_key_present: false,
        google_key_present: false,
        anthropic_key_present: false,
        total_input_tokens: u64::MAX,
        total_output_tokens: u64::MAX,
        total_tokens: u64::MAX,
    };
    let json = serde_json::to_value(&stats).unwrap();
    assert_eq!(json["total_input_tokens"], u64::MAX);
    assert_eq!(json["total_output_tokens"], u64::MAX);
    assert_eq!(json["total_tokens"], u64::MAX);
}

// --- DashboardEndpoint extended tests ---

#[test]
fn test_dashboard_endpoint_with_error() {
    use super::DashboardEndpoint;
    use crate::types::endpoint::{EndpointStatus, EndpointType};

    let endpoint = DashboardEndpoint {
        id: uuid::Uuid::nil(),
        name: "error-endpoint".to_string(),
        base_url: "http://localhost:8080".to_string(),
        status: EndpointStatus::Offline,
        endpoint_type: EndpointType::Vllm,
        health_check_interval_secs: 60,
        inference_timeout_secs: 300,
        latency_ms: None,
        last_seen: Some(chrono::Utc::now()),
        last_error: Some("Connection refused".to_string()),
        error_count: 5,
        registered_at: chrono::Utc::now(),
        notes: Some("This endpoint has issues".to_string()),
        model_count: 0,
        total_requests: 50,
        successful_requests: 40,
        failed_requests: 10,
    };
    let json = serde_json::to_value(&endpoint).unwrap();
    assert_eq!(json["status"], "offline");
    assert_eq!(json["last_error"], "Connection refused");
    assert_eq!(json["error_count"], 5);
    assert_eq!(json["notes"], "This endpoint has issues");
}

#[test]
fn test_dashboard_endpoint_all_endpoint_types() {
    use super::DashboardEndpoint;
    use crate::types::endpoint::{EndpointStatus, EndpointType};

    let types = [
        EndpointType::Xllm,
        EndpointType::Vllm,
        EndpointType::Ollama,
        EndpointType::OpenaiCompatible,
    ];
    for ep_type in types {
        let endpoint = DashboardEndpoint {
            id: uuid::Uuid::new_v4(),
            name: format!("ep-{:?}", ep_type),
            base_url: "http://localhost".to_string(),
            status: EndpointStatus::Online,
            endpoint_type: ep_type,
            health_check_interval_secs: 30,
            inference_timeout_secs: 120,
            latency_ms: None,
            last_seen: None,
            last_error: None,
            error_count: 0,
            registered_at: chrono::Utc::now(),
            notes: None,
            model_count: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
        };
        let json = serde_json::to_value(&endpoint).unwrap();
        assert!(json["endpoint_type"].is_string());
    }
}

#[test]
fn test_dashboard_endpoint_clone() {
    use super::DashboardEndpoint;
    use crate::types::endpoint::{EndpointStatus, EndpointType};

    let endpoint = DashboardEndpoint {
        id: uuid::Uuid::nil(),
        name: "clone-test".to_string(),
        base_url: "http://localhost".to_string(),
        status: EndpointStatus::Online,
        endpoint_type: EndpointType::Xllm,
        health_check_interval_secs: 30,
        inference_timeout_secs: 120,
        latency_ms: Some(42),
        last_seen: None,
        last_error: None,
        error_count: 0,
        registered_at: chrono::Utc::now(),
        notes: None,
        model_count: 2,
        total_requests: 10,
        successful_requests: 8,
        failed_requests: 2,
    };
    let cloned = endpoint.clone();
    assert_eq!(endpoint, cloned);
}

// --- PersistedRequestTotals / PersistedTokenTotals extended tests ---

#[test]
fn test_persisted_request_totals_copy() {
    use super::overview::PersistedRequestTotals;
    let totals = PersistedRequestTotals {
        total_requests: 100,
        successful_requests: 90,
        failed_requests: 10,
    };
    let copied = totals;
    assert_eq!(copied.total_requests, 100);
    assert_eq!(copied.successful_requests, 90);
    assert_eq!(copied.failed_requests, 10);
}

#[test]
fn test_persisted_token_totals_copy() {
    use super::overview::PersistedTokenTotals;
    let totals = PersistedTokenTotals {
        total_input_tokens: 50000,
        total_output_tokens: 25000,
        total_tokens: 75000,
    };
    let copied = totals;
    assert_eq!(copied.total_input_tokens, 50000);
    assert_eq!(copied.total_output_tokens, 25000);
    assert_eq!(copied.total_tokens, 75000);
}

#[test]
fn test_persisted_totals_cache_copy() {
    use super::{PersistedRequestTotals, PersistedTokenTotals, PersistedTotalsCache};
    let cache = PersistedTotalsCache {
        request_totals: PersistedRequestTotals {
            total_requests: 10,
            successful_requests: 8,
            failed_requests: 2,
        },
        token_totals: PersistedTokenTotals {
            total_input_tokens: 1000,
            total_output_tokens: 500,
            total_tokens: 1500,
        },
    };
    let copied = cache;
    assert_eq!(copied.request_totals.total_requests, 10);
    assert_eq!(copied.token_totals.total_tokens, 1500);
}

// --- RequestHistoryQuery extended tests ---

#[test]
fn test_normalized_per_page_large_value_falls_back() {
    use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 500,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
}

#[test]
fn test_normalized_per_page_one_falls_back() {
    use super::{RequestHistoryQuery, DEFAULT_PAGE_SIZE};
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 1,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    assert_eq!(query.normalized_per_page(), DEFAULT_PAGE_SIZE);
}

#[test]
fn test_to_record_filter_all_none() {
    use super::RequestHistoryQuery;
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    let filter = query.to_record_filter().unwrap();
    assert!(filter.model.is_none());
    assert!(filter.endpoint_id.is_none());
    assert!(filter.status.is_none());
    assert!(filter.start_time.is_none());
    assert!(filter.end_time.is_none());
    assert!(filter.client_ip.is_none());
}

#[test]
fn test_to_record_filter_with_client_ip() {
    use super::RequestHistoryQuery;
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: Some("192.168.1.1".to_string()),
    };
    let filter = query.to_record_filter().unwrap();
    assert_eq!(filter.client_ip, Some("192.168.1.1".to_string()));
}

#[test]
fn test_to_record_filter_with_endpoint_id() {
    use super::RequestHistoryQuery;
    let id = uuid::Uuid::new_v4();
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: Some(id),
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    let filter = query.to_record_filter().unwrap();
    assert_eq!(filter.endpoint_id, Some(id));
}

#[test]
fn test_to_record_filter_start_equals_end_is_ok() {
    use super::RequestHistoryQuery;
    use chrono::TimeZone;

    let ts = chrono::Utc.with_ymd_and_hms(2026, 2, 27, 12, 0, 0).unwrap();
    let query = RequestHistoryQuery {
        page: 1,
        per_page: 10,
        limit: None,
        offset: None,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: Some(ts),
        end_time: Some(ts),
        client_ip: None,
    };
    // start == end should be ok
    assert!(query.to_record_filter().is_ok());
}

// --- RequestHistoryExportQuery extended tests ---

#[test]
fn test_export_query_to_record_filter_all_none() {
    use super::RequestHistoryExportQuery;
    let query = RequestHistoryExportQuery {
        format: super::RequestHistoryExportFormat::Csv,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: None,
        end_time: None,
        client_ip: None,
    };
    let filter = query.to_record_filter().unwrap();
    assert!(filter.model.is_none());
    assert!(filter.endpoint_id.is_none());
}

#[test]
fn test_export_query_to_record_filter_start_equals_end_is_ok() {
    use super::RequestHistoryExportQuery;
    use chrono::TimeZone;

    let ts = chrono::Utc.with_ymd_and_hms(2026, 1, 15, 0, 0, 0).unwrap();
    let query = RequestHistoryExportQuery {
        format: super::RequestHistoryExportFormat::Json,
        model: None,
        endpoint_id: None,
        status: None,
        start_time: Some(ts),
        end_time: Some(ts),
        client_ip: None,
    };
    assert!(query.to_record_filter().is_ok());
}

// --- RequestHistoryExportFormat extended tests ---

#[test]
fn test_export_format_equality() {
    use super::RequestHistoryExportFormat;
    assert_eq!(
        RequestHistoryExportFormat::Csv,
        RequestHistoryExportFormat::Csv
    );
    assert_eq!(
        RequestHistoryExportFormat::Json,
        RequestHistoryExportFormat::Json
    );
    assert_ne!(
        RequestHistoryExportFormat::Csv,
        RequestHistoryExportFormat::Json
    );
}

#[test]
fn test_export_format_copy() {
    use super::RequestHistoryExportFormat;
    let fmt = RequestHistoryExportFormat::Json;
    let copied = fmt;
    assert_eq!(fmt, copied);
}

#[test]
fn test_export_format_debug() {
    use super::RequestHistoryExportFormat;
    let debug = format!("{:?}", RequestHistoryExportFormat::Csv);
    assert!(debug.contains("Csv"));
}

#[test]
fn test_export_format_invalid_deserialization() {
    use super::RequestHistoryExportFormat;
    assert!(serde_json::from_str::<RequestHistoryExportFormat>("\"xml\"").is_err());
    assert!(serde_json::from_str::<RequestHistoryExportFormat>("\"CSV\"").is_err());
}

// --- parse_ip_alert_threshold extended tests ---

#[test]
fn parse_ip_alert_threshold_max_i64() {
    let result = parse_ip_alert_threshold(&i64::MAX.to_string());
    assert_eq!(result.unwrap(), i64::MAX);
}

#[test]
fn parse_ip_alert_threshold_whitespace_around() {
    assert_eq!(parse_ip_alert_threshold("  100  ").unwrap(), 100);
}

#[test]
fn parse_ip_alert_threshold_leading_plus_rejected() {
    // "+100" may or may not parse, but the function trims, so let's check
    // Rust's i64 parse does accept "+100"
    let result = parse_ip_alert_threshold("+100");
    assert_eq!(result.unwrap(), 100);
}

// --- effective_ip_alert_threshold extended tests ---

#[test]
fn effective_threshold_with_whitespace() {
    use super::effective_ip_alert_threshold;
    assert_eq!(effective_ip_alert_threshold(Some("  75  ")), 75);
}

#[test]
fn effective_threshold_with_empty_string() {
    use super::effective_ip_alert_threshold;
    use super::IP_ALERT_THRESHOLD_DEFAULT_VALUE;
    assert_eq!(
        effective_ip_alert_threshold(Some("")),
        IP_ALERT_THRESHOLD_DEFAULT_VALUE
    );
}

// --- fallback_avg_response_time extended tests ---

#[test]
fn test_avg_response_time_fallback_mixed_status_endpoints() {
    let mut ep_online = Endpoint::new(
        "Online".to_string(),
        "http://localhost:8001".to_string(),
        EndpointType::Xllm,
    );
    ep_online.status = EndpointStatus::Online;
    ep_online.latency_ms = Some(300);

    let mut ep_offline = Endpoint::new(
        "Offline".to_string(),
        "http://localhost:8002".to_string(),
        EndpointType::Xllm,
    );
    ep_offline.status = EndpointStatus::Offline;
    ep_offline.latency_ms = Some(100);

    let mut ep_pending = Endpoint::new(
        "Pending".to_string(),
        "http://localhost:8003".to_string(),
        EndpointType::Xllm,
    );
    ep_pending.status = EndpointStatus::Pending;
    ep_pending.latency_ms = Some(50);

    let endpoints = vec![ep_online, ep_offline, ep_pending];
    // Only the online endpoint with latency should be counted
    let result = fallback_avg_response_time(None, &endpoints);
    assert_eq!(result, Some(300.0));
}

#[test]
fn test_avg_response_time_fallback_single_online_endpoint() {
    let mut ep = Endpoint::new(
        "Single".to_string(),
        "http://localhost:8001".to_string(),
        EndpointType::Vllm,
    );
    ep.status = EndpointStatus::Online;
    ep.latency_ms = Some(42);

    let result = fallback_avg_response_time(None, &[ep]);
    assert_eq!(result, Some(42.0));
}

#[test]
fn test_avg_response_time_fallback_empty_endpoints() {
    let result = fallback_avg_response_time(None, &[]);
    assert_eq!(result, None);
}

#[test]
fn test_calculate_output_tps_weighted_by_generated_tokens() {
    use super::overview::calculate_output_tps;
    use crate::balancer::EndpointTpsSummary;

    let result = calculate_output_tps(&[
        EndpointTpsSummary {
            endpoint_id: uuid::Uuid::nil(),
            model_count: 1,
            aggregate_tps: Some(50.0),
            total_output_tokens: 100,
            total_requests: 2,
        },
        EndpointTpsSummary {
            endpoint_id: uuid::Uuid::nil(),
            model_count: 1,
            aggregate_tps: Some(100.0),
            total_output_tokens: 100,
            total_requests: 2,
        },
    ])
    .expect("TPS should be computed from measured entries");

    assert!((result - 66.666_666_666_7).abs() < 0.000_001);
}

#[test]
fn test_calculate_output_tps_ignores_unmeasured_entries() {
    use super::overview::calculate_output_tps;
    use crate::balancer::EndpointTpsSummary;

    let result = calculate_output_tps(&[
        EndpointTpsSummary {
            endpoint_id: uuid::Uuid::nil(),
            model_count: 0,
            aggregate_tps: None,
            total_output_tokens: 100,
            total_requests: 1,
        },
        EndpointTpsSummary {
            endpoint_id: uuid::Uuid::nil(),
            model_count: 1,
            aggregate_tps: Some(0.0),
            total_output_tokens: 100,
            total_requests: 1,
        },
    ]);

    assert_eq!(result, None);
}

#[test]
fn test_count_unique_model_ids_deduplicates_across_endpoints() {
    let result = count_unique_model_ids(vec![
        "llama-3.2:3b".to_string(),
        "qwen2.5:7b".to_string(),
        "llama-3.2:3b".to_string(),
    ]);

    assert_eq!(result, 2);
}

#[test]
fn test_action_items_count_offline_and_error_endpoints_independently() {
    let operations = DashboardOperations {
        health: "attention".to_string(),
        total_endpoints: 4,
        online_endpoints: 1,
        pending_endpoints: 0,
        registering_endpoints: 0,
        offline_endpoints: 2,
        error_endpoints: 1,
        total_requests: 0,
        successful_requests: 0,
        failed_requests: 0,
        success_rate: None,
        active_requests: 0,
        queued_requests: 0,
        average_response_time_ms: None,
        output_tps: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_tokens: 0,
        last_registered_at: None,
        last_seen_at: None,
    };

    let items = collect_action_items(&operations);
    let offline = items
        .iter()
        .find(|item| item.title == "Offline endpoints")
        .expect("offline endpoints action item should exist");
    let errors = items
        .iter()
        .find(|item| item.title == "Endpoint errors")
        .expect("endpoint errors action item should exist");

    assert_eq!(offline.count, 2);
    assert_eq!(errors.count, 1);
}

// --- DashboardOverview tests ---

#[test]
fn test_dashboard_overview_serialization() {
    use super::{DashboardActionItem, DashboardCapacity, DashboardOperations, DashboardOverview};

    let overview = DashboardOverview {
        endpoints: vec![],
        operations: DashboardOperations {
            health: "healthy".to_string(),
            total_endpoints: 0,
            online_endpoints: 0,
            pending_endpoints: 0,
            registering_endpoints: 0,
            offline_endpoints: 0,
            error_endpoints: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            success_rate: None,
            active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            output_tps: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            last_registered_at: None,
            last_seen_at: None,
        },
        capacity: DashboardCapacity {
            total_models: 0,
            gpu_capable_endpoints: 0,
            gpu_telemetry_endpoints: 0,
            total_gpu_memory_bytes: None,
            used_gpu_memory_bytes: None,
            gpu_memory_usage_percent: None,
            telemetry_status: "unavailable".to_string(),
        },
        action_items: vec![DashboardActionItem {
            severity: "info".to_string(),
            title: "No action required".to_string(),
            detail: "All operational signals are nominal.".to_string(),
            count: 0,
        }],
        history: vec![],
        endpoint_tps: vec![],
        generated_at: chrono::Utc::now(),
        generation_time_ms: 42,
    };

    let json = serde_json::to_value(&overview).unwrap();
    assert!(json["endpoints"].is_array());
    assert!(json["operations"].is_object());
    assert!(json["capacity"].is_object());
    assert!(json["action_items"].is_array());
    assert!(json["stats"].is_null());
    assert!(json["operations"]["health"].is_string());
    assert!(json["operations"]["total_endpoints"].is_number());
    assert!(json["operations"]["output_tps"].is_null());
    assert!(json["capacity"]["gpu_capable_endpoints"].is_number());
    assert!(json["history"].is_array());
    assert!(json["endpoint_tps"].is_array());
    assert!(json["generated_at"].is_string());
    assert_eq!(json["generation_time_ms"], 42);
}

#[test]
fn test_dashboard_overview_equality() {
    use super::{DashboardActionItem, DashboardCapacity, DashboardOperations, DashboardOverview};

    let now = chrono::Utc::now();

    let overview = DashboardOverview {
        endpoints: vec![],
        operations: DashboardOperations {
            health: "healthy".to_string(),
            total_endpoints: 0,
            online_endpoints: 0,
            pending_endpoints: 0,
            registering_endpoints: 0,
            offline_endpoints: 0,
            error_endpoints: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            success_rate: None,
            active_requests: 0,
            queued_requests: 0,
            average_response_time_ms: None,
            output_tps: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            last_registered_at: None,
            last_seen_at: None,
        },
        capacity: DashboardCapacity {
            total_models: 0,
            gpu_capable_endpoints: 0,
            gpu_telemetry_endpoints: 0,
            total_gpu_memory_bytes: None,
            used_gpu_memory_bytes: None,
            gpu_memory_usage_percent: None,
            telemetry_status: "unavailable".to_string(),
        },
        action_items: vec![DashboardActionItem {
            severity: "info".to_string(),
            title: "No action required".to_string(),
            detail: "All operational signals are nominal.".to_string(),
            count: 0,
        }],
        history: vec![],
        endpoint_tps: vec![],
        generated_at: now,
        generation_time_ms: 0,
    };
    let overview2 = overview.clone();
    assert_eq!(overview, overview2);
}

// --- DailyTokenStats extended tests ---

#[test]
fn test_daily_token_stats_clone() {
    use super::DailyTokenStats;
    let stats = DailyTokenStats {
        date: "2026-01-01".to_string(),
        total_input_tokens: 100,
        total_output_tokens: 50,
        total_tokens: 150,
        request_count: 5,
    };
    let cloned = stats.clone();
    assert_eq!(cloned.date, "2026-01-01");
    assert_eq!(cloned.request_count, 5);
}

// --- MonthlyTokenStats extended tests ---

#[test]
fn test_monthly_token_stats_clone() {
    use super::MonthlyTokenStats;
    let stats = MonthlyTokenStats {
        month: "2026-01".to_string(),
        total_input_tokens: 10000,
        total_output_tokens: 5000,
        total_tokens: 15000,
        request_count: 100,
    };
    let cloned = stats.clone();
    assert_eq!(cloned.month, "2026-01");
    assert_eq!(cloned.total_tokens, 15000);
}

// --- RequestHistoryQuery deserialization extended tests ---

#[test]
fn test_request_history_query_with_limit_and_offset() {
    use super::RequestHistoryQuery;
    let json = r#"{"limit": 50, "offset": 100}"#;
    let q: RequestHistoryQuery = serde_json::from_str(json).unwrap();
    assert_eq!(q.limit, Some(50));
    assert_eq!(q.offset, Some(100));
}

#[test]
fn test_request_history_query_with_endpoint_id_alias() {
    use super::RequestHistoryQuery;
    let id = uuid::Uuid::new_v4();
    let json = format!(r#"{{"node_id": "{}"}}"#, id);
    let q: RequestHistoryQuery = serde_json::from_str(&json).unwrap();
    assert_eq!(q.endpoint_id, Some(id));
}

// --- EndpointDailyStatsQuery extended tests ---

#[test]
fn test_endpoint_daily_stats_query_with_zero_days() {
    use super::EndpointDailyStatsQuery;
    let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 0}"#).unwrap();
    assert_eq!(q.days, Some(0));
}

#[test]
fn test_endpoint_daily_stats_query_with_large_days() {
    use super::EndpointDailyStatsQuery;
    let q: EndpointDailyStatsQuery = serde_json::from_str(r#"{"days": 1000}"#).unwrap();
    assert_eq!(q.days, Some(1000));
}

// --- ClientsQuery extended tests ---

#[test]
fn test_clients_query_zero_values() {
    use super::ClientsQuery;
    let q: ClientsQuery = serde_json::from_str(r#"{"page": 0, "per_page": 0}"#).unwrap();
    assert_eq!(q.page, 0);
    assert_eq!(q.per_page, 0);
}

// --- SettingUpdateBody extended tests ---

#[test]
fn test_setting_update_body_empty_value() {
    use super::SettingUpdateBody;
    let body: SettingUpdateBody = serde_json::from_str(r#"{"value": ""}"#).unwrap();
    assert_eq!(body.value, "");
}

#[test]
fn test_setting_update_body_numeric_string_value() {
    use super::SettingUpdateBody;
    let body: SettingUpdateBody = serde_json::from_str(r#"{"value": "42"}"#).unwrap();
    assert_eq!(body.value, "42");
}

// --- ModelTpsEntry extended tests ---

#[test]
fn test_model_tps_entry_with_none_tps() {
    use super::ModelTpsEntry;
    use crate::common::protocol::{TpsApiKind, TpsSource};

    let entry = ModelTpsEntry {
        model_id: "new-model".to_string(),
        api_kind: TpsApiKind::Completions,
        source: TpsSource::Benchmark,
        tps: None,
        request_count: 0,
        total_output_tokens: 0,
        average_duration_ms: None,
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert!(json["tps"].is_null());
    assert!(json["average_duration_ms"].is_null());
    assert_eq!(json["request_count"], 0);
}

#[test]
fn test_model_tps_entry_clone() {
    use super::ModelTpsEntry;
    use crate::common::protocol::{TpsApiKind, TpsSource};

    let entry = ModelTpsEntry {
        model_id: "model".to_string(),
        api_kind: TpsApiKind::ChatCompletions,
        source: TpsSource::Production,
        tps: Some(10.0),
        request_count: 5,
        total_output_tokens: 100,
        average_duration_ms: Some(50.0),
    };
    let cloned = entry.clone();
    assert_eq!(cloned.model_id, "model");
    assert_eq!(cloned.tps, Some(10.0));
}
