//! エンドポイント管理API
//!
//! SPEC-e8e9326e: llmlb主導エンドポイント登録システム

use super::error::AppError;
use crate::common::auth::{Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::common::http::RequestBuilderBearerExt;
use crate::db::endpoints as db;
use crate::detection::{detect_endpoint_type_with_client, DetectionError};
use crate::sync::{self, SyncError};
use crate::system_info;
use crate::types::endpoint::{DeviceInfo, Endpoint, EndpointStatus, EndpointType};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use reqwest::Url;
use uuid::Uuid;

mod dto;
pub use dto::*;
mod proxy;
pub use proxy::proxy_chat_completions;
mod downloads;
pub use downloads::*;

/// エンドポイント固有の情報取得方法でデバイス情報を取得（SPEC-f8e3a1b7, SPEC-e8e9326e）
///
/// エンドポイント登録時に呼び出し、デバイス情報を取得する。
/// エンドポイントタイプに応じて最適な取得方法を使用する：
/// - xLLM/Ollama: GET /api/system
/// - llama.cpp: GET /slots (primary) → GET /metrics (fallback)
/// - vLLM/OpenAI互換: サポートなし
///
/// 応答がない場合（タイムアウト、404等）はNoneを返す。
async fn fetch_system_info(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    endpoint_type: &EndpointType,
) -> Option<DeviceInfo> {
    system_info::get_endpoint_system_info(client, base_url, api_key, endpoint_type).await
}

/// Admin権限を確認
fn ensure_admin(claims: &Claims) -> Result<(), AppError> {
    if claims.role != UserRole::Admin {
        return Err(AppError(LbError::Authorization(
            "Admin permission required".to_string(),
        )));
    }
    Ok(())
}

/// 接続テスト実行（DB/キャッシュの更新を含む）
async fn run_connection_test(state: &AppState, endpoint: &Endpoint) -> TestConnectionResponse {
    // GET /v1/models でヘルスチェック
    let url = format!("{}/v1/models", endpoint.base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();

    let mut request = state.http_client.get(&url);
    request = request.bearer_opt(endpoint.api_key.as_ref());

    let result = request
        .timeout(std::time::Duration::from_secs(
            endpoint.inference_timeout_secs as u64,
        ))
        .send()
        .await;

    let latency_ms = start.elapsed().as_millis() as u32;

    match result {
        Ok(response) => {
            if response.status().is_success() {
                // モデル一覧を取得
                let models_found: Option<Vec<String>> = match response
                    .json::<serde_json::Value>()
                    .await
                {
                    Ok(json) => json["data"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m["id"].as_str().map(String::from))
                                .collect()
                        })
                        .or_else(|| {
                            json["models"].as_array().map(|arr| {
                                arr.iter()
                                    .filter_map(|m| {
                                        m["name"].as_str().or(m["model"].as_str()).map(String::from)
                                    })
                                    .collect()
                            })
                        }),
                    Err(_) => None,
                };

                // ステータスを更新（DB + キャッシュ）
                let _ = state
                    .endpoint_registry
                    .update_status(endpoint.id, EndpointStatus::Online, Some(latency_ms), None)
                    .await;

                // モデル数を計算
                let model_count = models_found.as_ref().map(|m| m.len()).unwrap_or(0);

                TestConnectionResponse {
                    success: true,
                    latency_ms: Some(latency_ms),
                    error: None,
                    models_found,
                    endpoint_info: Some(EndpointTestInfo { model_count }),
                }
            } else {
                let error_msg = format!("HTTP {}", response.status());
                let _ = state
                    .endpoint_registry
                    .update_status(endpoint.id, EndpointStatus::Error, None, Some(&error_msg))
                    .await;

                TestConnectionResponse {
                    success: false,
                    latency_ms: Some(latency_ms),
                    error: Some(error_msg),
                    models_found: None,
                    endpoint_info: None,
                }
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            let _ = state
                .endpoint_registry
                .update_status(endpoint.id, EndpointStatus::Error, None, Some(&error_msg))
                .await;

            TestConnectionResponse {
                success: false,
                latency_ms: None,
                error: Some(error_msg),
                models_found: None,
                endpoint_info: None,
            }
        }
    }
}

// --- Handlers ---

/// POST /api/endpoints - エンドポイント登録
pub async fn create_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Json(req): Json<CreateEndpointRequest>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // バリデーション
    if req.name.trim().is_empty() {
        return AppError(LbError::Common(CommonError::Validation(
            "Name is required".to_string(),
        )))
        .into_response();
    }

    if req.base_url.trim().is_empty() {
        return AppError(LbError::Common(CommonError::Validation(
            "Base URL is required".to_string(),
        )))
        .into_response();
    }

    // URL形式チェック
    if Url::parse(&req.base_url).is_err() {
        return AppError(LbError::Common(CommonError::Validation(
            "Invalid URL format".to_string(),
        )))
        .into_response();
    }

    // ヘルスチェック間隔のバリデーション（10-300秒）
    if req.health_check_interval_secs < 10 || req.health_check_interval_secs > 300 {
        return AppError(LbError::Common(CommonError::Validation(
            "Health check interval must be between 10 and 300 seconds".to_string(),
        )))
        .into_response();
    }

    // 名前の重複チェック
    match db::find_by_name(&state.db_pool, &req.name).await {
        Ok(Some(_)) => {
            return AppError(LbError::Common(CommonError::Validation(format!(
                "Endpoint with name '{}' already exists",
                req.name
            ))))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to check name uniqueness: {}", e);
            return AppError(LbError::Database(
                "Failed to check name uniqueness".to_string(),
            ))
            .into_response();
        }
        Ok(None) => {} // OK - 名前は一意
    }

    // SPEC-e8e9326e: 自動検出（手動指定は廃止、対応タイプのみ許可）
    let detection_result =
        detect_endpoint_type_with_client(&state.http_client, &req.base_url, req.api_key.as_deref())
            .await;

    let detected_type = match detection_result {
        Ok(result) => result.endpoint_type,
        Err(DetectionError::Unreachable(msg)) => {
            return AppError(LbError::Http(format!("Endpoint unreachable: {}", msg)))
                .into_response();
        }
        Err(DetectionError::UnsupportedType(msg)) => {
            return AppError(LbError::Common(CommonError::Validation(format!(
                "Unsupported endpoint type: {}",
                msg
            ))))
            .into_response();
        }
    };

    let mut endpoint = Endpoint::new(req.name, req.base_url.clone(), detected_type);
    endpoint.api_key = req.api_key.clone();
    endpoint.health_check_interval_secs = req.health_check_interval_secs;
    if let Some(timeout) = req.inference_timeout_secs {
        endpoint.inference_timeout_secs = timeout;
    }
    endpoint.notes = req.notes;
    if !req.capabilities.is_empty() {
        endpoint.capabilities = req.capabilities;
    }

    match db::create_endpoint(&state.db_pool, &endpoint).await {
        Ok(()) => {
            // EndpointRegistryキャッシュも更新（DBは既に保存済みなのでキャッシュのみ）
            state.endpoint_registry.add_to_cache(endpoint.clone()).await;

            // arch-review [L2]: 登録直後のバックグラウンドタスク（デバイス情報取得・
            // 接続チェック＋モデル同期）を関数へ抽出し、単体で検証しやすくする。
            spawn_post_registration_tasks(&state, &endpoint);

            (StatusCode::CREATED, Json(EndpointResponse::from(endpoint))).into_response()
        }
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("UNIQUE constraint failed") {
                AppError(LbError::Conflict(
                    "Endpoint with this name or URL already exists".to_string(),
                ))
                .into_response()
            } else {
                tracing::error!("Failed to create endpoint: {}", e);
                AppError(LbError::Database("Failed to create endpoint".to_string())).into_response()
            }
        }
    }
}

