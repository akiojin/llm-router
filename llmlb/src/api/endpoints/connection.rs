//! エンドポイントの接続テスト。
//!
//! GET /v1/models でレイテンシを計測しモデル一覧を抽出、ステータスを更新して
//! 結果を返す。薄い HTTP ハンドラ test_endpoint を含む。

use super::dto::{EndpointTestInfo, TestConnectionResponse};
use super::ensure_admin;
use crate::api::error::AppError;
use crate::common::auth::Claims;
use crate::common::error::LbError;
use crate::common::http::RequestBuilderBearerExt;
use crate::db::endpoints as db;
use crate::types::endpoint::{Endpoint, EndpointStatus};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use uuid::Uuid;

/// 接続テスト実行（DB/キャッシュの更新を含む）
pub(super) async fn run_connection_test(
    state: &AppState,
    endpoint: &Endpoint,
) -> TestConnectionResponse {
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
