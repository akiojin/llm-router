//! エンドポイント登録フロー。
//!
//! 入力検証→タイプ検出→DB作成→キャッシュ登録→登録直後のバックグラウンド
//! タスク（接続テスト・モデル同期）起動までを担う。

use super::connection::run_connection_test;
use super::dto::{CreateEndpointRequest, EndpointResponse};
use super::ensure_admin;
use crate::api::error::AppError;
use crate::common::auth::Claims;
use crate::common::error::{CommonError, LbError};
use crate::db::endpoints as db;
use crate::detection::{detect_endpoint_type_with_client, DetectionError};
use crate::sync;
use crate::system_info;
use crate::types::endpoint::{DeviceInfo, Endpoint, EndpointType};
use crate::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use reqwest::Url;

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