/// エンドポイント登録直後のバックグラウンドタスクを起動する。
///
/// arch-review [L2]: create_endpoint 内に inline 展開されていた 2 つの fire-and-forget
/// spawn を関数へ抽出した。デバイス情報取得と接続チェック＋モデル同期は互いに独立で、
/// レスポンスをブロックしないよう並行に spawn する。
fn spawn_post_registration_tasks(state: &AppState, endpoint: &Endpoint) {
    // SPEC-f8e3a1b7, SPEC-e8e9326e: エンドポイント固有の方法でデバイス情報を取得
    let endpoint_id = endpoint.id;
    let base_url = endpoint.base_url.clone();
    let api_key = endpoint.api_key.clone();
    let endpoint_type = endpoint.endpoint_type;
    let registry = state.endpoint_registry.clone();
    let http_client = state.http_client.clone();

    // Fire-and-forget: デバイス情報取得は非同期で行う（レスポンスをブロックしない）
    tokio::spawn(async move {
        if let Some(device_info) =
            fetch_system_info(&http_client, &base_url, api_key.as_deref(), &endpoint_type).await
        {
            tracing::info!(
                endpoint_id = %endpoint_id,
                device_type = ?device_info.device_type,
                gpu_count = device_info.gpu_devices.len(),
                endpoint_type = ?endpoint_type,
                "Retrieved device info via endpoint-specific method"
            );
            if let Err(e) = registry
                .update_device_info(endpoint_id, Some(device_info))
                .await
            {
                tracing::warn!(
                    endpoint_id = %endpoint_id,
                    error = %e,
                    "Failed to save device info"
                );
            }
        }
    });

    // 登録直後に接続チェック＆モデル同期（バックグラウンド実行）
    let state_clone = state.clone();
    let endpoint_clone = endpoint.clone();
    tokio::spawn(async move {
        let test_result = run_connection_test(&state_clone, &endpoint_clone).await;
        if !test_result.success {
            tracing::warn!(
                endpoint_id = %endpoint_clone.id,
                endpoint_name = %endpoint_clone.name,
                error = ?test_result.error,
                "Auto connection test failed"
            );
            return;
        }

        match sync::sync_models_with_type(
            &state_clone.db_pool,
            &state_clone.http_client,
            endpoint_clone.id,
            &endpoint_clone.base_url,
            endpoint_clone.api_key.as_deref(),
            endpoint_clone.inference_timeout_secs as u64,
            Some(endpoint_clone.endpoint_type),
        )
        .await
        {
            Ok(result) => {
                if let Err(e) = state_clone
                    .endpoint_registry
                    .refresh_model_mappings(endpoint_clone.id)
                    .await
                {
                    tracing::warn!(
                        endpoint_id = %endpoint_clone.id,
                        error = %e,
                        "Failed to refresh model mappings"
                    );
                }
                tracing::info!(
                    endpoint_id = %endpoint_clone.id,
                    added = result.added,
                    removed = result.removed,
                    updated = result.updated,
                    "Auto model sync completed"
                );
            }
            Err(e) => {
                tracing::warn!(
                    endpoint_id = %endpoint_clone.id,
                    error = %e,
                    "Auto model sync failed"
                );
            }
        }
    });
}

