use axum::{http::header, http::StatusCode, response::IntoResponse};
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};
use std::sync::Once;

static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);
static COUNTER: Lazy<IntCounterVec> = Lazy::new(|| {
    let opts = Opts::new("cloud_requests_total", "Cloud LLM requests");
    IntCounterVec::new(opts, &["provider", "status"]).expect("counter vec")
});
static HISTO: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "cloud_request_latency_seconds",
        "Cloud LLM request latency (seconds)",
    );
    HistogramVec::new(opts, &["provider"]).expect("histogram vec")
});

/// Register cloud metrics (idempotent).
///
/// arch-review [L18]: 従来は毎回の `record()` から呼ばれるたびに
/// `Box::new(clone())` と（初回以降は必ず失敗する）register を実行していた。
/// `Once` で初回のみ登録し、ホットパスのオーバーヘッドを排除する。
pub fn init_metrics() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        REGISTRY.register(Box::new(COUNTER.clone())).ok();
        REGISTRY.register(Box::new(HISTO.clone())).ok();
    });
}

/// Record a cloud provider request with status and latency (ms).
pub fn record(provider: &str, status_code: u16, latency_ms: u128) {
    // ensure registry setup
    init_metrics();
    let status_str = status_code.to_string();
    COUNTER
        .with_label_values(&[provider, status_str.as_str()])
        .inc();
    let secs = latency_ms as f64 / 1000.0;
    HISTO.with_label_values(&[provider]).observe(secs);
}

/// Expose Prometheus text format for cloud metrics.
pub async fn export_metrics() -> impl IntoResponse {
    init_metrics();
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buf = Vec::new();
    let res = if let Err(e) = encoder.encode(&metric_families, &mut buf) {
        axum::response::Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(axum::body::Body::from(format!("encode error: {e}")))
            .unwrap()
    } else {
        let body = String::from_utf8(buf).unwrap_or_default();
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_export() {
        record("openai", StatusCode::OK.as_u16(), 123);
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();
        let mut buf = Vec::new();
        encoder.encode(&metric_families, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("cloud_requests_total"));
        assert!(out.contains("cloud_request_latency_seconds"));
    }

    #[test]
    fn record_multiple_providers() {
        record("openai", 200, 100);
        record("google", 200, 150);
        record("anthropic", 200, 200);
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();
        let mut buf = Vec::new();
        encoder.encode(&metric_families, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("openai"));
        assert!(out.contains("google"));
        assert!(out.contains("anthropic"));
    }

    #[test]
    fn record_various_status_codes() {
        record("openai", 200, 50);
        record("openai", 400, 25);
        record("openai", 500, 30);
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();
        let mut buf = Vec::new();
        encoder.encode(&metric_families, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("200"));
        assert!(out.contains("400"));
        assert!(out.contains("500"));
    }

    #[test]
    fn init_metrics_is_idempotent() {
        init_metrics();
        init_metrics();
        init_metrics();
        // No panic means success
    }

    #[tokio::test]
    async fn export_metrics_returns_text_plain() {
        record("test", 200, 10);
        let response = export_metrics().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get(header::CONTENT_TYPE);
        assert!(content_type.is_some());
        assert!(content_type
            .unwrap()
            .to_str()
            .unwrap()
            .contains("text/plain"));
    }

    #[test]
    fn latency_conversion_ms_to_seconds() {
        record("latency_test", 200, 1500);
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();
        let mut buf = Vec::new();
        encoder.encode(&metric_families, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        // 1500ms = 1.5s, bucket should contain this value
        assert!(out.contains("cloud_request_latency_seconds"));
    }
}
