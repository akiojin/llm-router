//! Dashboard smoke tests
//!
//! Axum router を直接呼び出し、ダッシュボードの主要なHTTP経路が期待通りに
//! 応答することを確認する。UI機能の最小限のE2E保証として利用する。

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ollama_coordinator_common::protocol::RegisterRequest;
use ollama_coordinator_coordinator::{
    api,
    balancer::{LoadManager, MetricsUpdate, RequestOutcome},
    registry::AgentRegistry,
    AppState,
};
use serde_json::Value;
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use tower::ServiceExt;

fn build_router() -> (Router, AgentRegistry, LoadManager) {
    let registry = AgentRegistry::new();
    let load_manager = LoadManager::new(registry.clone());
    let state = AppState {
        registry: registry.clone(),
        load_manager: load_manager.clone(),
    };
    let router = api::create_router(state);
    (router, registry, load_manager)
}

#[tokio::test]
async fn dashboard_serves_static_index() {
    let (router, _, _) = build_router();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dashboard/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();

    let content_type = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/html"),
        "content-type was {content_type}"
    );
    assert!(
        bytes.starts_with(b"<!DOCTYPE html>"),
        "unexpected body prefix: {:?}",
        &bytes[..bytes.len().min(32)]
    );
}

#[tokio::test]
async fn dashboard_agents_and_stats_reflect_registry() {
    let (router, registry, load_manager) = build_router();

    let agent_id = registry
        .register(RegisterRequest {
            machine_name: "agent-smoke".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 42)),
            ollama_version: "0.1.0".into(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    load_manager
        .record_metrics(MetricsUpdate {
            agent_id,
            cpu_usage: 12.5,
            memory_usage: 34.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 2,
            average_response_time_ms: Some(110.0),
        })
        .await
        .unwrap();
    load_manager.begin_request(agent_id).await.unwrap();
    load_manager
        .finish_request(
            agent_id,
            RequestOutcome::Success,
            Duration::from_millis(140),
        )
        .await
        .unwrap();

    let agents_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(agents_response.status(), StatusCode::OK);
    let body = to_bytes(agents_response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let agents: Value = serde_json::from_slice(&body).unwrap();

    assert!(agents.is_array(), "expected array payload, got {agents:?}");
    let agent = &agents.as_array().unwrap()[0];
    assert_eq!(agent["machine_name"], "agent-smoke");
    assert_eq!(agent["status"], "online");
    assert_eq!(agent["total_requests"], 1);
    assert_eq!(agent["successful_requests"], 1);
    assert!(agent["loaded_models"].is_array());

    let stats_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(stats_response.status(), StatusCode::OK);
    let stats_body = to_bytes(stats_response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let stats: Value = serde_json::from_slice(&stats_body).unwrap();

    assert_eq!(stats["total_agents"], 1);
    assert_eq!(stats["online_agents"], 1);
    assert_eq!(stats["total_requests"], 1);
    assert_eq!(stats["successful_requests"], 1);
}

#[tokio::test]
async fn dashboard_request_history_tracks_activity() {
    let (router, registry, load_manager) = build_router();

    let agent_id = registry
        .register(RegisterRequest {
            machine_name: "history-agent".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
            ollama_version: "0.1.0".into(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    load_manager.begin_request(agent_id).await.unwrap();
    load_manager
        .finish_request(
            agent_id,
            RequestOutcome::Success,
            Duration::from_millis(200),
        )
        .await
        .unwrap();
    load_manager.begin_request(agent_id).await.unwrap();
    load_manager
        .finish_request(agent_id, RequestOutcome::Error, Duration::from_millis(250))
        .await
        .unwrap();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/request-history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let history: Value = serde_json::from_slice(&body).unwrap();

    assert!(
        history.is_array(),
        "expected history array, got {history:?}"
    );
    assert_eq!(history.as_array().unwrap().len(), 60);
    let latest = history.as_array().unwrap().last().unwrap();
    let success = latest["success"].as_u64().unwrap_or_default();
    let error = latest["error"].as_u64().unwrap_or_default();
    assert!(success >= 1, "expected latest success >= 1, got {success}");
    assert!(error >= 1, "expected latest error >= 1, got {error}");
}

#[tokio::test]
async fn dashboard_overview_returns_combined_payload() {
    let (router, registry, load_manager) = build_router();

    let agent_id = registry
        .register(RegisterRequest {
            machine_name: "overview-smoke".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)),
            ollama_version: "0.1.0".into(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    load_manager.begin_request(agent_id).await.unwrap();
    load_manager
        .finish_request(agent_id, RequestOutcome::Success, Duration::from_millis(90))
        .await
        .unwrap();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/dashboard/overview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let overview: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(overview["agents"].as_array().unwrap().len(), 1);
    let agent = overview["agents"].as_array().unwrap().first().unwrap();
    assert!(agent["loaded_models"].is_array());
    assert_eq!(overview["stats"]["total_agents"], 1);
    assert_eq!(overview["history"].as_array().unwrap().len(), 60);
    assert!(overview["generated_at"].is_string());
    assert!(overview["generation_time_ms"].as_u64().is_some());
}

#[tokio::test]
async fn dashboard_agent_metrics_endpoint_returns_history() {
    let (router, registry, load_manager) = build_router();

    let agent_id = registry
        .register(RegisterRequest {
            machine_name: "metrics-endpoint".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 11)),
            ollama_version: "0.1.0".into(),
            ollama_port: 11434,
        })
        .await
        .unwrap()
        .agent_id;

    load_manager
        .record_metrics(MetricsUpdate {
            agent_id,
            cpu_usage: 42.0,
            memory_usage: 55.0,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 2,
            average_response_time_ms: Some(105.0),
        })
        .await
        .unwrap();

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/dashboard/metrics/{agent_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let metrics: Value = serde_json::from_slice(&body).unwrap();
    assert!(metrics.is_array());
    assert_eq!(metrics.as_array().unwrap().len(), 1);
    assert_eq!(
        metrics.as_array().unwrap()[0]["agent_id"].as_str().unwrap(),
        agent_id.to_string()
    );
}