/// GET /api/endpoints - エンドポイント一覧
pub async fn list_endpoints(
    State(state): State<AppState>,
    Query(query): Query<ListEndpointsQuery>,
) -> impl IntoResponse {
    match db::list_endpoints(&state.db_pool).await {
        Ok(endpoints) => {
            // ステータスでフィルタ
            let mut filtered_endpoints: Vec<Endpoint> = if let Some(ref status) = query.status {
                endpoints
                    .into_iter()
                    .filter(|ep| ep.status.as_str() == status)
                    .collect()
            } else {
                endpoints
            };

            // SPEC-e8e9326e: タイプでフィルタ
            if let Some(ref endpoint_type) = query.endpoint_type {
                filtered_endpoints.retain(|ep| ep.endpoint_type.as_str() == endpoint_type);
            }

            let total = filtered_endpoints.len();
            let mut response_endpoints = Vec::with_capacity(total);

            for ep in filtered_endpoints {
                let ep_id = ep.id;
                let mut response = EndpointResponse::from(ep);

                // モデル数を取得
                if let Ok(models) = db::list_endpoint_models(&state.db_pool, ep_id).await {
                    response.model_count = Some(models.len());
                } else {
                    response.model_count = Some(0);
                }

                response_endpoints.push(response);
            }

            let response = ListEndpointsResponse {
                endpoints: response_endpoints,
                total,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list endpoints: {}", e);
            AppError(LbError::Database("Failed to list endpoints".to_string())).into_response()
        }
    }
}

/// GET /api/endpoints/:id - エンドポイント詳細
pub async fn get_endpoint(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(endpoint)) => {
            // モデル一覧も取得して詳細レスポンスに含める
            let models = match db::list_endpoint_models(&state.db_pool, id).await {
                Ok(m) => Some(m.into_iter().map(EndpointModelResponse::from).collect()),
                Err(_) => None,
            };
            let mut response = EndpointResponse::from(endpoint);
            response.models = models;
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint: {}", e);
            AppError(LbError::Database("Failed to get endpoint".to_string())).into_response()
        }
    }
}

/// PUT /api/endpoints/:id - エンドポイント更新
pub async fn update_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateEndpointRequest>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // 既存のエンドポイントを取得
    let existing = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for update: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    // 名前のバリデーション（空文字列は不許可）
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return AppError(LbError::Common(CommonError::Validation(
                "Name cannot be empty".to_string(),
            )))
            .into_response();
        }
    }

    // URL形式チェック
    if let Some(ref url) = req.base_url {
        if Url::parse(url).is_err() {
            return AppError(LbError::Common(CommonError::Validation(
                "Invalid URL format".to_string(),
            )))
            .into_response();
        }
    }

    // 名前変更時の重複チェック（他のエンドポイントと重複していないか）
    if let Some(ref new_name) = req.name {
        if new_name != &existing.name {
            match db::find_by_name(&state.db_pool, new_name).await {
                Ok(Some(_)) => {
                    return AppError(LbError::Common(CommonError::Validation(format!(
                        "Endpoint with name '{}' already exists",
                        new_name
                    ))))
                    .into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to check name uniqueness: {}", e);
                    return AppError(LbError::Database(
                        "Failed to check name uniqueness".to_string(),
                    ))
                    .into_response();
                }
                Ok(None) => {} // OK - 名前は一意
            }
        }
    }

    // 更新内容を適用
    let original_base_url = existing.base_url.clone();
    let mut updated = existing;
    if let Some(name) = req.name {
        updated.name = name;
    }
    if let Some(base_url) = req.base_url {
        updated.base_url = base_url;
    }
    if let Some(api_key) = req.api_key {
        updated.api_key = Some(api_key);
    }
    if let Some(interval) = req.health_check_interval_secs {
        updated.health_check_interval_secs = interval;
    }
    if let Some(timeout) = req.inference_timeout_secs {
        updated.inference_timeout_secs = timeout;
    }
    // notes: None=未指定(そのまま), Some(None)=削除, Some(Some(v))=設定
    if let Some(notes_value) = req.notes {
        updated.notes = notes_value;
    }

    // SPEC-e8e9326e: base_url変更時はタイプを再検出
    if updated.base_url != original_base_url {
        let detection_result = detect_endpoint_type_with_client(
            &state.http_client,
            &updated.base_url,
            updated.api_key.as_deref(),
        )
        .await;

        match detection_result {
            Ok(result) => {
                updated.endpoint_type = result.endpoint_type;
            }
            Err(DetectionError::Unreachable(msg)) => {
                return AppError(LbError::Http(format!("Endpoint unreachable: {}", msg)))
                    .into_response();
            }
            Err(DetectionError::UnsupportedType(msg)) => {
                return AppError(LbError::Common(CommonError::Validation(format!(
                    "Unsupported endpoint type: {}",
                    msg
                ))))
                .into_response();
            }
        }
    }

    match state.endpoint_registry.update(updated.clone()).await {
        Ok(true) => (StatusCode::OK, Json(EndpointResponse::from(updated))).into_response(),
        Ok(false) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("UNIQUE constraint failed") {
                AppError(LbError::Conflict(
                    "Endpoint with this name or URL already exists".to_string(),
                ))
                .into_response()
            } else {
                tracing::error!("Failed to update endpoint: {}", e);
                AppError(LbError::Database("Failed to update endpoint".to_string())).into_response()
            }
        }
    }
}

