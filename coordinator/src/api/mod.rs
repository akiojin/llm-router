//! REST APIハンドラー
//!
//! エージェント登録、ヘルスチェック、プロキシAPI

pub mod agent;
pub mod dashboard;
pub mod health;
pub mod metrics;
pub mod models;
pub mod proxy;

use crate::AppState;
use axum::{
    body::Body,
    extract::Path as AxumPath,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use include_dir::{include_dir, Dir, File};
use mime_guess::MimeGuess;

static DASHBOARD_ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/web/static");
const DASHBOARD_INDEX: &str = "index.html";

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/agents",
            post(agent::register_agent).get(agent::list_agents),
        )
        .route("/api/agents/:agent_id", delete(agent::delete_agent))
        .route(
            "/api/agents/:agent_id/disconnect",
            post(agent::disconnect_agent),
        )
        .route(
            "/api/agents/:agent_id/settings",
            put(agent::update_agent_settings),
        )
        .route(
            "/api/agents/:agent_id/metrics",
            post(metrics::update_metrics),
        )
        .route("/api/agents/metrics", get(agent::list_agent_metrics))
        .route("/api/metrics/summary", get(agent::metrics_summary))
        .route("/api/dashboard/agents", get(dashboard::get_agents))
        .route("/api/dashboard/stats", get(dashboard::get_stats))
        .route(
            "/api/dashboard/request-history",
            get(dashboard::get_request_history),
        )
        .route("/api/dashboard/overview", get(dashboard::get_overview))
        .route(
            "/api/dashboard/metrics/:agent_id",
            get(dashboard::get_agent_metrics),
        )
        .route(
            "/api/dashboard/request-responses",
            get(dashboard::list_request_responses),
        )
        .route(
            "/api/dashboard/request-responses/:id",
            get(dashboard::get_request_response_detail),
        )
        .route(
            "/api/dashboard/request-responses/export",
            get(dashboard::export_request_responses),
        )
        .route("/api/health", post(health::health_check))
        .route("/api/chat", post(proxy::proxy_chat))
        .route("/api/generate", post(proxy::proxy_generate))
        // モデル管理API (SPEC-8ae67d67)
        .route("/api/models/available", get(models::get_available_models))
        .route("/api/models/distribute", post(models::distribute_models))
        .route(
            "/api/agents/:agent_id/models",
            get(models::get_agent_models),
        )
        .route(
            "/api/agents/:agent_id/models/pull",
            post(models::pull_model_to_agent),
        )
        .route("/api/tasks/:task_id", get(models::get_task_progress))
        .route(
            "/api/tasks/:task_id/progress",
            post(models::update_progress),
        )
        .route("/dashboard", get(serve_dashboard_index))
        .route("/dashboard/", get(serve_dashboard_index))
        .route("/dashboard/*path", get(serve_dashboard_asset))
        .with_state(state)
}

async fn serve_dashboard_index() -> Response {
    embedded_dashboard_response(DASHBOARD_INDEX)
}

async fn serve_dashboard_asset(AxumPath(request_path): AxumPath<String>) -> Response {
    let normalized = normalize_dashboard_path(&request_path);
    match normalized {
        Some(path) => embedded_dashboard_response(&path),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn embedded_dashboard_response(path: &str) -> Response {
    match DASHBOARD_ASSETS.get_file(path) {
        Some(file) => file_response(file),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn file_response(file: &File<'_>) -> Response {
    let mime = MimeGuess::from_path(file.path())
        .first_or_octet_stream()
        .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(file.contents().to_vec()))
        .expect("failed to build embedded dashboard response")
}

fn normalize_dashboard_path(request_path: &str) -> Option<String> {
    let trimmed = request_path.trim_matches('/');
    if trimmed.is_empty() {
        return Some(DASHBOARD_INDEX.to_string());
    }
    if trimmed.contains("..") || trimmed.contains('\\') {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate},
        registry::AgentRegistry,
        tasks::DownloadTaskManager,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use ollama_coordinator_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use tower::Service;

    fn test_state() -> (AppState, AgentRegistry) {
        let registry = AgentRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history =
            std::sync::Arc::new(crate::db::request_history::RequestHistoryStorage::new().unwrap());
        let task_manager = DownloadTaskManager::new();
        let state = AppState {
            registry: registry.clone(),
            load_manager,
            request_history,
            task_manager,
        };
        (state, registry)
    }

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: None,
        }]
    }

    #[tokio::test]
    async fn test_dashboard_static_served() {
        let (state, _) = test_state();
        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/dashboard/index.html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, 1024 * 1024).await.unwrap();

        assert_eq!(status, StatusCode::OK);
        let content_type = parts.headers[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap();
        assert!(content_type.starts_with("text/html"));
        assert!(bytes.starts_with(b"<!DOCTYPE html"));
    }

    #[tokio::test]
    async fn test_dashboard_agents_endpoint_returns_json() {
        let (state, registry) = test_state();
        registry
            .register(RegisterRequest {
                machine_name: "test-agent".into(),
                ip_address: "127.0.0.1".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap();

        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .uri("/api/dashboard/agents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let agents: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(agents.is_array());
        assert_eq!(agents.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_dashboard_overview_endpoint_returns_all_sections() {
        let (state, registry) = test_state();
        registry
            .register(RegisterRequest {
                machine_name: "overview-agent".into(),
                ip_address: "127.0.0.1".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap();

        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .uri("/api/dashboard/overview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let overview: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(overview["agents"].is_array());
        assert!(overview["stats"].is_object());
        assert!(overview["history"].is_array());
        assert!(overview["generated_at"].is_string());
        assert!(overview["generation_time_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_dashboard_metrics_endpoint_returns_history() {
        let (state, registry) = test_state();
        let agent_id = registry
            .register(RegisterRequest {
                machine_name: "metrics-route".into(),
                ip_address: "127.0.0.1".parse().unwrap(),
                ollama_version: "0.1.0".into(),
                ollama_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            })
            .await
            .unwrap()
            .agent_id;

        state
            .load_manager
            .record_metrics(MetricsUpdate {
                agent_id,
                cpu_usage: 12.0,
                memory_usage: 34.0,
                gpu_usage: None,
                gpu_memory_usage: None,
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: Some(90.0),
            })
            .await
            .unwrap();

        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .uri(format!("/api/dashboard/metrics/{agent_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let metrics: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(metrics.is_array());
        assert_eq!(metrics.as_array().unwrap().len(), 1);
        assert_eq!(
            metrics.as_array().unwrap()[0]["agent_id"].as_str().unwrap(),
            agent_id.to_string()
        );
    }
}
