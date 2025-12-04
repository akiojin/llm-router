//! ヘルスチェックAPIハンドラー

use crate::{api::nodes::AppError, balancer::MetricsUpdate, AppState};
use axum::{extract::State, Json};
use llm_router_common::protocol::HealthCheckRequest;

/// POST /api/health - ヘルスチェック受信
pub async fn health_check(
    State(state): State<AppState>,
    Json(req): Json<HealthCheckRequest>,
) -> Result<Json<()>, AppError> {
    // ノードの最終確認時刻を更新
    let models = if req.loaded_models.is_empty() {
        None
    } else {
        Some(req.loaded_models.clone())
    };
    state
        .registry
        .update_last_seen(
            req.node_id,
            models,
            req.gpu_model_name.clone(),
            req.gpu_compute_capability.clone(),
            req.gpu_capability_score,
            Some(req.initializing),
            req.ready_models,
        )
        .await?;

    // 最新メトリクスをロードマネージャーに記録
    state
        .load_manager
        .record_metrics(MetricsUpdate {
            node_id: req.node_id,
            cpu_usage: req.cpu_usage,
            memory_usage: req.memory_usage,
            gpu_usage: req.gpu_usage,
            gpu_memory_usage: req.gpu_memory_usage,
            gpu_memory_total_mb: req.gpu_memory_total_mb,
            gpu_memory_used_mb: req.gpu_memory_used_mb,
            gpu_temperature: req.gpu_temperature,
            gpu_model_name: req.gpu_model_name,
            gpu_compute_capability: req.gpu_compute_capability,
            gpu_capability_score: req.gpu_capability_score,
            active_requests: req.active_requests,
            average_response_time_ms: req.average_response_time_ms,
            initializing: req.initializing,
            ready_models: req.ready_models,
        })
        .await?;

    Ok(Json(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{balancer::LoadManager, registry::NodeRegistry, tasks::DownloadTaskManager};
    use llm_router_common::{protocol::RegisterRequest, types::GpuDeviceInfo};
    use std::net::IpAddr;
    use uuid::Uuid;

    async fn create_test_state() -> AppState {
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
        AppState {
            registry,
            load_manager,
            request_history,
            task_manager,
            db_pool,
            jwt_secret,
        }
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let state = create_test_state().await;

        // まずノードを登録
        let register_req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: vec![GpuDeviceInfo {
                model: "Test GPU".to_string(),
                count: 1,
                memory: None,
            }],
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let register_response = state.registry.register(register_req).await.unwrap();

        // ヘルスチェックを送信
        let health_req = HealthCheckRequest {
            node_id: register_response.node_id,
            cpu_usage: 45.5,
            memory_usage: 60.2,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 3,
            average_response_time_ms: Some(110.0),
            loaded_models: vec!["gpt-oss:20b".into()],
            initializing: false,
            ready_models: Some((1, 5)),
        };

        let result = health_check(State(state.clone()), Json(health_req)).await;
        assert!(result.is_ok());

        // ノードが更新されたことを確認
        let agent = state.registry.get(register_response.node_id).await.unwrap();
        assert_eq!(agent.status, llm_router_common::types::NodeStatus::Online);
        assert_eq!(agent.loaded_models, vec!["gpt-oss:20b"]);
    }

    #[tokio::test]
    async fn test_health_check_unknown_agent() {
        let state = create_test_state().await;

        let health_req = HealthCheckRequest {
            node_id: Uuid::new_v4(),
            cpu_usage: 45.5,
            memory_usage: 60.2,
            gpu_usage: None,
            gpu_memory_usage: None,
            gpu_memory_total_mb: None,
            gpu_memory_used_mb: None,
            gpu_temperature: None,
            gpu_model_name: None,
            gpu_compute_capability: None,
            gpu_capability_score: None,
            active_requests: 3,
            average_response_time_ms: None,
            loaded_models: Vec::new(),
            initializing: false,
            ready_models: None,
        };

        let result = health_check(State(state), Json(health_req)).await;
        assert!(result.is_err());
    }
}