/// DELETE /api/endpoints/:id - エンドポイント削除
pub async fn delete_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // arch-review [L12]: 削除は registry と LoadManager の状態掃除を単一の調整メソッドに
    // 集約した remove_endpoint_fully を通す。
    match remove_endpoint_fully(&state, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to delete endpoint: {}", e);
            AppError(LbError::Database("Failed to delete endpoint".to_string())).into_response()
        }
    }
}

/// エンドポイントをレジストリ・DB・LoadManager 状態から一括削除する調整メソッド。
///
/// arch-review [L12]: エンドポイントの runtime 状態が EndpointRegistry と LoadManager に
/// 分散し、削除時に呼び出し側が両方を手で掃除する必要があった。EndpointRegistry::remove
/// は LoadManager の負荷/TPS 状態までは掃除しないため、両者の掃除を本メソッドへ集約し、
/// 削除経路が LoadManager 状態の破棄を取りこぼさないようにする。
async fn remove_endpoint_fully(state: &AppState, id: Uuid) -> Result<bool, sqlx::Error> {
    let removed = state.endpoint_registry.remove(id).await?;
    if removed {
        // 負荷状態・TPS状態がリークしないよう明示的に破棄する。
        state.load_manager.forget_endpoint(id).await;
    }
    Ok(removed)
}

/// POST /api/endpoints/:id/test - 接続テスト
pub async fn test_endpoint(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // エンドポイントを取得
    let endpoint = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for test: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    let response = run_connection_test(&state, &endpoint).await;
    (StatusCode::OK, Json(response)).into_response()
}

/// POST /api/endpoints/:id/sync - モデル一覧同期
pub async fn sync_endpoint_models(
    Extension(claims): Extension<Claims>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Admin権限チェック
    if let Err(e) = ensure_admin(&claims) {
        return e.into_response();
    }

    // エンドポイントを取得
    let endpoint = match db::get_endpoint(&state.db_pool, id).await {
        Ok(Some(ep)) => ep,
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint for sync: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
    };

    let result = sync::sync_models_with_type(
        &state.db_pool,
        &state.http_client,
        id,
        &endpoint.base_url,
        endpoint.api_key.as_deref(),
        endpoint.inference_timeout_secs as u64,
        Some(endpoint.endpoint_type),
    )
    .await;

    match result {
        Ok(result) => {
            // EndpointRegistryキャッシュをリロードしてモデルマッピングを更新
            let _ = state.endpoint_registry.reload().await;

            let synced_models = result
                .models
                .into_iter()
                .map(EndpointModelResponse::from)
                .collect();

            (
                StatusCode::OK,
                Json(SyncModelsResponse {
                    synced_models,
                    added: result.added,
                    removed: result.removed,
                    updated: result.updated,
                }),
            )
                .into_response()
        }
        Err(err) => {
            let lb_error = match err {
                SyncError::ConnectionError(msg) => {
                    LbError::ServiceUnavailable(format!("Failed to connect: {}", msg))
                }
                SyncError::HttpError(status, _) => {
                    LbError::Http(format!("Endpoint returned HTTP {}", status))
                }
                SyncError::ParseError(msg) => {
                    LbError::Http(format!("Failed to parse response: {}", msg))
                }
                SyncError::DbError(msg) => {
                    LbError::Database(format!("Failed to update models: {}", msg))
                }
            };

            AppError(lb_error).into_response()
        }
    }
}

