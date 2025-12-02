//! REST APIハンドラー
//!
//! ノード登録、ヘルスチェック、プロキシAPI

pub mod api_keys;
pub mod auth;
pub mod dashboard;
pub mod health;
pub mod logs;
pub mod metrics;
pub mod models;
pub mod nodes;
pub mod openai;
pub mod proxy;
pub mod users;

use crate::cloud_metrics;
use crate::AppState;
use axum::{
    body::Body,
    extract::Path as AxumPath,
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use include_dir::{include_dir, Dir, File};
use mime_guess::MimeGuess;

static DASHBOARD_ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/web/static");
const DASHBOARD_INDEX: &str = "index.html";
const CHAT_INDEX: &str = "chat/index.html";

/// APIルーターを作成
pub fn create_router(state: AppState) -> Router {
    // JWT認証が必要な保護されたルート
    let protected_routes = Router::new()
        .route("/api/auth/me", get(auth::me))
        .route(
            "/api/users",
            get(users::list_users).post(users::create_user),
        )
        .route(
            "/api/users/:id",
            put(users::update_user).delete(users::delete_user),
        )
        .route(
            "/api/api-keys",
            get(api_keys::list_api_keys).post(api_keys::create_api_key),
        )
        .route(
            "/api/api-keys/:id",
            put(api_keys::update_api_key).delete(api_keys::delete_api_key),
        )
        .layer(middleware::from_fn_with_state(
            state.jwt_secret.clone(),
            crate::auth::middleware::jwt_auth_middleware,
        ));

    // エージェントトークン認証が必要なルート
    let agent_protected_routes = Router::new()
        .route("/api/health", post(health::health_check))
        .layer(middleware::from_fn_with_state(
            state.db_pool.clone(),
            crate::auth::middleware::agent_token_auth_middleware,
        ));

    // APIキー認証が必要なルート（OpenAI互換エンドポイント）
    let api_key_protected_routes = Router::new()
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/completions", post(openai::completions))
        .route("/v1/embeddings", post(openai::embeddings))
        .route("/v1/models", get(openai::list_models))
        .route("/v1/models/:model_id", get(openai::get_model))
        .layer(middleware::from_fn_with_state(
            state.db_pool.clone(),
            crate::auth::middleware::api_key_auth_middleware,
        ));

    Router::new()
        // 認証エンドポイント（認証不要）
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        // 保護されたルート
        .merge(protected_routes)
        .merge(agent_protected_routes)
        .merge(api_key_protected_routes)
        // 既存のルート
        .route(
            "/api/nodes",
            post(nodes::register_node).get(nodes::list_nodes),
        )
        .route("/api/nodes/:node_id", delete(nodes::delete_node))
        .route(
            "/api/nodes/:node_id/disconnect",
            post(nodes::disconnect_node),
        )
        .route(
            "/api/nodes/:node_id/settings",
            put(nodes::update_node_settings),
        )
        .route("/api/nodes/:node_id/metrics", post(metrics::update_metrics))
        .route("/api/nodes/metrics", get(nodes::list_node_metrics))
        .route("/api/metrics/summary", get(nodes::metrics_summary))
        .route("/api/dashboard/nodes", get(dashboard::get_nodes))
        .route("/api/dashboard/stats", get(dashboard::get_stats))
        .route(
            "/api/dashboard/request-history",
            get(dashboard::get_request_history),
        )
        .route("/api/dashboard/overview", get(dashboard::get_overview))
        .route(
            "/api/dashboard/metrics/:node_id",
            get(dashboard::get_node_metrics),
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
        .route(
            "/api/dashboard/logs/coordinator",
            get(logs::get_coordinator_logs),
        )
        .route(
            "/api/dashboard/logs/nodes/:node_id",
            get(logs::get_node_logs),
        )
        // FR-002: node log proxy (spec path)
        .route("/api/nodes/:node_id/logs", get(logs::get_node_logs))
        .route("/api/chat", post(proxy::proxy_chat))
        .route("/api/generate", post(proxy::proxy_generate))
        // モデル管理API (SPEC-8ae67d67)
        .route("/api/models/available", get(models::get_available_models))
        .route("/api/models/register", post(models::register_model))
        .route("/api/models/pull", post(models::pull_model_from_hf))
        .route("/api/models/loaded", get(models::get_loaded_models))
        .route("/api/models/distribute", post(models::distribute_models))
        .route("/api/models/download", post(models::distribute_models))
        .route("/api/nodes/:node_id/models", get(models::get_node_models))
        .route(
            "/api/nodes/:node_id/models/pull",
            post(models::pull_model_to_node),
        )
        .route("/api/tasks", get(models::list_tasks))
        .route("/api/tasks/:task_id", get(models::get_task_progress))
        .route(
            "/api/tasks/:task_id/progress",
            post(models::update_progress),
        )
        .route("/metrics/cloud", get(cloud_metrics::export_metrics))
        .route("/dashboard", get(serve_dashboard_index))
        .route("/dashboard/", get(serve_dashboard_index))
        .route("/dashboard/*path", get(serve_dashboard_asset))
        // チャットUI（正式）
        .route("/chat", get(serve_chat_index))
        .route("/chat/", get(serve_chat_index))
        .route("/chat/*path", get(serve_chat_asset))
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

async fn serve_chat_index() -> Response {
    embedded_dashboard_response(CHAT_INDEX)
}

async fn serve_chat_asset(AxumPath(request_path): AxumPath<String>) -> Response {
    let normalized = normalize_chat_path(&request_path);
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

fn normalize_chat_path(request_path: &str) -> Option<String> {
    let trimmed = request_path.trim_matches('/');
    if trimmed.is_empty() {
        return Some(CHAT_INDEX.to_string());
    }
    if trimmed.contains("..") || trimmed.contains('\\') {
        return None;
    }
    Some(format!("chat/{}", trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate},
        registry::NodeRegistry,
        tasks::DownloadTaskManager,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use tower::Service;

    async fn test_state() -> (AppState, NodeRegistry) {
        let registry = NodeRegistry::new();
        let load_manager = LoadManager::new(registry.clone());
        let request_history =
            std::sync::Arc::new(crate::db::request_history::RequestHistoryStorage::new().unwrap());
        let task_manager = DownloadTaskManager::new();
        let db_pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to create test database");
        sqlx::migrate!("./migrations")
            .run(&db_pool)
            .await
            .expect("Failed to run migrations");
        let jwt_secret = "test-secret".to_string();
        let state = AppState {
            registry: registry.clone(),
            load_manager,
            request_history,
            task_manager,
            db_pool,
            jwt_secret,
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
        let (state, _) = test_state().await;
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
    async fn test_chat_static_served() {
        let (state, _) = test_state().await;
        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/chat")
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
    async fn test_dashboard_nodes_endpoint_returns_json() {
        let (state, registry) = test_state().await;
        registry
            .register(RegisterRequest {
                machine_name: "test-node".into(),
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
                    .uri("/api/dashboard/nodes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let nodes: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(nodes.is_array());
        assert_eq!(nodes.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_dashboard_overview_endpoint_returns_all_sections() {
        let (state, registry) = test_state().await;
        registry
            .register(RegisterRequest {
                machine_name: "overview-node".into(),
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
        assert!(overview["nodes"].is_array());
        assert!(overview["stats"].is_object());
        assert!(overview["history"].is_array());
        assert!(overview["generated_at"].is_string());
        assert!(overview["generation_time_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_dashboard_metrics_endpoint_returns_history() {
        let (state, registry) = test_state().await;
        let node_id = registry
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
            .node_id;

        state
            .load_manager
            .record_metrics(MetricsUpdate {
                node_id,
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
                initializing: false,
                ready_models: None,
            })
            .await
            .unwrap();

        let mut router = create_router(state);
        let response = router
            .call(
                Request::builder()
                    .uri(format!("/api/dashboard/metrics/{node_id}"))
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
            metrics.as_array().unwrap()[0]["node_id"].as_str().unwrap(),
            node_id.to_string()
        );
    }
}
