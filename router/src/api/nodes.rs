//! ノード登録APIハンドラー

use crate::registry::models::{ensure_router_model_cached, router_model_path};
use crate::{
    balancer::{AgentLoadSnapshot, SystemSummary},
    registry::NodeSettingsUpdate,
    AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use llm_router_common::{
    error::RouterError,
    protocol::{RegisterRequest, RegisterResponse},
    types::Node,
};
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info};

/// POST /api/nodes - ノード登録
pub async fn register_node(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    info!(
        "Node registration request: machine={}, ip={}, gpu_available={}",
        req.machine_name, req.ip_address, req.gpu_available
    );

    // GPU必須要件の検証（詳細なエラーメッセージ）
    if !req.gpu_available {
        error!(
            "Node registration rejected: GPU not available (machine={})",
            req.machine_name
        );
        return Err(AppError(RouterError::Common(
            llm_router_common::error::CommonError::Validation(
                "GPU hardware is required for agent registration. gpu_available must be true."
                    .to_string(),
            ),
        )));
    }

    if req.gpu_devices.is_empty() {
        error!(
            "Node registration rejected: No GPU devices (machine={})",
            req.machine_name
        );
        return Err(AppError(RouterError::Common(
            llm_router_common::error::CommonError::Validation(
                "GPU hardware is required for agent registration. No GPU devices detected in gpu_devices array."
                    .to_string(),
            ),
        )));
    }

    if !req.gpu_devices.iter().all(|device| device.is_valid()) {
        error!(
            "Node registration rejected: Invalid GPU device info (machine={})",
            req.machine_name
        );
        return Err(AppError(RouterError::Common(
            llm_router_common::error::CommonError::Validation(
                "GPU hardware is required for agent registration. Invalid GPU device information (empty model or zero count)."
                    .to_string(),
            ),
        )));
    }

    let mut req = req;

    if req.gpu_count.is_none() {
        let total_count = req.gpu_devices.iter().map(|device| device.count).sum();
        req.gpu_count = Some(total_count);
    }

    if req.gpu_model.is_none() {
        req.gpu_model = req.gpu_devices.first().map(|device| device.model.clone());
    }

    // GPUメモリ情報からモデルを選択（reqを移動する前に取得）
    // GPUメモリに応じた簡易選択（仕様上は gpt-oss:20b をデフォルト、超大容量のみ120b）
    let gpu_memory_bytes = req
        .gpu_devices
        .iter()
        .filter_map(|d| d.memory)
        .max()
        .unwrap_or(16_000_000_000); // デフォルトは16GB想定

    let model_name = if gpu_memory_bytes >= 80_000_000_000 {
        "gpt-oss:120b".to_string()
    } else {
        "gpt-oss:20b".to_string()
    };

    info!(
        "Selected model for auto-distribution: model={}, gpu_memory_gb={:.2}",
        model_name,
        gpu_memory_bytes as f64 / 1_000_000_000.0
    );

    // ヘルスチェックはノードのOpenAI互換API経由で実施
    let node_api_port = req.runtime_port + 1;
    let node_api_base = format!("http://{}:{}", req.ip_address, node_api_port);
    let health_url = format!("{}/v1/models", node_api_base);

    let skip_health_check = cfg!(test) || std::env::var("LLM_ROUTER_SKIP_HEALTH_CHECK").is_ok();
    let (loaded_models, initializing, ready_models) = if skip_health_check {
        (vec!["gpt-oss:20b".to_string()], false, Some((1, 1)))
    } else {
        let health_res = state.http_client.get(&health_url).send().await;
        if let Err(e) = health_res {
            error!(
                "Node registration rejected: node API health check failed at {} ({})",
                health_url, e
            );
            return Err(AppError(RouterError::Internal(format!(
                "Node API not reachable at {}: {}",
                health_url, e
            ))));
        }
        let resp = health_res.unwrap();
        if !resp.status().is_success() {
            error!(
                "Node registration rejected: node API returned HTTP {} at {}",
                resp.status(),
                health_url
            );
            return Err(AppError(RouterError::Internal(format!(
                "Node API health check failed with HTTP {}",
                resp.status()
            ))));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError(RouterError::Internal(e.to_string())))?;

        let models: Vec<String> = json
            .get("data")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        m.get("id")
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();

        let models_count = models.len().min(u8::MAX as usize) as u8;

        let ready_models = json
            .get("ready_models")
            .and_then(|v| {
                v.as_array().and_then(|arr| {
                    if arr.len() == 2 {
                        let a = arr[0].as_u64().unwrap_or(0) as u8;
                        let b = arr[1].as_u64().unwrap_or(0) as u8;
                        Some((a, b))
                    } else {
                        None
                    }
                })
            })
            .or(if models_count > 0 {
                Some((models_count, models_count))
            } else {
                None
            });

        let initializing = json
            .get("initializing")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| {
                ready_models
                    .map(|(ready, total)| ready < total)
                    .unwrap_or(false)
            });

        (models, initializing, ready_models)
    };

    // ヘルスチェックOKなら登録を実施
    let mut response = state.registry.register(req).await?;
    response.agent_api_port = Some(node_api_port);

    // エージェントトークンを生成（更新時は既存トークンを削除して再生成）
    if response.status == llm_router_common::protocol::RegisterStatus::Updated {
        // 既存トークンを削除
        let _ = crate::db::agent_tokens::delete(&state.db_pool, response.node_id).await;
    }
    let agent_token_with_plaintext =
        crate::db::agent_tokens::create(&state.db_pool, response.node_id)
            .await
            .map_err(|e| {
                error!("Failed to create agent token: {}", e);
                AppError(RouterError::Internal(format!(
                    "Failed to create agent token: {}",
                    e
                )))
            })?;
    response.agent_token = Some(agent_token_with_plaintext.token);

    // 取得した初期状態を反映
    let _ = state
        .registry
        .update_last_seen(
            response.node_id,
            Some(loaded_models),
            None,
            None,
            None,
            Some(initializing),
            ready_models,
        )
        .await;

    state
        .load_manager
        .upsert_initial_state(response.node_id, initializing, ready_models)
        .await;

    // HTTPステータスコードを決定（新規登録=201, 更新=200）
    let status_code = match response.status {
        llm_router_common::protocol::RegisterStatus::Registered => StatusCode::CREATED,
        llm_router_common::protocol::RegisterStatus::Updated => StatusCode::OK,
    };

    // ノード登録成功後、ルーターがサポートする全モデルを自動配布
    // テストモードではスキップ
    if skip_health_check {
        info!("Auto-distribution skipped in test mode");
        return Ok((status_code, Json(response)));
    }

    let node_id = response.node_id;
    let task_manager = state.task_manager.clone();
    let registry = state.registry.clone();
    let client = crate::runtime::RuntimeClient::new()?;
    let supported_models = client.get_predefined_models();

    let mut created_tasks = Vec::new();

    for model in supported_models {
        let task = task_manager.create_task(node_id, model.name.clone()).await;
        let task_id = task.id;
        created_tasks.push((model.name.clone(), task_id));

        let cached = ensure_router_model_cached(&model).await;
        let shared_path = cached
            .or_else(|| router_model_path(&model.name))
            .map(|p| p.to_string_lossy().to_string());
        let download_url = model.download_url.clone();

        info!(
            "Auto-distribution started: node_id={}, model={}, task_id={}",
            node_id, model.name, task_id
        );

        // ノードにモデルプル要求を送信（バックグラウンド）
        let registry = registry.clone();
        let http_client = state.http_client.clone();
        tokio::spawn(async move {
            match registry.get(node_id).await {
                Ok(node) => {
                    // ノードAPIのポート（デフォルト: LLM runtime port + 1）
                    let node_api_port = node.agent_api_port.unwrap_or(node.runtime_port + 1);
                    let node_url = format!("http://{}:{}/pull", node.ip_address, node_api_port);

                    info!("Sending pull request to node: {}", node_url);

                    let pull_request = serde_json::json!({
                        "model": model.name,
                        "task_id": task_id,
                        "path": shared_path,
                        "download_url": download_url,
                    });

                    match http_client.post(&node_url).json(&pull_request).send().await {
                        Ok(response) => {
                            if response.status().is_success() {
                                info!("Successfully sent pull request to node {}", node_id);
                            } else {
                                error!(
                                    "Node {} returned error status: {}",
                                    node_id,
                                    response.status()
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to send pull request to node {}: {}", node_id, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to get node {} info: {}", node_id, e);
                }
            }
        });
    }

    // レスポンスには先頭のタスク情報だけ添える（後方互換のため）
    if let Some((first_model, first_task)) = created_tasks.first() {
        response.auto_distributed_model = Some(first_model.clone());
        response.download_task_id = Some(*first_task);
    }

    Ok((status_code, Json(response)))
}

/// GET /api/nodes - ノード一覧取得
pub async fn list_nodes(State(state): State<AppState>) -> Json<Vec<Node>> {
    let nodes = state.registry.list().await;
    Json(nodes)
}

/// PUT /api/nodes/:id/settings - ノード設定更新
pub async fn update_node_settings(
    State(state): State<AppState>,
    axum::extract::Path(node_id): axum::extract::Path<uuid::Uuid>,
    Json(payload): Json<UpdateNodeSettingsPayload>,
) -> Result<Json<Node>, AppError> {
    let update = NodeSettingsUpdate {
        custom_name: payload.custom_name,
        tags: payload.tags,
        notes: payload.notes,
    };

    let node = state.registry.update_settings(node_id, update).await?;

    Ok(Json(node))
}

/// ノード設定更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateNodeSettingsPayload {
    /// 表示名（nullでリセット）
    #[serde(default)]
    pub custom_name: Option<Option<String>>,
    /// タグ一覧
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// メモ（nullでリセット）
    #[serde(default)]
    pub notes: Option<Option<String>>,
}

/// GET /api/nodes/metrics - ノードメトリクス取得
pub async fn list_node_metrics(State(state): State<AppState>) -> Json<Vec<AgentLoadSnapshot>> {
    let snapshots = state.load_manager.snapshots().await;
    Json(snapshots)
}

/// GET /api/metrics/summary - システム統計
pub async fn metrics_summary(State(state): State<AppState>) -> Json<SystemSummary> {
    let summary = state.load_manager.summary().await;
    Json(summary)
}

/// DELETE /api/nodes/:id - ノードを削除
pub async fn delete_node(
    State(state): State<AppState>,
    axum::extract::Path(node_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, AppError> {
    state.registry.delete(node_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/nodes/:id/disconnect - ノードを強制オフラインにする
pub async fn disconnect_node(
    State(state): State<AppState>,
    axum::extract::Path(node_id): axum::extract::Path<uuid::Uuid>,
) -> Result<StatusCode, AppError> {
    state.registry.mark_offline(node_id).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Axum用のエラーレスポンス型
#[derive(Debug)]
pub struct AppError(RouterError);

impl From<RouterError> for AppError {
    fn from(err: RouterError) -> Self {
        AppError(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            RouterError::AgentNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            RouterError::NoAgentsAvailable => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            RouterError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            RouterError::AgentOffline(_) => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            RouterError::InvalidModelName(_) => (StatusCode::BAD_REQUEST, self.0.to_string()),
            RouterError::InsufficientStorage(_) => {
                (StatusCode::INSUFFICIENT_STORAGE, self.0.to_string())
            }
            RouterError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Http(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            RouterError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            RouterError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::PasswordHash(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Jwt(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            RouterError::Authentication(_) => (StatusCode::UNAUTHORIZED, self.0.to_string()),
            RouterError::Authorization(_) => (StatusCode::FORBIDDEN, self.0.to_string()),
            RouterError::Common(err) => {
                // GPU必須エラーの場合は403 Forbiddenを返す
                let message = err.to_string();
                if message.contains("GPU is required")
                    || message.contains("GPU hardware is required")
                {
                    (StatusCode::FORBIDDEN, self.0.to_string())
                } else {
                    (StatusCode::BAD_REQUEST, self.0.to_string())
                }
            }
        };

        let payload = json!({
            "error": message
        });

        (status, Json(payload)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        balancer::{LoadManager, MetricsUpdate, RequestOutcome},
        registry::NodeRegistry,
        tasks::DownloadTaskManager,
    };
    use axum::body::to_bytes;
    use llm_router_common::{
        protocol::RegisterStatus,
        types::{GpuDeviceInfo, NodeStatus},
    };
    use std::net::IpAddr;
    use std::time::Duration;

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
            http_client: reqwest::Client::new(),
        }
    }

    fn sample_gpu_devices() -> Vec<GpuDeviceInfo> {
        vec![GpuDeviceInfo {
            model: "Test GPU".to_string(),
            count: 1,
            memory: None,
        }]
    }

    #[tokio::test]
    async fn test_register_node_success() {
        let state = create_test_state().await;
        let req = RegisterRequest {
            machine_name: "test-machine".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let result = register_node(State(state), Json(req)).await;
        assert!(result.is_ok());

        let response = result.unwrap().1 .0;
        assert!(!response.node_id.to_string().is_empty());
    }

    #[tokio::test]
    async fn test_list_nodes_empty() {
        let state = create_test_state().await;
        let result = list_nodes(State(state)).await;
        assert_eq!(result.0.len(), 0);
    }

    #[tokio::test]
    async fn test_list_nodes_with_nodes() {
        let state = create_test_state().await;

        // ノードを2つ登録
        let req1 = RegisterRequest {
            machine_name: "machine1".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let _ = register_node(State(state.clone()), Json(req1))
            .await
            .unwrap();

        let req2 = RegisterRequest {
            machine_name: "machine2".to_string(),
            ip_address: "192.168.1.101".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let _ = register_node(State(state.clone()), Json(req2))
            .await
            .unwrap();

        let result = list_nodes(State(state)).await;
        assert_eq!(result.0.len(), 2);
    }

    #[tokio::test]
    async fn test_register_node_gpu_required_error_is_json() {
        let state = create_test_state().await;
        let req = RegisterRequest {
            machine_name: "gpu-required-test".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: false,
            gpu_devices: Vec::new(),
            gpu_count: None,
            gpu_model: None,
        };

        let response = register_node(State(state), Json(req))
            .await
            .unwrap_err()
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let expected = "Validation error: GPU hardware is required for agent registration. gpu_available must be true.";
        assert_eq!(body["error"], expected);
    }

    #[tokio::test]
    async fn test_register_node_missing_gpu_devices_rejected() {
        let state = create_test_state().await;
        let req = RegisterRequest {
            machine_name: "missing-gpu-devices".to_string(),
            ip_address: "192.168.1.102".parse().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: Vec::new(),
            gpu_count: None,
            gpu_model: None,
        };

        let response = register_node(State(state), Json(req))
            .await
            .unwrap_err()
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["error"],
            "Validation error: GPU hardware is required for agent registration. No GPU devices detected in gpu_devices array."
        );
    }

    #[tokio::test]
    async fn test_register_same_machine_different_port_creates_multiple_nodes() {
        let state = create_test_state().await;

        let req1 = RegisterRequest {
            machine_name: "shared-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let res1 = register_node(State(state.clone()), Json(req1))
            .await
            .unwrap()
            .1
             .0;
        assert_eq!(res1.status, RegisterStatus::Registered);

        let req2 = RegisterRequest {
            machine_name: "shared-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 12434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let res2 = register_node(State(state.clone()), Json(req2))
            .await
            .unwrap()
            .1
             .0;
        assert_eq!(res2.status, RegisterStatus::Registered);

        let nodes = list_nodes(State(state)).await.0;
        assert_eq!(nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_list_node_metrics_returns_snapshot() {
        let state = create_test_state().await;

        // ノードを登録
        let req = RegisterRequest {
            machine_name: "metrics-machine".to_string(),
            ip_address: "192.168.1.150".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };

        let response = register_node(State(state.clone()), Json(req))
            .await
            .unwrap()
            .1
             .0;

        // メトリクスを記録
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                node_id: response.node_id,
                cpu_usage: 42.0,
                memory_usage: 33.0,
                gpu_usage: Some(55.0),
                gpu_memory_usage: Some(48.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 1,
                average_response_time_ms: None,
                initializing: false,
                ready_models: None,
            })
            .await
            .unwrap();

        let metrics = list_node_metrics(State(state)).await;
        assert_eq!(metrics.0.len(), 1);

        let snapshot = &metrics.0[0];
        assert_eq!(snapshot.node_id, response.node_id);
        assert_eq!(snapshot.cpu_usage.unwrap(), 42.0);
        assert_eq!(snapshot.memory_usage.unwrap(), 33.0);
        assert_eq!(snapshot.gpu_usage, Some(55.0));
        assert_eq!(snapshot.gpu_memory_usage, Some(48.0));
        assert_eq!(snapshot.active_requests, 1);
        assert!(!snapshot.is_stale);
    }

    #[tokio::test]
    async fn test_metrics_summary_empty() {
        let state = create_test_state().await;
        let summary = metrics_summary(State(state)).await;
        assert_eq!(summary.total_agents, 0);
        assert_eq!(summary.online_agents, 0);
        assert_eq!(summary.total_requests, 0);
        assert_eq!(summary.total_active_requests, 0);
        assert!(summary.average_response_time_ms.is_none());
        assert!(summary.last_metrics_updated_at.is_none());
    }

    #[tokio::test]
    async fn test_metrics_summary_counts_requests() {
        let state = create_test_state().await;

        // ノードを登録
        let register_req = RegisterRequest {
            machine_name: "stats-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: true,
            gpu_devices: sample_gpu_devices(),
            gpu_count: Some(1),
            gpu_model: Some("Test GPU".to_string()),
        };
        let response = register_node(State(state.clone()), Json(register_req))
            .await
            .unwrap()
            .1
             .0;

        // ハートビートでメトリクス更新
        state
            .load_manager
            .record_metrics(MetricsUpdate {
                node_id: response.node_id,
                cpu_usage: 55.0,
                memory_usage: 44.0,
                gpu_usage: Some(60.0),
                gpu_memory_usage: Some(62.0),
                gpu_memory_total_mb: None,
                gpu_memory_used_mb: None,
                gpu_temperature: None,
                gpu_model_name: None,
                gpu_compute_capability: None,
                gpu_capability_score: None,
                active_requests: 2,
                average_response_time_ms: Some(150.0),
                initializing: false,
                ready_models: None,
            })
            .await
            .unwrap();

        // リクエストを成功・失敗で記録
        state
            .load_manager
            .begin_request(response.node_id)
            .await
            .unwrap();
        state
            .load_manager
            .finish_request(
                response.node_id,
                RequestOutcome::Success,
                Duration::from_millis(120),
            )
            .await
            .unwrap();

        state
            .load_manager
            .begin_request(response.node_id)
            .await
            .unwrap();
        state
            .load_manager
            .finish_request(
                response.node_id,
                RequestOutcome::Error,
                Duration::from_millis(200),
            )
            .await
            .unwrap();

        let summary = metrics_summary(State(state)).await;
        assert_eq!(summary.total_agents, 1);
        assert_eq!(summary.online_agents, 1);
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.successful_requests, 1);
        assert_eq!(summary.failed_requests, 1);
        assert_eq!(summary.total_active_requests, 2);
        let avg = summary.average_response_time_ms.unwrap();
        assert!((avg - 160.0).abs() < 0.1);
        assert!(summary.last_metrics_updated_at.is_some());
    }

    #[tokio::test]
    async fn test_update_node_settings_endpoint() {
        let state = create_test_state().await;

        let node_id = register_node(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "node-settings".into(),
                ip_address: "10.0.0.5".parse().unwrap(),
                runtime_version: "0.1.0".into(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            }),
        )
        .await
        .unwrap()
        .1
         .0
        .node_id;

        let payload = UpdateNodeSettingsPayload {
            custom_name: Some(Some("Primary".into())),
            tags: Some(vec!["dallas".into(), "gpu".into()]),
            notes: Some(Some("Keep online".into())),
        };

        let node = update_node_settings(
            State(state.clone()),
            axum::extract::Path(node_id),
            Json(payload),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(node.custom_name.as_deref(), Some("Primary"));
        assert_eq!(node.tags, vec!["dallas", "gpu"]);
        assert_eq!(node.notes.as_deref(), Some("Keep online"));
    }

    #[tokio::test]
    async fn test_delete_node_endpoint() {
        let state = create_test_state().await;
        let response = register_node(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "delete-node".into(),
                ip_address: "10.0.0.7".parse().unwrap(),
                runtime_version: "0.1.0".into(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            }),
        )
        .await
        .unwrap()
        .1
         .0;

        let status = delete_node(State(state.clone()), axum::extract::Path(response.node_id))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NO_CONTENT);

        let nodes = list_nodes(State(state)).await.0;
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn test_disconnect_node_endpoint_marks_offline() {
        let state = create_test_state().await;
        let node_id = register_node(
            State(state.clone()),
            Json(RegisterRequest {
                machine_name: "disconnect-node".into(),
                ip_address: "10.0.0.8".parse().unwrap(),
                runtime_version: "0.1.0".into(),
                runtime_port: 11434,
                gpu_available: true,
                gpu_devices: sample_gpu_devices(),
                gpu_count: Some(1),
                gpu_model: Some("Test GPU".to_string()),
            }),
        )
        .await
        .unwrap()
        .1
         .0
        .node_id;

        let status = disconnect_node(State(state.clone()), axum::extract::Path(node_id))
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);

        let node = state.registry.get(node_id).await.unwrap();
        assert_eq!(node.status, NodeStatus::Offline);
    }

    #[tokio::test]
    async fn test_register_node_without_gpu_rejected() {
        let state = create_test_state().await;
        let req = RegisterRequest {
            machine_name: "no-gpu-machine".to_string(),
            ip_address: "192.168.1.200".parse::<IpAddr>().unwrap(),
            runtime_version: "0.1.0".to_string(),
            runtime_port: 11434,
            gpu_available: false,
            gpu_devices: Vec::new(),
            gpu_count: None,
            gpu_model: None,
        };

        let result = register_node(State(state), Json(req)).await;
        assert!(result.is_err());

        // エラーがValidationエラーであることを確認
        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);
        assert!(err_msg.contains("Validation") || err_msg.contains("GPU"));
    }
}