/// GET /api/endpoints/:id/models - エンドポイントのモデル一覧
pub async fn list_endpoint_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // エンドポイント存在確認
    match db::get_endpoint(&state.db_pool, id).await {
        Ok(None) => return AppError(LbError::EndpointNotFound(id)).into_response(),
        Err(e) => {
            tracing::error!("Failed to get endpoint: {}", e);
            return AppError(LbError::Database("Failed to get endpoint".to_string()))
                .into_response();
        }
        Ok(Some(_)) => {}
    }

    match db::list_endpoint_models(&state.db_pool, id).await {
        Ok(models) => (
            StatusCode::OK,
            Json(EndpointModelsResponse {
                endpoint_id: id,
                models: models
                    .into_iter()
                    .map(EndpointModelResponse::from)
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to list endpoint models: {}", e);
            AppError(LbError::Database("Failed to list models".to_string())).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::auth::{Claims, UserRole};
    use crate::db::test_utils::{TestAppStateBuilder, TEST_LOCK};
    use crate::types::endpoint::{
        DeviceInfo, DeviceType, DownloadStatus, Endpoint, EndpointModel, EndpointStatus,
        EndpointType, GpuDevice, ModelDownloadTask,
    };
    use axum::{
        body::{to_bytes, Bytes},
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
        Extension, Json,
    };
    use chrono::Utc;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_default_health_check_interval() {
        assert_eq!(default_health_check_interval(), 30);
    }

    #[test]
    fn test_create_endpoint_request_minimal() {
        let json = json!({
            "name": "test-ep",
            "base_url": "http://localhost:8080"
        });
        let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, "test-ep");
        assert_eq!(req.base_url, "http://localhost:8080");
        assert!(req.api_key.is_none());
        assert_eq!(req.health_check_interval_secs, 30);
        assert!(req.inference_timeout_secs.is_none());
        assert!(req.notes.is_none());
        assert!(req.capabilities.is_empty());
    }

    #[test]
    fn test_create_endpoint_request_full() {
        let json = json!({
            "name": "my-endpoint",
            "base_url": "http://gpu-server:11434",
            "api_key": "sk-test-key",
            "health_check_interval_secs": 60,
            "inference_timeout_secs": 300,
            "notes": "Production GPU node",
            "capabilities": ["image_generation"]
        });
        let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, "my-endpoint");
        assert_eq!(req.api_key, Some("sk-test-key".to_string()));
        assert_eq!(req.health_check_interval_secs, 60);
        assert_eq!(req.inference_timeout_secs, Some(300));
        assert_eq!(req.notes, Some("Production GPU node".to_string()));
        assert_eq!(req.capabilities.len(), 1);
    }

    #[test]
    fn test_create_endpoint_request_missing_name_fails() {
        let json = json!({ "base_url": "http://localhost:8080" });
        assert!(serde_json::from_value::<CreateEndpointRequest>(json).is_err());
    }

    #[test]
    fn test_create_endpoint_request_missing_base_url_fails() {
        let json = json!({ "name": "test" });
        assert!(serde_json::from_value::<CreateEndpointRequest>(json).is_err());
    }

    #[test]
    fn test_update_endpoint_request_empty() {
        let json = json!({});
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.base_url.is_none());
        assert!(req.api_key.is_none());
        assert!(req.health_check_interval_secs.is_none());
        assert!(req.inference_timeout_secs.is_none());
        assert!(req.notes.is_none());
    }

    #[test]
    fn test_update_endpoint_request_partial() {
        let json = json!({ "name": "new-name", "health_check_interval_secs": 45 });
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, Some("new-name".to_string()));
        assert_eq!(req.health_check_interval_secs, Some(45));
        assert!(req.base_url.is_none());
    }

    #[test]
    fn test_update_endpoint_request_notes_null_clears() {
        let json = json!({ "notes": null });
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.notes, Some(None));
    }

    #[test]
    fn test_update_endpoint_request_notes_with_value() {
        let json = json!({ "notes": "updated note" });
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.notes, Some(Some("updated note".to_string())));
    }

    #[tokio::test]
    async fn update_endpoint_syncs_registry_cache() {
        let _guard = TEST_LOCK.lock().await;
        let state = TestAppStateBuilder::new().await.build().await;

        let endpoint = Endpoint::new(
            "sync-cache".to_string(),
            "http://localhost:8080".to_string(),
            EndpointType::OpenaiCompatible,
        );
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");

        let claims = Claims {
            sub: "admin-user".to_string(),
            role: UserRole::Admin,
            exp: 0,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };

        let response = update_endpoint(
            Extension(claims),
            State(state.clone()),
            Path(endpoint_id),
            Json(UpdateEndpointRequest {
                name: None,
                base_url: None,
                api_key: None,
                health_check_interval_secs: None,
                inference_timeout_secs: Some(1),
                notes: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let updated = state
            .endpoint_registry
            .get(endpoint_id)
            .await
            .expect("endpoint remains in registry");
        assert_eq!(updated.inference_timeout_secs, 1);
    }

    #[tokio::test]
    async fn proxy_chat_completions_keeps_endpoint_online_on_client_error() {
        let _guard = TEST_LOCK.lock().await;
        let state = TestAppStateBuilder::new().await.build().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "message": "invalid model",
                    "type": "invalid_request_error",
                    "code": 400
                }
            })))
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "proxy-client-error".to_string(),
            server.uri(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint to registry");

        let response = proxy_chat_completions(
            State(state.clone()),
            Path(endpoint_id),
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "bad-model",
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .expect("serialize request"),
            ),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("proxy response body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("proxy response json");
        assert_eq!(json["error"]["message"], "invalid model");

        let updated = state
            .endpoint_registry
            .get(endpoint_id)
            .await
            .expect("endpoint remains in registry");
        assert_eq!(updated.status, EndpointStatus::Online);
        assert_eq!(updated.last_error, None);
    }

    #[tokio::test]
    async fn proxy_chat_completions_does_not_probe_ollama_load_state_on_success() {
        let _guard = TEST_LOCK.lock().await;
        let state = TestAppStateBuilder::new().await.build().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-123",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "proxy-ollama-success".to_string(),
            server.uri(),
            EndpointType::Ollama,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint to registry");

        let response = proxy_chat_completions(
            State(state.clone()),
            Path(endpoint_id),
            Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "qwen3:30b",
                    "messages": [{"role": "user", "content": "hello"}]
                }))
                .expect("serialize request"),
            ),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/v1/chat/completions");
    }

    fn sample_endpoint() -> Endpoint {
        let mut ep = Endpoint::new(
            "test-ep".to_string(),
            "http://localhost:8080".to_string(),
            EndpointType::Xllm,
        );
        ep.status = EndpointStatus::Online;
        ep.latency_ms = Some(42);
        ep.error_count = 0;
        ep.notes = Some("note".to_string());
        ep
    }

    #[test]
    fn test_endpoint_response_from_endpoint() {
        let ep = sample_endpoint();
        let id = ep.id;
        let resp = EndpointResponse::from(ep);
        assert_eq!(resp.id, id);
        assert_eq!(resp.name, "test-ep");
        assert_eq!(resp.base_url, "http://localhost:8080");
        assert_eq!(resp.status, "online");
        assert_eq!(resp.endpoint_type, "xllm");
        assert_eq!(resp.latency_ms, Some(42));
        assert_eq!(resp.error_count, 0);
        assert_eq!(resp.notes, Some("note".to_string()));
        assert!(resp.model_count.is_none());
        assert!(resp.models.is_none());
    }

    #[test]
    fn test_endpoint_response_serialization() {
        let ep = sample_endpoint();
        let resp = EndpointResponse::from(ep);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "test-ep");
        assert_eq!(json["status"], "online");
        assert_eq!(json["endpoint_type"], "xllm");
        assert_eq!(json["latency_ms"], 42);
    }

    #[test]
    fn test_endpoint_response_with_device_info() {
        let mut ep = sample_endpoint();
        ep.device_info = Some(DeviceInfo {
            device_type: DeviceType::Gpu,
            gpu_devices: vec![GpuDevice {
                name: "RTX 4090".to_string(),
                total_memory_bytes: 24_000_000_000,
                used_memory_bytes: 8_000_000_000,
            }],
        });
        let resp = EndpointResponse::from(ep);
        assert!(resp.device_info.is_some());
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["device_info"].is_object());
    }

    #[test]
    fn test_endpoint_response_offline_status() {
        let mut ep = sample_endpoint();
        ep.status = EndpointStatus::Offline;
        ep.latency_ms = None;
        ep.last_error = Some("Connection refused".to_string());
        ep.error_count = 5;
        let resp = EndpointResponse::from(ep);
        assert_eq!(resp.status, "offline");
        assert!(resp.latency_ms.is_none());
        assert_eq!(resp.last_error, Some("Connection refused".to_string()));
        assert_eq!(resp.error_count, 5);
    }

    #[test]
    fn test_endpoint_response_pending_status() {
        let mut ep = sample_endpoint();
        ep.status = EndpointStatus::Pending;
        let resp = EndpointResponse::from(ep);
        assert_eq!(resp.status, "pending");
    }

    #[test]
    fn test_endpoint_response_error_status() {
        let mut ep = sample_endpoint();
        ep.status = EndpointStatus::Error;
        let resp = EndpointResponse::from(ep);
        assert_eq!(resp.status, "error");
    }

    #[test]
    fn test_list_endpoints_response_empty() {
        let resp = ListEndpointsResponse {
            endpoints: vec![],
            total: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 0);
        assert!(json["endpoints"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_list_endpoints_query_empty() {
        let json = json!({});
        let q: ListEndpointsQuery = serde_json::from_value(json).unwrap();
        assert!(q.status.is_none());
        assert!(q.endpoint_type.is_none());
    }

    #[test]
    fn test_list_endpoints_query_with_filters() {
        let json = json!({ "status": "online", "type": "xllm" });
        let q: ListEndpointsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.status, Some("online".to_string()));
        assert_eq!(q.endpoint_type, Some("xllm".to_string()));
    }

    #[test]
    fn test_endpoint_model_response_from() {
        let model = EndpointModel {
            endpoint_id: Uuid::new_v4(),
            model_id: "llama3".to_string(),
            capabilities: Some(vec!["chat".to_string()]),
            max_tokens: Some(8192),
            last_checked: Some(Utc::now()),
            supported_apis: vec![],
            canonical_name: None,
        };
        let resp = EndpointModelResponse::from(model);
        assert_eq!(resp.model_id, "llama3");
        assert_eq!(resp.capabilities, Some(vec!["chat".to_string()]));
        assert_eq!(resp.max_tokens, Some(8192));
        assert!(resp.last_checked.is_some());
        assert_eq!(resp.canonical_name, None);
    }

    #[test]
    fn test_endpoint_model_response_from_with_canonical_name() {
        let model = EndpointModel {
            endpoint_id: Uuid::new_v4(),
            model_id: "gpt-oss:20b".to_string(),
            capabilities: Some(vec!["chat".to_string()]),
            max_tokens: Some(8192),
            last_checked: None,
            supported_apis: vec![],
            canonical_name: Some("openai/gpt-oss-20b".to_string()),
        };
        let resp = EndpointModelResponse::from(model);
        assert_eq!(resp.model_id, "gpt-oss:20b");
        assert_eq!(resp.canonical_name, Some("openai/gpt-oss-20b".to_string()));

        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["canonical_name"], "openai/gpt-oss-20b");
    }

    #[test]
    fn test_endpoint_model_response_from_minimal() {
        let model = EndpointModel {
            endpoint_id: Uuid::new_v4(),
            model_id: "embed-v1".to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![],
            canonical_name: None,
        };
        let resp = EndpointModelResponse::from(model);
        assert_eq!(resp.model_id, "embed-v1");
        assert!(resp.capabilities.is_none());
        assert!(resp.max_tokens.is_none());
        assert!(resp.last_checked.is_none());
    }

    #[test]
    fn test_endpoint_models_response_serialization() {
        let id = Uuid::new_v4();
        let resp = EndpointModelsResponse {
            endpoint_id: id,
            models: vec![EndpointModelResponse {
                model_id: "gpt-4".to_string(),
                capabilities: Some(vec!["chat".to_string(), "embeddings".to_string()]),
                max_tokens: Some(128000),
                last_checked: None,
                canonical_name: None,
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["endpoint_id"], id.to_string());
        assert_eq!(json["models"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_sync_models_response_serialization() {
        let resp = SyncModelsResponse {
            synced_models: vec![],
            added: 3,
            removed: 1,
            updated: 2,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["added"], 3);
        assert_eq!(json["removed"], 1);
        assert_eq!(json["updated"], 2);
    }

    #[test]
    fn test_connection_response_success() {
        let resp = TestConnectionResponse {
            success: true,
            latency_ms: Some(15),
            error: None,
            models_found: Some(vec!["llama3".to_string()]),
            endpoint_info: Some(EndpointTestInfo { model_count: 1 }),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["latency_ms"], 15);
    }

    #[test]
    fn test_connection_response_failure() {
        let resp = TestConnectionResponse {
            success: false,
            latency_ms: None,
            error: Some("Connection refused".to_string()),
            models_found: None,
            endpoint_info: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "Connection refused");
    }

    #[test]
    fn test_download_model_request() {
        let json = json!({ "model": "llama3:8b" });
        let req: DownloadModelRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.model, "llama3:8b");
    }

    #[test]
    fn test_download_model_request_missing_model_fails() {
        let json = json!({});
        assert!(serde_json::from_value::<DownloadModelRequest>(json).is_err());
    }

    #[test]
    fn test_download_task_response_from() {
        let task = ModelDownloadTask {
            id: "task-1".to_string(),
            endpoint_id: Uuid::new_v4(),
            model: "llama3:8b".to_string(),
            status: DownloadStatus::Downloading,
            progress: 45.5,
            speed_mbps: Some(120.0),
            eta_seconds: Some(60),
            error_message: None,
            started_at: Utc::now(),
            completed_at: None,
            filename: None,
        };
        let resp = DownloadTaskResponse::from(task);
        assert_eq!(resp.task_id, "task-1");
        assert_eq!(resp.model, "llama3:8b");
        assert_eq!(resp.status, "downloading");
        assert!((resp.progress - 45.5).abs() < f64::EPSILON);
        assert_eq!(resp.speed_mbps, Some(120.0));
        assert_eq!(resp.eta_seconds, Some(60));
        assert!(resp.error_message.is_none());
    }

    #[test]
    fn test_download_task_response_completed() {
        let task = ModelDownloadTask {
            id: "task-2".to_string(),
            endpoint_id: Uuid::new_v4(),
            model: "mistral:7b".to_string(),
            status: DownloadStatus::Completed,
            progress: 100.0,
            speed_mbps: None,
            eta_seconds: None,
            error_message: None,
            started_at: Utc::now(),
            completed_at: None,
            filename: Some("model.gguf".to_string()),
        };
        let resp = DownloadTaskResponse::from(task);
        assert_eq!(resp.status, "completed");
        assert!((resp.progress - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_download_task_response_failed() {
        let task = ModelDownloadTask {
            id: "task-3".to_string(),
            endpoint_id: Uuid::new_v4(),
            model: "big-model".to_string(),
            status: DownloadStatus::Failed,
            progress: 30.0,
            speed_mbps: None,
            eta_seconds: None,
            error_message: Some("Disk full".to_string()),
            started_at: Utc::now(),
            completed_at: None,
            filename: None,
        };
        let resp = DownloadTaskResponse::from(task);
        assert_eq!(resp.status, "failed");
        assert_eq!(resp.error_message, Some("Disk full".to_string()));
    }

    #[test]
    fn test_download_task_response_serialization_skips_none() {
        let task = ModelDownloadTask {
            id: "task-4".to_string(),
            endpoint_id: Uuid::new_v4(),
            model: "test".to_string(),
            status: DownloadStatus::Pending,
            progress: 0.0,
            speed_mbps: None,
            eta_seconds: None,
            error_message: None,
            started_at: Utc::now(),
            completed_at: None,
            filename: None,
        };
        let resp = DownloadTaskResponse::from(task);
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("speed_mbps").is_none());
        assert!(json.get("eta_seconds").is_none());
        assert!(json.get("error_message").is_none());
    }

    #[test]
    fn test_download_progress_response_empty() {
        let id = Uuid::new_v4();
        let resp = DownloadProgressResponse {
            endpoint_id: id,
            tasks: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["endpoint_id"], id.to_string());
        assert!(json["tasks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_model_info_response_serialization() {
        let id = Uuid::new_v4();
        let resp = ModelInfoResponse {
            model_id: "llama3".to_string(),
            endpoint_id: id,
            max_tokens: Some(8192),
            last_checked: Some("2024-01-01T00:00:00Z".to_string()),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["model_id"], "llama3");
        assert_eq!(json["endpoint_id"], id.to_string());
        assert_eq!(json["max_tokens"], 8192);
    }

    #[test]
    fn test_model_info_response_skips_none() {
        let resp = ModelInfoResponse {
            model_id: "test".to_string(),
            endpoint_id: Uuid::new_v4(),
            max_tokens: None,
            last_checked: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("max_tokens").is_none());
        assert!(json.get("last_checked").is_none());
    }

    #[test]
    fn test_model_info_path_deserialization() {
        let id = Uuid::new_v4();
        let json = json!({ "id": id.to_string(), "model": "llama3:8b" });
        let path: ModelInfoPath = serde_json::from_value(json).unwrap();
        assert_eq!(path.id, id);
        assert_eq!(path.model, "llama3:8b");
    }

    #[test]
    fn test_error_response_serialization() {
        let resp = ErrorResponse {
            error: "Something went wrong".to_string(),
            code: "INTERNAL_ERROR".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"], "Something went wrong");
        assert_eq!(json["code"], "INTERNAL_ERROR");
    }

    #[test]
    fn test_ensure_admin_with_admin_role() {
        let claims = Claims {
            sub: "admin-user".to_string(),
            role: UserRole::Admin,
            exp: 0,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };
        assert!(ensure_admin(&claims).is_ok());
    }

    #[test]
    fn test_ensure_admin_with_viewer_role() {
        let claims = Claims {
            sub: "viewer-user".to_string(),
            role: UserRole::Viewer,
            exp: 0,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };
        assert!(ensure_admin(&claims).is_err());
    }

    #[test]
    fn test_endpoint_test_info_serialization() {
        let info = EndpointTestInfo { model_count: 5 };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["model_count"], 5);
    }

    #[test]
    fn test_optional_field_absent() {
        let json = json!({});
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert!(req.notes.is_none());
    }

    #[test]
    fn test_optional_field_null() {
        let json = json!({ "notes": null });
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.notes, Some(None));
    }

    #[test]
    fn test_optional_field_present() {
        let json = json!({ "notes": "hello" });
        let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.notes, Some(Some("hello".to_string())));
    }

    #[test]
    fn test_endpoint_response_all_types() {
        for (ep_type, expected) in [
            (EndpointType::Xllm, "xllm"),
            (EndpointType::Ollama, "ollama"),
            (EndpointType::Vllm, "vllm"),
            (EndpointType::OpenaiCompatible, "openai_compatible"),
            (EndpointType::LmStudio, "lm_studio"),
        ] {
            let ep = Endpoint::new("ep".to_string(), "http://localhost".to_string(), ep_type);
            let resp = EndpointResponse::from(ep);
            assert_eq!(resp.endpoint_type, expected);
        }
    }

    #[test]
    fn test_create_endpoint_request_multiple_capabilities() {
        let json = json!({
            "name": "multi-cap",
            "base_url": "http://localhost:8080",
            "capabilities": ["image_generation", "audio_transcription", "audio_speech"]
        });
        let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.capabilities.len(), 3);
    }

    #[test]
    fn test_endpoint_response_registered_at_is_rfc3339() {
        let ep = sample_endpoint();
        let resp = EndpointResponse::from(ep);
        assert!(chrono::DateTime::parse_from_rfc3339(&resp.registered_at).is_ok());
    }
}
