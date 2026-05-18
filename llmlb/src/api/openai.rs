//! OpenAI互換APIエンドポイント (/v1/*)
//!
//! このモジュールはEndpointRegistry/Endpoint型を使用しています。

/// 未指定/仮想IPアドレス（クラウドプロバイダ等、実IPを持たない場合に使用）
const UNSPECIFIED_IP: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

/// `id` の組織プレフィックス（"org/name" の "org"）を `owned_by` として返す。
/// スラッシュがない場合は "load balancer" にフォールバック。
fn owned_by_from_id(id: &str) -> String {
    match id.split_once('/') {
        Some((org, _)) if !org.is_empty() => org.to_string(),
        _ => "load balancer".to_string(),
    }
}

/// 登録済みモデルの capabilities とエンドポイント申告 supported_apis を統合し、
/// `as_str()` 昇順で並べた `Vec<String>` を返す（OpenAI互換 /v1/models 用）。
///
/// 解決順:
/// 1. 登録モデルの `ModelCapability` から導出した SupportedAPI を加算する
///    （A-1: embedding capability の取りこぼし対策）。
/// 2. **常に** endpoint_reported もマージする。`ModelInfo::get_capabilities()` は
///    メタデータが空のレガシー行に対して `[TextGeneration]` を既定で返すため、
///    capability 由来の set が非空でも endpoint がサポートする `embeddings` 等を
///    取りこぼさないように両者をマージする（CodeRabbit/Codex review #642 対応）。
/// 3. 双方が何も寄与しなければ `ChatCompletions` のフォールバック。
/// 4. 最後に `as_str()` 昇順で並べて決定論化する（A-3）。
fn build_supported_apis(
    model: Option<&crate::registry::models::ModelInfo>,
    endpoint_reported: Option<&std::collections::HashSet<crate::types::endpoint::SupportedAPI>>,
) -> Vec<String> {
    use crate::types::endpoint::SupportedAPI;
    use std::collections::HashSet;

    let mut set: HashSet<SupportedAPI> = HashSet::new();
    if let Some(m) = model {
        for cap in m.get_capabilities() {
            match cap {
                ModelCapability::TextGeneration => {
                    set.insert(SupportedAPI::ChatCompletions);
                    set.insert(SupportedAPI::Responses);
                }
                ModelCapability::Embedding => {
                    set.insert(SupportedAPI::Embeddings);
                }
                ModelCapability::ImageInput => {
                    set.insert(SupportedAPI::ImageInput);
                }
                ModelCapability::ImageGeneration => {
                    set.insert(SupportedAPI::ImageGeneration);
                }
                _ => {}
            }
        }
    }
    if let Some(reported) = endpoint_reported {
        for api in reported {
            set.insert(*api);
        }
    }
    if set.is_empty() {
        set.insert(SupportedAPI::ChatCompletions);
    }
    let mut v: Vec<SupportedAPI> = set.into_iter().collect();
    v.sort_by_key(|a| a.as_str());
    v.into_iter().map(|a| a.as_str().to_string()).collect()
}

use crate::common::{
    error::{CommonError, LbError},
    protocol::{RecordStatus, RequestResponseRecord, RequestType, TpsApiKind},
};
use crate::types::{
    endpoint::SupportedAPI,
    model::{ModelCapabilities, ModelCapability},
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{collections::HashMap, net::IpAddr, net::SocketAddr, time::Instant};
use tracing::{error, warn};
use uuid::Uuid;

use crate::auth::middleware::ApiKeyAuthContext;
use crate::common::ip::{normalize_ip, normalize_socket_ip};

use crate::{
    api::{
        cloud_proxy::{proxy_cloud_provider, resolve_provider},
        error::AppError,
        model_name::{
            parse_quantized_model_name, rewrite_payload_model_for_endpoint, ParsedModelName,
        },
        models::{list_registered_models, load_registered_model, LifecycleStatus},
        openai_util::{
            classify_upstream_request_error, model_unavailable_response, openai_error_response,
            openai_error_response_with_type, probe_ollama_model_loaded, queue_error_response,
            sanitize_openai_payload_for_history, upstream_error_message_from_bytes,
        },
        proxy::{
            forward_streaming_response_with_tps_tracking, record_endpoint_request_stats,
            save_request_record, select_available_endpoint,
            select_available_endpoint_with_queue_for_model,
            select_available_endpoint_with_queue_for_model_and_api, QueueSelection,
        },
    },
    balancer::RequestOutcome,
    token::extract_usage_from_response,
    AppState,
};

/// SPEC-f8e3a1b7: 推論リクエスト成功時にエンドポイントのレイテンシを更新（Fire-and-forget）
fn update_inference_latency(
    registry: &crate::registry::endpoints::EndpointRegistry,
    endpoint_id: Uuid,
    duration: std::time::Duration,
) {
    let registry = registry.clone();
    let latency_ms = duration.as_millis() as f64;
    tokio::spawn(async move {
        if let Err(e) = registry
            .update_inference_latency(endpoint_id, latency_ms)
            .await
        {
            tracing::debug!(
                endpoint_id = %endpoint_id,
                latency_ms = latency_ms,
                error = %e,
                "Failed to update inference latency"
            );
        }
    });
}

fn add_queue_headers(response: &mut Response, wait_ms: u128) {
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-queue-status"),
        HeaderValue::from_static("queued"),
    );
    let wait_value = wait_ms.to_string();
    if let Ok(value) = HeaderValue::from_str(&wait_value) {
        headers.insert(HeaderName::from_static("x-queue-wait-ms"), value);
    }
}

/// クライアントIPとAPIキーIDを抽出するヘルパー
fn extract_client_info(
    addr: &SocketAddr,
    headers: &HeaderMap,
    auth_ctx: &Option<axum::Extension<ApiKeyAuthContext>>,
) -> (Option<IpAddr>, Option<Uuid>) {
    let client_ip =
        Some(extract_client_ip_from_headers(headers).unwrap_or_else(|| normalize_socket_ip(addr)));
    let api_key_id = auth_ctx.as_ref().map(|ext| ext.0.id);
    (client_ip, api_key_id)
}

fn extract_client_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    extract_x_forwarded_for(headers).or_else(|| extract_forwarded_for(headers))
}

fn extract_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value
        .split(',')
        .map(str::trim)
        .find_map(parse_client_ip_from_forwarded_value)
}

fn extract_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("forwarded")?.to_str().ok()?;
    value.split(',').find_map(|entry| {
        entry
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                if key.trim().eq_ignore_ascii_case("for") {
                    parse_client_ip_from_forwarded_value(value.trim())
                } else {
                    None
                }
            })
    })
}

fn parse_client_ip_from_forwarded_value(value: &str) -> Option<IpAddr> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") || trimmed.starts_with('_') {
        return None;
    }

    let host = if let Some(stripped) = trimmed.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default().trim()
    } else {
        trimmed
    };

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(normalize_ip(ip));
    }

    if let Some((ip_candidate, _port)) = host.rsplit_once(':') {
        if !ip_candidate.contains(':') {
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                return Some(normalize_ip(ip));
            }
        }
    }

    None
}

/// POST /v1/chat/completions - OpenAI互換チャットAPI
#[allow(deprecated)] // NodeRegistry migration in progress
pub async fn chat_completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model(&payload)?;
    let parsed = if parse_cloud_model(&model).is_some() {
        ParsedModelName {
            raw: model.clone(),
            base: model.clone(),
            quantization: None,
        }
    } else {
        parse_quantized_model_name(&model).map_err(AppError::from)?
    };
    let requires_image_input = payload_requires_image_input(&payload);

    // モデルの TextGeneration capability を検証
    let models = list_registered_models(&state.db_pool).await?;
    if let Some(model_info) = models.iter().find(|m| m.name == model) {
        if !model_info.has_capability(ModelCapability::TextGeneration) {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("Model '{}' does not support text generation", parsed.raw),
            ))));
        }
        if requires_image_input && !model_info.has_capability(ModelCapability::ImageInput) {
            return Err(AppError::from(LbError::Common(CommonError::Validation(
                format!("Model '{}' does not support image input", parsed.raw),
            ))));
        }
    }
    // 登録されていないモデルはエンドポイント側で処理（クラウドモデル等）

    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/chat/completions",
        parsed.raw,
        stream,
        RequestType::Chat,
        client_ip,
        api_key_id,
    )
    .await
}

/// POST /api/dashboard/playground/chat/completions - Dashboardセッション用チャットAPI
///
/// LB Playgroundからの試行リクエストを、APIキーではなくJWTセッションで受け付ける。
/// 外部クライアント向けの`/v1/chat/completions`は引き続きAPIキー必須。
pub async fn dashboard_playground_chat_completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    chat_completions(
        ConnectInfo(addr),
        headers,
        State(state),
        None,
        Json(payload),
    )
    .await
}

/// POST /v1/completions - OpenAI互換テキスト補完API
pub async fn completions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model(&payload)?;
    if parse_cloud_model(&model).is_none() {
        parse_quantized_model_name(&model).map_err(AppError::from)?;
    }
    let stream = extract_stream(&payload);
    proxy_openai_post(
        &state,
        payload,
        "/v1/completions",
        model,
        stream,
        RequestType::Generate,
        client_ip,
        api_key_id,
    )
    .await
}

/// POST /v1/embeddings - OpenAI互換Embeddings API
pub async fn embeddings(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &auth_ctx);
    let model = extract_model_with_default(&payload, crate::config::get_default_embedding_model());
    if parse_cloud_model(&model).is_none() {
        parse_quantized_model_name(&model).map_err(AppError::from)?;
    }
    proxy_openai_post(
        &state,
        payload,
        "/v1/embeddings",
        model,
        false,
        RequestType::Embeddings,
        client_ip,
        api_key_id,
    )
    .await
}

/// GET /v1/models - モデル一覧取得（OpenAI互換 + Azure capabilities + ダッシュボード拡張）
///
/// OpenAI API 互換形式に Azure OpenAI 形式の capabilities と
/// ダッシュボード用の拡張フィールド（lifecycle_status, download_progress, ready）を追加。
/// 登録済みの全モデルを返す（ダウンロード中・待機中含む）。
pub async fn list_models(State(state): State<AppState>) -> Result<Response, AppError> {
    use crate::types::endpoint::SupportedAPI;
    use std::collections::HashSet;

    // Load registered models from the database.
    let mut registered_map: std::collections::HashMap<String, crate::registry::models::ModelInfo> =
        HashMap::new();
    for model in list_registered_models(&state.db_pool).await? {
        registered_map.insert(model.name.clone(), model);
    }

    // SPEC-0f1de549: エンドポイントのモデルとsupported_apisを取得
    let mut endpoint_model_apis: HashMap<String, HashSet<SupportedAPI>> = HashMap::new();
    let mut endpoint_model_max_tokens: HashMap<String, Option<u32>> = HashMap::new();
    let mut endpoint_model_ids: HashMap<String, HashSet<String>> = HashMap::new();
    // canonical name解決マップを構築
    let canonical_resolution;
    {
        let registry = &state.endpoint_registry;
        let online_endpoints = registry.list_online().await;

        // 全エンドポイントモデルを収集してcanonical解決マップを構築
        let mut all_models: Vec<(String, Option<String>)> = Vec::new();
        for ep in &online_endpoints {
            if let Ok(models) = registry.list_models(ep.id).await {
                for model in &models {
                    all_models.push((model.model_id.clone(), model.canonical_name.clone()));
                }
            }
        }
        canonical_resolution = crate::models::mapping::build_canonical_maps(
            all_models
                .iter()
                .map(|(id, cn)| (id.as_str(), cn.as_deref())),
        );

        for ep in online_endpoints {
            if let Ok(models) = registry.list_models(ep.id).await {
                for model in models {
                    // 表示用キーの決定: canonical_nameがあればそれを使用
                    let display_key = model
                        .canonical_name
                        .clone()
                        .unwrap_or_else(|| model.model_id.clone());

                    endpoint_model_ids
                        .entry(display_key.clone())
                        .or_default()
                        .insert(ep.id.to_string());
                    // エイリアス名でもエンドポイントIDを登録（ルーティング用）
                    if display_key != model.model_id {
                        endpoint_model_ids
                            .entry(model.model_id.clone())
                            .or_default()
                            .insert(ep.id.to_string());
                    }
                    let apis = endpoint_model_apis.entry(display_key.clone()).or_default();
                    for api in model.supported_apis {
                        apis.insert(api);
                    }
                    // Responses APIは全エンドポイント対応前提（判定/フラグは廃止）
                    apis.insert(SupportedAPI::Responses);

                    // max_tokens を集約（複数エンドポイントにある場合は最大値を採用）
                    let entry = endpoint_model_max_tokens.entry(display_key).or_insert(None);
                    if let Some(mt) = model.max_tokens {
                        *entry = Some(entry.map_or(mt, |existing| existing.max(mt)));
                    }
                }
            }
        }
    }

    // オンラインエンドポイントの実行可能モデル一覧を構築
    let mut available_models: Vec<String> = endpoint_model_apis.keys().cloned().collect();
    available_models.sort();
    let available_set: std::collections::HashSet<String> =
        available_models.iter().cloned().collect();

    // OpenAI互換レスポンス形式 + Azure capabilities + ダッシュボード拡張
    let mut data: Vec<Value> = Vec::new();

    // 観測時刻（last_modified が無いモデルは観測時刻を created に採用）
    let observed_now = chrono::Utc::now().timestamp();

    // ノードのモデルを追加
    for model_id in &available_models {
        let ready = available_set.contains(model_id);

        // supported_apis: 登録モデルの capability 由来 / エンドポイント申告 / フォールバックを統合し as_str() 昇順で返す
        let supported_apis: Vec<String> = build_supported_apis(
            registered_map.get(model_id),
            endpoint_model_apis.get(model_id),
        );
        let endpoint_ids: Vec<String> = endpoint_model_ids
            .get(model_id)
            .map(|ids| {
                let mut ids: Vec<String> = ids.iter().cloned().collect();
                ids.sort();
                ids
            })
            .unwrap_or_default();

        // エイリアス情報を取得
        let aliases = canonical_resolution.aliases_for(model_id);
        // canonical_nameを取得（self-fallback により null は返らない）
        let canonical_name = canonical_resolution.canonical_for(model_id);
        // max_tokens: endpoint 申告 → 既知 canonical テーブルの順で解決
        let max_tokens = crate::models::mapping::resolve_max_tokens(
            &canonical_name,
            endpoint_model_max_tokens.get(model_id).copied().flatten(),
        );
        // 量子化サフィックスを ID から分離（G-3 暫定: ID は維持しつつ別フィールドへ出す）
        let (_, quantization) = crate::models::mapping::split_quantization_suffix(model_id);
        let quantization = quantization.map(|s| s.to_string());

        if let Some(m) = registered_map.get(model_id) {
            let caps: ModelCapabilities = m.get_capabilities().into();
            let created = m
                .last_modified
                .map(|t| t.timestamp())
                .unwrap_or(observed_now);
            let obj = json!({
                "id": m.name,
                "object": "model",
                "created": created,
                "owned_by": owned_by_from_id(&m.name),
                "capabilities": caps,
                "lifecycle_status": LifecycleStatus::Registered,
                "download_progress": null,
                "ready": ready,
                "repo": m.repo,
                "filename": m.filename,
                "size_bytes": m.size,
                "required_memory_bytes": m.required_memory,
                "source": m.source,
                "tags": m.tags,
                "description": m.description,
                "chat_template": m.chat_template,
                "supported_apis": supported_apis,
                "max_tokens": max_tokens,
                "quantization": quantization,
                "endpoint_ids": endpoint_ids,
                "canonical_name": canonical_name,
                "aliases": aliases,
            });
            data.push(obj);
        } else {
            let obj = json!({
                "id": model_id,
                "object": "model",
                "created": observed_now,
                "owned_by": owned_by_from_id(model_id),
                "lifecycle_status": LifecycleStatus::Registered,
                "download_progress": null,
                "ready": ready,
                "supported_apis": supported_apis,
                "max_tokens": max_tokens,
                "quantization": quantization,
                "endpoint_ids": endpoint_ids,
                "canonical_name": canonical_name,
                "aliases": aliases,
            });
            data.push(obj);
        }
    }

    // NOTE: かつて存在した "endpoint_model_apis を再走査する" ループは
    // available_models = endpoint_model_apis.keys() のため到達不能だった。
    // 上の available_models ループで全件カバー済み。

    // NOTE: SPEC-6cd7f960 FR-6により、登録済みだがオンラインエンドポイントにないモデルは
    // /v1/models に含めない（利用可能なモデルのみを返す）

    // クラウドプロバイダーのモデル一覧を追加（SPEC-996e37bf）

    let cloud_models = super::cloud_models::get_cached_models(&state.http_client).await;
    for cm in cloud_models {
        let obj = json!({
            "id": cm.id,
            "object": cm.object,
            "created": cm.created,
            "owned_by": cm.owned_by,
            // クラウドモデルはリモートで常に利用可能
            "lifecycle_status": LifecycleStatus::Registered,
            "download_progress": null,
            "ready": true,
            "supported_apis": vec!["chat_completions"],
            "max_tokens": null,
            "endpoint_ids": Vec::<String>::new(),
        });
        data.push(obj);
    }

    let body = json!({
        "object": "list",
        "data": data,
    });

    Ok((StatusCode::OK, Json(body)).into_response())
}

// NOTE: list_models_extended() は廃止されました。
// /v1/models に Azure OpenAI 形式の capabilities とダッシュボード拡張が統合されています。

/// GET /v1/models/:id - モデル詳細取得（Azure capabilities 形式）
///
/// SPEC-0f1de549: Endpoints APIで登録されたモデルも検索対象に含める
#[allow(deprecated)] // NodeRegistry migration in progress
pub async fn get_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Response, AppError> {
    use crate::types::endpoint::SupportedAPI;
    use std::collections::HashSet;

    let mut registered_map: HashMap<String, crate::registry::models::ModelInfo> = HashMap::new();
    for model in list_registered_models(&state.db_pool).await? {
        registered_map.insert(model.name.clone(), model);
    }

    // SPEC-0f1de549: エンドポイントのモデルとsupported_apisを取得
    let mut endpoint_model_apis: HashMap<String, HashSet<SupportedAPI>> = HashMap::new();
    {
        let registry = &state.endpoint_registry;
        let online_endpoints = registry.list_online().await;
        for ep in online_endpoints {
            if let Ok(models) = registry.list_models(ep.id).await {
                for model in models {
                    let apis = endpoint_model_apis
                        .entry(model.model_id.clone())
                        .or_default();
                    for api in model.supported_apis {
                        apis.insert(api);
                    }
                    apis.insert(SupportedAPI::Responses);
                }
            }
        }
    }

    let model = registered_map.remove(&model_id);
    let is_endpoint_model = endpoint_model_apis.contains_key(&model_id);

    if model.is_none() && !is_endpoint_model {
        // 404 を OpenAI 換算で返す
        let body = json!({
            "error": {
                "message": "The model does not exist",
                "type": "invalid_request_error",
                "param": "model",
                "code": "model_not_found"
            }
        });
        return Ok((StatusCode::NOT_FOUND, Json(body)).into_response());
    }

    // supported_apis: 登録モデルの capability 由来 / エンドポイント申告 を統合し as_str() 昇順で返す
    let supported_apis: Vec<String> =
        build_supported_apis(model.as_ref(), endpoint_model_apis.get(&model_id));
    let observed_now = chrono::Utc::now().timestamp();

    if let Some(model) = model {
        // Azure OpenAI 形式の capabilities (boolean object)
        let caps: ModelCapabilities = model.get_capabilities().into();
        let ready = is_endpoint_model;
        let lifecycle_status = if ready {
            LifecycleStatus::Registered
        } else {
            LifecycleStatus::Pending
        };
        let created = model
            .last_modified
            .map(|t| t.timestamp())
            .unwrap_or(observed_now);

        let body = json!({
            "id": model_id,
            "object": "model",
            "created": created,
            "owned_by": owned_by_from_id(&model_id),
            "capabilities": caps,
            // ダッシュボード用拡張フィールド
            "lifecycle_status": lifecycle_status,
            "ready": ready,
            // 追加メタデータ（ダッシュボード向け）
            "repo": model.repo,
            "filename": model.filename,
            "size_bytes": model.size,
            "required_memory_bytes": model.required_memory,
            "source": model.source,
            "tags": model.tags,
            "description": model.description,
            "chat_template": model.chat_template,
            "supported_apis": supported_apis,
        });

        return Ok((StatusCode::OK, Json(body)).into_response());
    }

    // エンドポイント専用モデル（メタデータなし）
    let body = json!({
        "id": model_id,
        "object": "model",
        "created": observed_now,
        "owned_by": owned_by_from_id(&model_id),
        "lifecycle_status": LifecycleStatus::Registered,
        "ready": is_endpoint_model,
        "supported_apis": supported_apis,
    });

    Ok((StatusCode::OK, Json(body)).into_response())
}

fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| validation_error("`model` field is required for OpenAI-compatible requests"))
}

/// モデル名を抽出し、未指定または空の場合はデフォルト値を使用
fn extract_model_with_default(payload: &Value, default: String) -> String {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or(default)
}

fn extract_stream(payload: &Value) -> bool {
    payload
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn payload_requires_image_input(payload: &Value) -> bool {
    let Some(messages) = payload.get("messages").and_then(|v| v.as_array()) else {
        return false;
    };

    for message in messages {
        let Some(parts) = message.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str());
            if matches!(part_type, Some("image_url" | "input_image")) {
                return true;
            }
        }
    }

    false
}

fn parse_cloud_model(model: &str) -> Option<(String, String)> {
    // Accept prefixes like "openai:foo", "google:bar", "anthropic:baz"
    let prefixes = ["openai:", "google:", "anthropic:", "ahtnorpic:"];
    for p in prefixes.iter() {
        if model.starts_with(p) {
            let rest = model.trim_start_matches(p);
            if rest.is_empty() {
                return None;
            }
            let provider = if *p == "ahtnorpic:" {
                "anthropic"
            } else {
                p.trim_end_matches(':')
            };
            return Some((provider.to_string(), rest.to_string()));
        }
    }
    None
}

/// クラウドプロバイダ用の仮想ノード情報を生成する
fn cloud_virtual_node(provider: &str) -> (Uuid, String, IpAddr) {
    // 仮想ノードIDはクラウドプロバイダごとに固定値
    let endpoint_id = match provider {
        "openai" => Uuid::parse_str("00000000-0000-0000-0000-00000000c001")
            .expect("static UUID string is valid"),
        "google" => Uuid::parse_str("00000000-0000-0000-0000-00000000c002")
            .expect("static UUID string is valid"),
        "anthropic" => Uuid::parse_str("00000000-0000-0000-0000-00000000c003")
            .expect("static UUID string is valid"),
        _ => Uuid::parse_str("00000000-0000-0000-0000-00000000c0ff")
            .expect("static UUID string is valid"),
    };
    let machine_name = format!("cloud:{provider}");
    (endpoint_id, machine_name, UNSPECIFIED_IP)
}

#[allow(clippy::too_many_arguments)]
async fn proxy_openai_cloud_post(
    state: &AppState,
    _target_path: &str,
    model: &str,
    stream: bool,
    payload: Value,
    request_type: RequestType,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    let (provider, model_name) = parse_cloud_model(model)
        .ok_or_else(|| validation_error("cloud model prefix is invalid"))?;
    let (endpoint_id, endpoint_name, endpoint_ip) = cloud_virtual_node(&provider);
    let request_body = sanitize_openai_payload_for_history(&payload);
    let started = Instant::now();

    let cloud_provider = resolve_provider(provider.as_str())
        .ok_or_else(|| validation_error("unsupported cloud provider prefix"))?;
    let outcome = match proxy_cloud_provider(
        cloud_provider.as_ref(),
        &state.http_client,
        &payload,
        &model_name,
        stream,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            let duration = started.elapsed();
            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_ip,
                    model.to_string(),
                    request_type,
                    request_body,
                    StatusCode::BAD_GATEWAY,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: format!("{e:?}"),
                };
                save_request_record(state.request_history.clone(), record);
            }
            return Err(e);
        }
    };

    let duration = started.elapsed();
    let status = outcome.status;
    {
        let mut record = RequestResponseRecord::new(
            endpoint_id,
            endpoint_name,
            endpoint_ip,
            model.to_string(),
            request_type,
            request_body,
            status,
            duration,
            client_ip,
            api_key_id,
        );
        if !status.is_success() {
            record.status = RecordStatus::Error {
                message: outcome
                    .error_message
                    .clone()
                    .unwrap_or_else(|| status.to_string()),
            };
        }
        if status.is_success() {
            record.response_body = outcome.response_body.clone();
        }
        save_request_record(state.request_history.clone(), record);
    }

    Ok(outcome.response)
}

#[allow(deprecated)] // NodeRegistry migration in progress
#[allow(clippy::too_many_arguments)]
async fn proxy_openai_post(
    state: &AppState,
    payload: Value,
    target_path: &str,
    model: String,
    stream: bool,
    request_type: RequestType,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    // Cloud-prefixed model -> forward to provider API
    if parse_cloud_model(&model).is_some() {
        return proxy_openai_cloud_post(
            state,
            target_path,
            &model,
            stream,
            payload,
            request_type,
            client_ip,
            api_key_id,
        )
        .await;
    }

    // モデル名統一化: エイリアス名が渡された場合、正規名に変換して検索
    let resolved_model = {
        let found = state.endpoint_registry.find_by_model(&model).await;
        if found.is_empty() {
            // エイリアス名で見つからない場合、マッピングテーブルで正規名を解決
            if let Some(canonical) = crate::models::mapping::resolve_canonical_any(&model) {
                let canonical_found = state.endpoint_registry.find_by_model(canonical).await;
                if !canonical_found.is_empty() {
                    canonical.to_string()
                } else {
                    model.clone()
                }
            } else {
                model.clone()
            }
        } else {
            model.clone()
        }
    };

    // Check if any endpoint has this model
    if state
        .endpoint_registry
        .find_by_model(&resolved_model)
        .await
        .is_empty()
    {
        let is_registered = load_registered_model(&state.db_pool, &resolved_model).await?;
        if is_registered.is_none() {
            let message = format!("The model '{}' does not exist", model);
            return Ok(openai_error_response(message, StatusCode::NOT_FOUND));
        }
    }

    let request_body = sanitize_openai_payload_for_history(&payload);
    let tps_api_kind = TpsApiKind::from_request_type(request_type);
    let required_supported_api = if payload_requires_image_input(&payload) {
        Some(SupportedAPI::ImageInput)
    } else {
        None
    };
    let queue_config = state.queue_config;
    let mut queued_wait_ms: Option<u128> = None;

    // FR-004: エンドポイント選択失敗時もリクエスト履歴に記録する
    let endpoint = match if let Some(required_api) = required_supported_api {
        select_available_endpoint_with_queue_for_model_and_api(
            state,
            queue_config,
            &resolved_model,
            required_api,
            tps_api_kind,
        )
        .await
    } else {
        select_available_endpoint_with_queue_for_model(
            state,
            queue_config,
            &resolved_model,
            tps_api_kind,
        )
        .await
    } {
        Ok(QueueSelection::Ready {
            endpoint,
            queued_wait_ms: wait_ms,
        }) => {
            queued_wait_ms = wait_ms;
            *endpoint
        }
        Ok(QueueSelection::CapacityExceeded) => {
            let message = "Request queue is full".to_string();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    message.clone(),
                    0,
                    client_ip,
                    api_key_id,
                ),
            );
            let retry_after = queue_config.timeout.as_secs().max(1);
            return Ok(queue_error_response(
                StatusCode::TOO_MANY_REQUESTS,
                &message,
                "rate_limit_exceeded",
                Some(retry_after),
            ));
        }
        Ok(QueueSelection::Timeout { waited_ms }) => {
            let message = "Queue wait timeout".to_string();
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    message.clone(),
                    waited_ms as u64,
                    client_ip,
                    api_key_id,
                ),
            );
            return Ok(queue_error_response(
                StatusCode::GATEWAY_TIMEOUT,
                &message,
                "timeout",
                None,
            ));
        }
        Err(e) => {
            let error_message = if matches!(e, LbError::NoCapableEndpoints(_)) {
                format!("No available nodes support model: {}", model)
            } else {
                format!("Node selection failed: {}", e)
            };
            error!(
                endpoint = %target_path,
                model = %model,
                error = %e,
                "Failed to select available node"
            );
            save_request_record(
                state.request_history.clone(),
                RequestResponseRecord::error(
                    model.clone(),
                    request_type,
                    request_body,
                    error_message.clone(),
                    queued_wait_ms.unwrap_or(0) as u64,
                    client_ip,
                    api_key_id,
                ),
            );
            if matches!(e, LbError::NoCapableEndpoints(_)) {
                return Ok(model_unavailable_response(
                    error_message,
                    "no_capable_nodes",
                ));
            }
            return Err(e.into());
        }
    };
    let endpoint_id = endpoint.id;
    let endpoint_name = endpoint.name.clone();
    let endpoint_type = endpoint.endpoint_type;
    // RequestResponseRecordの互換性のため、デフォルトIP使用
    // (今後、RequestResponseRecordのフィールドをリネームすべき)
    let endpoint_host: std::net::IpAddr = UNSPECIFIED_IP;

    let request_lease = state
        .load_manager
        .begin_request(endpoint_id)
        .await
        .map_err(AppError::from)?;

    let client = state.http_client.clone();
    let runtime_url = format!("{}{}", endpoint.base_url.trim_end_matches('/'), target_path);
    let start = Instant::now();
    let endpoint_models = match state.endpoint_registry.list_models(endpoint_id).await {
        Ok(models) => models,
        Err(error) => {
            warn!(
                endpoint_id = %endpoint_id,
                model = %resolved_model,
                error = %error,
                "Failed to load endpoint models for request rewrite; falling back to static mapping"
            );
            Vec::new()
        }
    };
    let outbound_payload = rewrite_payload_model_for_endpoint(
        payload,
        &resolved_model,
        &endpoint_type,
        &endpoint_models,
    );

    let upstream_model = state
        .endpoint_registry
        .list_models(endpoint_id)
        .await
        .ok()
        .and_then(|endpoint_models| {
            endpoint_models
                .into_iter()
                .find(|endpoint_model| {
                    endpoint_model.model_id == model
                        || endpoint_model.model_id == resolved_model
                        || endpoint_model.canonical_name.as_deref() == Some(model.as_str())
                        || endpoint_model.canonical_name.as_deref() == Some(resolved_model.as_str())
                })
                .map(|endpoint_model| endpoint_model.model_id)
        })
        .or_else(|| {
            crate::models::mapping::resolve_engine_name(&model, &endpoint_type).map(str::to_string)
        })
        .or_else(|| {
            crate::models::mapping::resolve_engine_name(&resolved_model, &endpoint_type)
                .map(str::to_string)
        })
        .unwrap_or_else(|| resolved_model.clone());

    let mut upstream_payload = outbound_payload;
    if let Some(payload_object) = upstream_payload.as_object_mut() {
        payload_object.insert("model".to_string(), Value::String(upstream_model.clone()));

        // SPEC-8c32349f: ストリーミングリクエストに stream_options.include_usage を注入
        // Ollama 等のエンドポイントが最終チャンクに usage を含めるようにする
        if stream {
            if let Some(opts) = payload_object
                .entry("stream_options".to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()
            {
                opts.entry("include_usage".to_string())
                    .or_insert(json!(true));
            }
        }
    }

    let mut request_builder = client
        .post(&runtime_url)
        .timeout(std::time::Duration::from_secs(
            endpoint.inference_timeout_secs as u64,
        ))
        .json(&upstream_payload);
    if let Some(api_key) = &endpoint.api_key {
        request_builder = request_builder.bearer_auth(api_key);
    }

    let response = match request_builder.send().await {
        Ok(res) => res,
        Err(e) => {
            let duration = start.elapsed();
            let ollama_loading_model = if e.is_timeout()
                && endpoint_type == crate::types::endpoint::EndpointType::Ollama
            {
                match probe_ollama_model_loaded(
                    &client,
                    &endpoint.base_url,
                    endpoint.api_key.as_deref(),
                    &upstream_model,
                )
                .await
                {
                    Some(false) => Some(upstream_model.clone()),
                    _ => None,
                }
            } else {
                None
            };
            let classified_error = classify_upstream_request_error(
                &e,
                &runtime_url,
                endpoint.inference_timeout_secs,
                ollama_loading_model.as_deref(),
            );
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            // Note: Model exclusion is handled by the health check system
            // which will mark the endpoint as offline/error if requests fail repeatedly

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name.clone(),
                    endpoint_host,
                    model.clone(),
                    request_type,
                    request_body.clone(),
                    classified_error.status_code,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: classified_error.record_message,
                };
                save_request_record(state.request_history.clone(), record);
            }

            let mut response = openai_error_response_with_type(
                classified_error.client_message,
                classified_error.error_type,
                classified_error.status_code,
            );
            if let Some(wait_ms) = queued_wait_ms {
                add_queue_headers(&mut response, wait_ms);
            }
            return Ok(response);
        }
    };

    // ストリームの場合はレスポンスをそのままパススルー
    if stream {
        let duration = start.elapsed();
        let succeeded = response.status().is_success();
        let outcome = if succeeded {
            RequestOutcome::Success
        } else {
            RequestOutcome::Error
        };
        request_lease
            .complete(outcome, duration)
            .await
            .map_err(AppError::from)?;
        if succeeded {
            // SPEC-f8e3a1b7: 成功時に推論レイテンシを更新
            update_inference_latency(&state.endpoint_registry, endpoint_id, duration);
        } else {
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );
        }

        let mut axum_response = if succeeded {
            {
                let record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name.clone(),
                    endpoint_host,
                    model.clone(),
                    request_type,
                    request_body.clone(),
                    response.status(),
                    duration,
                    client_ip,
                    api_key_id,
                );
                save_request_record(state.request_history.clone(), record);
            }
            forward_streaming_response_with_tps_tracking(
                response,
                endpoint_id,
                model.clone(),
                tps_api_kind,
                endpoint_type,
                start,
                state.endpoint_registry.clone(),
                state.load_manager.clone(),
                state.event_bus.clone(),
            )
            .map_err(AppError::from)?
        } else {
            let status = response.status();
            let headers = response.headers().clone();
            let body_bytes = response.bytes().await.unwrap_or_default();
            let message = upstream_error_message_from_bytes(status, &body_bytes);

            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                endpoint_host,
                model.clone(),
                request_type,
                request_body.clone(),
                status,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);

            let mut response = Response::new(Body::from(body_bytes));
            *response.status_mut() = status;
            {
                let response_headers = response.headers_mut();
                for (name, value) in headers.iter() {
                    if let (Ok(header_name), Ok(header_value)) = (
                        HeaderName::from_bytes(name.as_str().as_bytes()),
                        HeaderValue::from_bytes(value.as_bytes()),
                    ) {
                        response_headers.insert(header_name, header_value);
                    }
                }
            }
            if !response
                .headers()
                .contains_key(axum::http::header::CONTENT_TYPE)
            {
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
            response
        };
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut axum_response, wait_ms);
        }
        return Ok(axum_response);
    }

    if !response.status().is_success() {
        let duration = start.elapsed();
        request_lease
            .complete(RequestOutcome::Error, duration)
            .await
            .map_err(AppError::from)?;
        record_endpoint_request_stats(
            state.endpoint_registry.clone(),
            endpoint_id,
            model.clone(),
            false,
            0,
            0,
            tps_api_kind,
            endpoint_type,
            state.load_manager.clone(),
            state.event_bus.clone(),
        );

        // Note: Model exclusion is handled by the health check system
        // which will mark the endpoint as offline/error if requests fail repeatedly

        let status = response.status();
        // OpenAI互換経路では upstream 非2xx は 502 に正規化して返す
        let status_code = StatusCode::BAD_GATEWAY;
        let body_bytes = response.bytes().await.unwrap_or_default();
        let message = upstream_error_message_from_bytes(status, &body_bytes);

        {
            let mut record = RequestResponseRecord::new(
                endpoint_id,
                endpoint_name.clone(),
                endpoint_host,
                model.clone(),
                request_type,
                request_body.clone(),
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: message.clone(),
            };
            save_request_record(state.request_history.clone(), record);
        }

        let payload = json!({
            "error": {
                "message": message,
                "type": "endpoint_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        let mut response = (status_code, Json(payload)).into_response();
        if let Some(wait_ms) = queued_wait_ms {
            add_queue_headers(&mut response, wait_ms);
        }
        return Ok(response);
    }

    let parsed = response.json::<Value>().await;
    let duration = start.elapsed();

    match parsed {
        Ok(mut body) => {
            if let Some(body_object) = body.as_object_mut() {
                body_object.insert("model".to_string(), Value::String(model.clone()));
            }

            // レスポンスからトークン使用量を抽出
            let token_usage = extract_usage_from_response(&body);

            request_lease
                .complete_with_tokens(RequestOutcome::Success, duration, token_usage.clone())
                .await
                .map_err(AppError::from)?;
            // SPEC-f8e3a1b7: 成功時に推論レイテンシを更新
            update_inference_latency(&state.endpoint_registry, endpoint_id, duration);
            // SPEC-4bb5b55f: TPS計測用にoutput_tokensとdurationを渡す
            let tps_output_tokens = token_usage
                .as_ref()
                .and_then(|u| u.output_tokens)
                .unwrap_or(0) as u64;
            let tps_duration_ms = if tps_output_tokens > 0 {
                duration.as_millis().max(1) as u64
            } else {
                0
            };
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                true,
                tps_output_tokens,
                tps_duration_ms,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            // RequestResponseRecordにトークン情報を保存
            let (input_tokens, output_tokens, total_tokens) = token_usage
                .as_ref()
                .map(|u| (u.input_tokens, u.output_tokens, u.total_tokens))
                .unwrap_or((None, None, None));

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_host,
                    model,
                    request_type,
                    request_body,
                    StatusCode::OK,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.response_body = Some(body.clone());
                record.input_tokens = input_tokens;
                record.output_tokens = output_tokens;
                record.total_tokens = total_tokens;
                save_request_record(state.request_history.clone(), record);
            }

            let mut response = (StatusCode::OK, Json(body)).into_response();
            if let Some(wait_ms) = queued_wait_ms {
                add_queue_headers(&mut response, wait_ms);
            }
            Ok(response)
        }
        Err(e) => {
            request_lease
                .complete(RequestOutcome::Error, duration)
                .await
                .map_err(AppError::from)?;
            record_endpoint_request_stats(
                state.endpoint_registry.clone(),
                endpoint_id,
                model.clone(),
                false,
                0,
                0,
                tps_api_kind,
                endpoint_type,
                state.load_manager.clone(),
                state.event_bus.clone(),
            );

            // Note: Model exclusion is handled by the health check system
            // which will mark the endpoint as offline/error if requests fail repeatedly

            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_host,
                    model,
                    request_type,
                    request_body,
                    StatusCode::BAD_GATEWAY,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: format!("Failed to parse OpenAI response: {}", e),
                };
                save_request_record(state.request_history.clone(), record);
            }

            Err(LbError::Http(format!("Failed to parse OpenAI response: {}", e)).into())
        }
    }
}

#[allow(dead_code)]
async fn proxy_openai_get(state: &AppState, target_path: &str) -> Result<Response, AppError> {
    let endpoint = select_available_endpoint(state).await?;
    let endpoint_id = endpoint.id;

    let request_lease = state
        .load_manager
        .begin_request(endpoint_id)
        .await
        .map_err(AppError::from)?;

    let client = state.http_client.clone();
    let runtime_url = format!("{}{}", endpoint.base_url.trim_end_matches('/'), target_path);
    let start = Instant::now();

    let response = client.get(&runtime_url).send().await.map_err(|e| {
        AppError::from(LbError::Http(format!(
            "Failed to proxy OpenAI models request: {}",
            e
        )))
    })?;

    let duration = start.elapsed();
    let outcome = if response.status().is_success() {
        RequestOutcome::Success
    } else {
        RequestOutcome::Error
    };
    request_lease
        .complete(outcome, duration)
        .await
        .map_err(AppError::from)?;

    if !response.status().is_success() {
        let status = response.status();
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body_bytes = response.bytes().await.unwrap_or_default();
        let message = if body_bytes.is_empty() {
            status.to_string()
        } else {
            String::from_utf8_lossy(&body_bytes).trim().to_string()
        };

        let payload = json!({
            "error": {
                "message": message,
                "type": "node_upstream_error",
                "code": status_code.as_u16(),
            }
        });

        return Ok((status_code, Json(payload)).into_response());
    }

    let body = response.json::<Value>().await.map_err(|e| {
        AppError::from(LbError::Http(format!(
            "Failed to parse OpenAI models response: {}",
            e
        )))
    })?;

    Ok((StatusCode::OK, Json(body)).into_response())
}

fn validation_error(message: impl Into<String>) -> AppError {
    let err = LbError::Common(CommonError::Validation(message.into()));
    err.into()
}

#[cfg(test)]
mod tests {
    use super::{
        extract_client_ip_from_headers, parse_client_ip_from_forwarded_value, parse_cloud_model,
        proxy_openai_cloud_post, proxy_openai_post,
    };
    use crate::common::protocol::{RecordStatus, RequestType};
    use crate::{
        db::test_utils::{TestAppStateBuilder, TEST_LOCK},
        AppState,
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::{body::to_bytes, Json};
    use serde_json::json;
    use serial_test::serial;
    use std::net::{IpAddr, SocketAddr};
    use tempfile::tempdir;
    use tokio::time::{sleep, Duration};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn create_local_state() -> AppState {
        TestAppStateBuilder::new().await.build().await
    }

    async fn create_state_with_tempdir() -> (AppState, tempfile::TempDir) {
        let dir = tempdir().expect("temp dir");
        std::env::set_var("LLMLB_DATA_DIR", dir.path());
        let state = create_local_state().await;
        (state, dir)
    }

    async fn add_online_chat_endpoint(
        state: &AppState,
        endpoint_name: &str,
        base_url: String,
        model_id: &str,
        inference_timeout_secs: u32,
    ) -> uuid::Uuid {
        add_online_chat_endpoint_with_type(
            state,
            endpoint_name,
            base_url,
            model_id,
            inference_timeout_secs,
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        )
        .await
    }

    async fn add_online_chat_endpoint_with_type(
        state: &AppState,
        endpoint_name: &str,
        base_url: String,
        model_id: &str,
        inference_timeout_secs: u32,
        endpoint_type: crate::types::endpoint::EndpointType,
    ) -> uuid::Uuid {
        use crate::types::endpoint::SupportedAPI;

        add_online_chat_endpoint_with_supported_apis(
            state,
            endpoint_name,
            base_url,
            model_id,
            inference_timeout_secs,
            endpoint_type,
            vec![SupportedAPI::ChatCompletions],
        )
        .await
    }

    async fn add_online_chat_endpoint_with_supported_apis(
        state: &AppState,
        endpoint_name: &str,
        base_url: String,
        model_id: &str,
        inference_timeout_secs: u32,
        endpoint_type: crate::types::endpoint::EndpointType,
        supported_apis: Vec<crate::types::endpoint::SupportedAPI>,
    ) -> uuid::Uuid {
        use crate::types::endpoint::{Endpoint, EndpointModel, EndpointStatus};

        let mut endpoint = Endpoint::new(endpoint_name.to_string(), base_url, endpoint_type);
        endpoint.status = EndpointStatus::Online;
        endpoint.inference_timeout_secs = inference_timeout_secs;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: model_id.to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis,
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");
        endpoint_id
    }

    #[test]
    fn parse_cloud_prefixes() {
        assert_eq!(
            parse_cloud_model("openai:gpt-4o"),
            Some(("openai".to_string(), "gpt-4o".to_string()))
        );
        assert_eq!(
            parse_cloud_model("google:gemini-pro"),
            Some(("google".to_string(), "gemini-pro".to_string()))
        );
        assert_eq!(
            parse_cloud_model("ahtnorpic:claude-3"),
            Some(("anthropic".to_string(), "claude-3".to_string()))
        );
        assert_eq!(parse_cloud_model("gpt-4"), None);
        assert_eq!(parse_cloud_model("openai:"), None);
    }

    #[test]
    fn parse_client_ip_from_forwarded_value_supports_bracketed_ipv6_with_port() {
        let parsed = parse_client_ip_from_forwarded_value("\"[2001:db8::7]:4711\"")
            .expect("must parse bracketed ipv6");
        assert_eq!(parsed, "2001:db8::7".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_from_headers_prefers_first_valid_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, 203.0.113.10, 10.0.0.1"),
        );
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=198.51.100.20;proto=https"),
        );

        let parsed = extract_client_ip_from_headers(&headers).expect("must parse x-forwarded-for");
        assert_eq!(parsed, "203.0.113.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_from_headers_falls_back_to_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown;proto=https, for=\"[2001:db8::11]:8443\""),
        );

        let parsed = extract_client_ip_from_headers(&headers).expect("must parse forwarded");
        assert_eq!(parsed, "2001:db8::11".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    #[serial]
    async fn openai_prefix_requires_api_key() {
        let _guard = TEST_LOCK.lock().await;
        // Save and remove any existing API key to test error case
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            false,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("OPENAI_API_KEY"),
            "expected error mentioning OPENAI_API_KEY, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn google_prefix_requires_api_key() {
        let _guard = TEST_LOCK.lock().await;
        // Save and remove any existing API key to test error case
        let saved = std::env::var("GOOGLE_API_KEY").ok();
        std::env::remove_var("GOOGLE_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"google:gemini-pro","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "google:gemini-pro",
            false,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("GOOGLE_API_KEY"),
            "expected GOOGLE_API_KEY error, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("GOOGLE_API_KEY", key);
        }
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn anthropic_prefix_requires_api_key() {
        let _guard = TEST_LOCK.lock().await;
        // Save and remove any existing API key to test error case
        let saved = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"anthropic:claude-3","messages":[]});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "anthropic:claude-3",
            false,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("ANTHROPIC_API_KEY"),
            "expected ANTHROPIC_API_KEY error, got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn openai_prefix_streams_via_cloud() {
        let _guard = TEST_LOCK.lock().await;
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                "text/event-stream",
            );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("OPENAI_API_KEY", "testkey");
        std::env::set_var("OPENAI_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[],"stream":true});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            true,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("cloud stream response");
        let body = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("delta"));
        assert!(body_str.contains("hi"));

        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn google_prefix_proxies_and_maps_response() {
        let _guard = TEST_LOCK.lock().await;
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "hello from gemini"}]}}]
        }));
        Mock::given(method("POST"))
            .and(path("/models/gemini-pro:generateContent"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("GOOGLE_API_KEY", "gkey");
        std::env::set_var("GOOGLE_API_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload =
            json!({"model":"google:gemini-pro","messages":[{"role":"user","content":"hi"}]});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "google:gemini-pro",
            false,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("google mapped response");
        let bytes = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["model"].as_str().unwrap(), "google:gemini-pro");
        assert_eq!(
            v["choices"][0]["message"]["content"].as_str().unwrap(),
            "hello from gemini"
        );

        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("GOOGLE_API_BASE_URL");
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn anthropic_prefix_proxies_and_maps_response() {
        let _guard = TEST_LOCK.lock().await;
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
                "id": "abc123",
                "model": "claude-3",
            "content": [{"text": "anthropic says hi"}]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("ANTHROPIC_API_KEY", "akey");
        std::env::set_var("ANTHROPIC_API_BASE_URL", server.uri());
        let (state, _dir) = create_state_with_tempdir().await;

        let payload =
            json!({"model":"anthropic:claude-3","messages":[{"role":"user","content":"hi"}]});
        let resp = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "anthropic:claude-3",
            false,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("anthropic mapped response");
        let bytes = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["model"].as_str().unwrap(), "anthropic:claude-3");
        assert_eq!(
            v["choices"][0]["message"]["content"].as_str().unwrap(),
            "anthropic says hi"
        );

        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_API_BASE_URL");
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn chat_image_input_routes_to_image_input_endpoint() {
        use crate::types::endpoint::{EndpointType, SupportedAPI};
        use axum::extract::{ConnectInfo, State};

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let text_only_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": {
                    "message": "text-only endpoint should not receive image input"
                }
            })))
            .mount(&text_only_server)
            .await;

        let image_input_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-image-input",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "vision-ok"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&image_input_server)
            .await;

        add_online_chat_endpoint_with_supported_apis(
            &state,
            "text-only-endpoint",
            text_only_server.uri(),
            "vision-model",
            60,
            EndpointType::LmStudio,
            vec![SupportedAPI::ChatCompletions],
        )
        .await;
        add_online_chat_endpoint_with_supported_apis(
            &state,
            "image-input-endpoint",
            image_input_server.uri(),
            "vision-model",
            60,
            EndpointType::LmStudio,
            vec![SupportedAPI::ChatCompletions, SupportedAPI::ImageInput],
        )
        .await;

        let response = super::chat_completions(
            ConnectInfo("127.0.0.1:0".parse::<SocketAddr>().unwrap()),
            HeaderMap::new(),
            State(state),
            None,
            Json(json!({
                "model": "vision-model",
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Describe this image briefly."},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}
                    ]
                }],
                "stream": false
            })),
        )
        .await
        .expect("image input chat response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("response body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
        assert_eq!(
            body["choices"][0]["message"]["content"].as_str(),
            Some("vision-ok")
        );

        assert_eq!(
            text_only_server
                .received_requests()
                .await
                .expect("text-only requests")
                .len(),
            0
        );
        assert_eq!(
            image_input_server
                .received_requests()
                .await
                .expect("image input requests")
                .len(),
            1
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn cloud_request_is_recorded_in_history() {
        let _guard = TEST_LOCK.lock().await;
        let temp_dir = tempdir().expect("temp dir");
        std::env::set_var("LLMLB_DATA_DIR", temp_dir.path());

        let state = create_local_state().await;
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        std::env::set_var("OPENAI_API_KEY", "testkey");
        std::env::set_var("OPENAI_BASE_URL", server.uri());

        let payload = json!({"model":"openai:gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":false});
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "openai:gpt-4o".into(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("cloud proxy succeeds");

        assert_eq!(response.status(), StatusCode::OK);
        sleep(Duration::from_millis(20)).await;

        let records = state.request_history.load_records().await.expect("records");
        assert_eq!(records.len(), 1, "cloud request should be recorded");

        let record = &records[0];
        assert_eq!(record.model, "openai:gpt-4o");
        assert!(matches!(record.status, RecordStatus::Success));
        assert_eq!(record.request_type, RequestType::Chat);
        assert!(
            record.response_body.is_some(),
            "response should be captured"
        );

        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    #[tokio::test]
    #[serial]
    async fn cloud_request_is_listed_in_dashboard_history() {
        use axum::routing::Router;
        use std::net::SocketAddr;
        use tokio::net::TcpListener;

        let _guard = TEST_LOCK.lock().await;

        // mock cloud provider
        let server = MockServer::start().await;
        let tmpl = ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-dashboard",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello cloud"},
                "finish_reason": "stop"
            }]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        // lb state with temp data dir
        std::env::set_var("OPENAI_API_KEY", "testkey");
        std::env::set_var("OPENAI_BASE_URL", server.uri());
        let (state, dir) = create_state_with_tempdir().await;

        // spawn lb
        let app: Router = crate::api::create_app(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .ok();
        });

        // send cloud request
        let client = reqwest::Client::new();
        let payload = json!({"model":"openai:gpt-4o","messages":[{"role":"user","content":"hi"}]});
        let resp = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .header("x-api-key", "sk_debug")
            .json(&payload)
            .send()
            .await
            .expect("send cloud request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        // wait for async save_request_record
        tokio::time::sleep(Duration::from_millis(50)).await;

        // login (dashboard is JWT-only; API keys are rejected for /api/dashboard/*)
        let login_resp = client
            .post(format!("http://{addr}/api/auth/login"))
            .header("content-type", "application/json")
            .json(&json!({"username":"admin","password":"test"}))
            .send()
            .await
            .expect("login request");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let login_body: serde_json::Value = login_resp.json().await.expect("login json");
        let jwt_token = login_body["token"].as_str().expect("login token");

        // fetch dashboard history
        let history_resp = client
            .get(format!("http://{addr}/api/dashboard/request-responses"))
            .header("authorization", format!("Bearer {jwt_token}"))
            .send()
            .await
            .expect("history request");
        assert_eq!(history_resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = history_resp.json().await.expect("history json");
        let records = body["records"].as_array().expect("records array");
        assert!(
            records.iter().any(|r| r["model"] == "openai:gpt-4o"),
            "cloud request should be listed in history"
        );

        // cleanup env
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_BASE_URL");
        std::env::remove_var("LLMLB_DATA_DIR");
        drop(dir);
    }

    #[tokio::test]
    #[serial]
    async fn non_prefixed_model_stays_on_local_path() {
        let _guard = TEST_LOCK.lock().await;
        let state = create_local_state().await;
        let payload = json!({"model":"gpt-oss-20b","messages":[]});
        let res = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "gpt-oss-20b".into(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await;
        // モデルが登録されておらず、どのノードも報告していない場合は404
        let response = res.expect("expected 404 response, not Err");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::NOT_FOUND,
            "expected NOT_FOUND for unregistered model"
        );
    }

    #[tokio::test]
    #[serial]
    async fn direct_routing_body_read_failure_releases_active_request() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut read_buf = [0u8; 4096];
                let _ = socket.read(&mut read_buf).await;
                // Intentionally send fewer bytes than Content-Length to force body read failure.
                let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 256\r\nConnection: close\r\n\r\n{\"id\":\"truncated\"}";
                let _ = socket.write_all(response).await;
                let _ = socket.shutdown().await;
            }
        });

        let mut endpoint = Endpoint::new(
            "broken-endpoint".to_string(),
            format!("http://{addr}"),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "broken-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "broken-model",
            "messages": [{"role":"user","content":"hello"}]
        });
        let result = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "broken-model".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await;

        assert!(
            result.is_err(),
            "expected upstream body read failure to return error"
        );

        let snapshot = state
            .load_manager
            .snapshot(endpoint_id)
            .await
            .expect("snapshot");
        assert_eq!(
            snapshot.active_requests, 0,
            "active request count must be released on body read error"
        );
    }

    #[tokio::test]
    #[serial]
    async fn upstream_timeout_returns_gateway_timeout_response() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        let delayed_body = json!({
            "id": "chatcmpl-timeout",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "slow"},
                "finish_reason": "stop"
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(delayed_body),
            )
            .mount(&server)
            .await;

        let endpoint_id =
            add_online_chat_endpoint(&state, "timeout-endpoint", server.uri(), "timeout-model", 1)
                .await;

        let response = proxy_openai_post(
            &state,
            json!({
                "model": "timeout-model",
                "messages": [{"role":"user","content":"hello"}]
            }),
            "/v1/chat/completions",
            "timeout-model".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("timeout should return response");

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("timeout body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("timeout json");
        let expected_message = format!(
            "Upstream request to {}/v1/chat/completions timed out after 1 seconds",
            server.uri()
        );
        assert_eq!(
            json["error"]["message"].as_str(),
            Some(expected_message.as_str())
        );
        assert_eq!(json["error"]["type"], "timeout");
        assert_eq!(json["error"]["code"], 504);

        tokio::time::sleep(Duration::from_millis(50)).await;
        let snapshot = state
            .load_manager
            .snapshot(endpoint_id)
            .await
            .expect("snapshot");
        assert_eq!(snapshot.active_requests, 0);

        let records = state.request_history.load_records().await.expect("records");
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].status, RecordStatus::Error { .. }));
    }

    #[tokio::test]
    #[serial]
    async fn canonical_model_routes_to_alias_backed_endpoint_and_rewrites_payload() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"model": "gpt-oss:20b"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-alias",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            })))
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "ollama-alias".to_string(),
            server.uri(),
            EndpointType::Ollama,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "gpt-oss:20b".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: Some("openai/gpt-oss-20b".to_string()),
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "openai/gpt-oss-20b",
            "messages": [{"role":"user","content":"hello"}]
        });
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "openai/gpt-oss-20b".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("canonical request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn ollama_cold_start_timeout_returns_model_loading_error() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": []
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(2))
                    .set_body_json(json!({
                        "id": "chatcmpl-cold-start",
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": "slow"},
                            "finish_reason": "stop"
                        }]
                    })),
            )
            .mount(&server)
            .await;

        add_online_chat_endpoint_with_type(
            &state,
            "ollama-loading-endpoint",
            server.uri(),
            "qwen3:30b",
            1,
            crate::types::endpoint::EndpointType::Ollama,
        )
        .await;

        let response = proxy_openai_post(
            &state,
            json!({
                "model": "qwen3:30b",
                "messages": [{"role":"user","content":"hello"}]
            }),
            "/v1/chat/completions",
            "qwen3:30b".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("ollama cold-start timeout should return response");

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("model loading body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("model loading json");
        assert_eq!(json["error"]["type"], "model_loading");
        assert!(
            json["error"]["message"]
                .as_str()
                .expect("model loading message")
                .contains("still loading"),
            "expected model loading hint in response body"
        );

        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/v1/chat/completions");
        assert_eq!(requests[1].url.path(), "/api/ps");
    }

    #[tokio::test]
    #[serial]
    async fn ollama_success_does_not_probe_load_state() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-success",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;

        add_online_chat_endpoint_with_type(
            &state,
            "ollama-success-endpoint",
            server.uri(),
            "qwen3:30b",
            5,
            crate::types::endpoint::EndpointType::Ollama,
        )
        .await;

        let response = proxy_openai_post(
            &state,
            json!({
                "model": "qwen3:30b",
                "messages": [{"role":"user","content":"hello"}]
            }),
            "/v1/chat/completions",
            "qwen3:30b".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("ollama success should return response");

        assert_eq!(response.status(), StatusCode::OK);

        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/v1/chat/completions");
    }

    #[tokio::test]
    #[serial]
    async fn canonical_model_routes_to_secondary_lm_studio_alias_and_rewrites_payload() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(
                json!({"model": "qwen/qwen3.5-35b-a3b:2"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-lmstudio-alias",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            })))
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "lmstudio-secondary-alias".to_string(),
            server.uri(),
            EndpointType::LmStudio,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "qwen/qwen3.5-35b-a3b:2".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: Some("Qwen/Qwen3.5-35B-A3B".to_string()),
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "Qwen/Qwen3.5-35B-A3B",
            "messages": [{"role":"user","content":"hello"}]
        });
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "Qwen/Qwen3.5-35B-A3B".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("canonical request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn upstream_connect_failure_returns_bad_gateway_response() {
        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        drop(listener);

        add_online_chat_endpoint(
            &state,
            "connect-failure-endpoint",
            format!("http://{addr}"),
            "connect-failure-model",
            5,
        )
        .await;

        let response = proxy_openai_post(
            &state,
            json!({
                "model": "connect-failure-model",
                "messages": [{"role":"user","content":"hello"}]
            }),
            "/v1/chat/completions",
            "connect-failure-model".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("connect failure should return response");

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("connect failure body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("connect failure json");
        let expected_message =
            format!("Failed to connect to upstream: http://{addr}/v1/chat/completions");
        assert_eq!(
            json["error"]["message"].as_str(),
            Some(expected_message.as_str())
        );
        assert_eq!(json["error"]["type"], "connection_error");
        assert_eq!(json["error"]["code"], 502);
    }

    #[tokio::test]
    #[serial]
    async fn local_streaming_request_updates_model_tps_after_stream_completion() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let tmpl = ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(stream_body, "text/event-stream");
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(tmpl)
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "stream-tps-endpoint".to_string(),
            server.uri(),
            EndpointType::Vllm,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "stream-tps-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "stream-tps-model",
            "messages": [{"role":"user","content":"hello"}],
            "stream": true
        });
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "stream-tps-model".to_string(),
            true,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("streaming request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("stream body should be readable");

        sleep(Duration::from_millis(100)).await;

        let tps = state.load_manager.get_model_tps(endpoint_id).await;
        let entry = tps
            .iter()
            .find(|info| info.model_id == "stream-tps-model")
            .expect("stream model should have TPS entry");
        assert!(entry.tps.is_some(), "TPS should be updated");
        assert!(
            entry.total_output_tokens > 0,
            "streaming output tokens should be accumulated"
        );
    }

    /// SPEC #575 US-013-A (2026-04-23 delta):
    /// llama.cpp タイプのエンドポイントに対するストリーミング chat completions が
    /// 正常にプロキシされ、TPS 計測が更新されることを確認する。
    #[tokio::test]
    #[serial]
    async fn llamacpp_streaming_request_updates_model_tps_after_stream_completion() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(stream_body, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "llamacpp-stream-tps-endpoint".to_string(),
            server.uri(),
            EndpointType::Llamacpp,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "llamacpp-stream-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "llamacpp-stream-model",
            "messages": [{"role":"user","content":"hello"}],
            "stream": true
        });
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "llamacpp-stream-model".to_string(),
            true,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("streaming request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1_000_000)
            .await
            .expect("stream body should be readable");

        sleep(Duration::from_millis(100)).await;

        let tps = state.load_manager.get_model_tps(endpoint_id).await;
        let entry = tps
            .iter()
            .find(|info| info.model_id == "llamacpp-stream-model")
            .expect("llamacpp stream model should have TPS entry");
        assert!(entry.tps.is_some(), "TPS should be updated for llamacpp");
        assert!(
            entry.total_output_tokens > 0,
            "streaming output tokens should be accumulated for llamacpp"
        );
    }

    #[tokio::test]
    #[serial]
    async fn interrupted_streaming_request_still_records_success_stats() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        let stream_body = concat!(
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(stream_body, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "stream-interrupted-endpoint".to_string(),
            server.uri(),
            EndpointType::Vllm,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "stream-interrupted-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");

        let response = proxy_openai_post(
            &state,
            json!({
                "model": "stream-interrupted-model",
                "messages": [{"role":"user","content":"hello"}],
                "stream": true
            }),
            "/v1/chat/completions",
            "stream-interrupted-model".to_string(),
            true,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("streaming request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        // Simulate client disconnect before fully draining the upstream stream.
        drop(response);

        sleep(Duration::from_millis(120)).await;

        let endpoint = crate::db::endpoints::get_endpoint(&state.db_pool, endpoint_id)
            .await
            .expect("get endpoint should succeed")
            .expect("endpoint should exist");
        assert_eq!(endpoint.total_requests, 1);
        assert_eq!(endpoint.successful_requests, 1);
        assert_eq!(endpoint.failed_requests, 0);

        let model_stats =
            crate::db::endpoint_daily_stats::get_model_stats(&state.db_pool, endpoint_id)
                .await
                .expect("get model stats");
        let stat = model_stats
            .iter()
            .find(|s| s.model_id == "stream-interrupted-model")
            .expect("model stats should exist for interrupted stream");
        assert_eq!(stat.total_requests, 1);
        assert_eq!(stat.successful_requests, 1);
        assert_eq!(stat.failed_requests, 0);
    }

    #[tokio::test]
    #[serial]
    async fn non_stream_without_usage_does_not_accumulate_tps_duration() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let server = MockServer::start().await;
        let body_without_usage = json!({
            "id": "chatcmpl-no-usage",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "hello"},
                "finish_reason": "stop"
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body_without_usage))
            .mount(&server)
            .await;

        let mut endpoint = Endpoint::new(
            "no-usage-endpoint".to_string(),
            server.uri(),
            EndpointType::Vllm,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: "no-usage-model".to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");

        let payload = json!({
            "model": "no-usage-model",
            "messages": [{"role":"user","content":"hello"}],
            "stream": false
        });
        let response = proxy_openai_post(
            &state,
            payload,
            "/v1/chat/completions",
            "no-usage-model".to_string(),
            false,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .expect("request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        sleep(Duration::from_millis(100)).await;

        let model_stats =
            crate::db::endpoint_daily_stats::get_model_stats(&state.db_pool, endpoint_id)
                .await
                .expect("get model stats");
        let stat = model_stats
            .iter()
            .find(|s| s.model_id == "no-usage-model")
            .expect("model stats should exist");
        assert_eq!(
            stat.total_output_tokens, 0,
            "usageがない場合はoutput_tokensを加算しない"
        );
        assert_eq!(
            stat.total_duration_ms, 0,
            "usageがない場合はduration_msを加算しない"
        );
    }

    #[tokio::test]
    #[serial]
    async fn streaming_allowed_for_cloud_prefix() {
        let _guard = TEST_LOCK.lock().await;
        // Save and remove any existing API key to test error case
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");
        let (state, _dir) = create_state_with_tempdir().await;

        let payload = json!({"model":"openai:gpt-4o","messages":[],"stream":true});
        let err = proxy_openai_cloud_post(
            &state,
            "/v1/chat/completions",
            "openai:gpt-4o",
            true,
            payload,
            RequestType::Chat,
            None,
            None,
        )
        .await
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("OPENAI_API_KEY"),
            "expected API key error (stream path), got {}",
            msg
        );

        // Restore API key if it was set
        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    // T006: chat capabilities検証テスト (RED)
    // TextGeneration capability を持たないモデルで /v1/chat/completions を呼ぶとエラー
    #[test]
    fn test_chat_capability_validation_error_message() {
        use crate::types::model::{ModelCapability, ModelType};

        // TTSモデルはTextToSpeechのみ、TextGenerationは非対応
        let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
        assert!(!tts_caps.contains(&ModelCapability::TextGeneration));

        // ASRモデルもSpeechToTextのみ、TextGenerationは非対応
        let stt_caps = ModelCapability::from_model_type(ModelType::SpeechToText);
        assert!(!stt_caps.contains(&ModelCapability::TextGeneration));

        // EmbeddingモデルもEmbeddingのみ、TextGenerationは非対応
        let embed_caps = ModelCapability::from_model_type(ModelType::Embedding);
        assert!(!embed_caps.contains(&ModelCapability::TextGeneration));

        // 期待されるエラーメッセージ形式
        let model_name = "whisper-large-v3";
        let expected_error = format!("Model '{}' does not support text generation", model_name);
        assert!(expected_error.contains("does not support text generation"));
    }

    // ===== extract_model tests =====

    #[test]
    fn extract_model_returns_model_string() {
        use super::extract_model;
        let payload = json!({"model": "llama-3-8b", "messages": []});
        let result = extract_model(&payload).unwrap();
        assert_eq!(result, "llama-3-8b");
    }

    #[test]
    fn extract_model_missing_field_returns_error() {
        use super::extract_model;
        let payload = json!({"messages": []});
        let err = extract_model(&payload);
        assert!(err.is_err());
    }

    #[test]
    fn extract_model_null_value_returns_error() {
        use super::extract_model;
        let payload = json!({"model": null});
        let err = extract_model(&payload);
        assert!(err.is_err());
    }

    #[test]
    fn extract_model_non_string_value_returns_error() {
        use super::extract_model;
        let payload = json!({"model": 42});
        let err = extract_model(&payload);
        assert!(err.is_err());
    }

    // ===== extract_model_with_default tests =====

    #[test]
    fn extract_model_with_default_returns_model_when_present() {
        use super::extract_model_with_default;
        let payload = json!({"model": "gpt-4"});
        let result = extract_model_with_default(&payload, "default-model".to_string());
        assert_eq!(result, "gpt-4");
    }

    #[test]
    fn extract_model_with_default_returns_default_when_missing() {
        use super::extract_model_with_default;
        let payload = json!({"messages": []});
        let result = extract_model_with_default(&payload, "default-embed".to_string());
        assert_eq!(result, "default-embed");
    }

    #[test]
    fn extract_model_with_default_returns_default_when_empty_string() {
        use super::extract_model_with_default;
        let payload = json!({"model": ""});
        let result = extract_model_with_default(&payload, "fallback".to_string());
        assert_eq!(result, "fallback");
    }

    #[test]
    fn extract_model_with_default_returns_default_when_null() {
        use super::extract_model_with_default;
        let payload = json!({"model": null});
        let result = extract_model_with_default(&payload, "fallback".to_string());
        assert_eq!(result, "fallback");
    }

    // ===== extract_stream tests =====

    #[test]
    fn extract_stream_returns_true_when_set() {
        use super::extract_stream;
        let payload = json!({"model": "x", "stream": true});
        assert!(extract_stream(&payload));
    }

    #[test]
    fn extract_stream_returns_false_when_unset() {
        use super::extract_stream;
        let payload = json!({"model": "x"});
        assert!(!extract_stream(&payload));
    }

    #[test]
    fn extract_stream_returns_false_when_false() {
        use super::extract_stream;
        let payload = json!({"model": "x", "stream": false});
        assert!(!extract_stream(&payload));
    }

    #[test]
    fn extract_stream_returns_false_when_non_bool() {
        use super::extract_stream;
        let payload = json!({"model": "x", "stream": "yes"});
        assert!(!extract_stream(&payload));
    }

    // ===== payload_requires_image_input tests =====

    #[test]
    fn payload_requires_image_input_returns_false_for_text_only() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_returns_false_when_no_messages() {
        use super::payload_requires_image_input;
        let payload = json!({"model": "llama-3"});
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_detects_image_url_content() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "What is in this image?"},
                        {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                    ]
                }
            ]
        });
        assert!(payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_detects_input_image_content() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "What is in this image?"},
                        {"type": "input_image", "image_url": "https://example.com/img.png"}
                    ]
                }
            ]
        });
        assert!(payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_allows_text_parts_array() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello world"}
                    ]
                }
            ]
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_skips_string_content_messages() {
        use super::payload_requires_image_input;
        // messages with string content should be skipped (no array parts)
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {"role": "user", "content": "plain text"},
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "OK"}
                    ]
                }
            ]
        });
        assert!(!payload_requires_image_input(&payload));
    }

    // ===== cloud_virtual_node tests =====

    #[test]
    fn cloud_virtual_node_returns_correct_ids_for_known_providers() {
        use super::cloud_virtual_node;
        use super::UNSPECIFIED_IP;

        let (openai_id, openai_name, openai_ip) = cloud_virtual_node("openai");
        assert_eq!(
            openai_id,
            uuid::Uuid::parse_str("00000000-0000-0000-0000-00000000c001").unwrap()
        );
        assert_eq!(openai_name, "cloud:openai");
        assert_eq!(openai_ip, UNSPECIFIED_IP);

        let (google_id, google_name, _) = cloud_virtual_node("google");
        assert_eq!(
            google_id,
            uuid::Uuid::parse_str("00000000-0000-0000-0000-00000000c002").unwrap()
        );
        assert_eq!(google_name, "cloud:google");

        let (anthropic_id, anthropic_name, _) = cloud_virtual_node("anthropic");
        assert_eq!(
            anthropic_id,
            uuid::Uuid::parse_str("00000000-0000-0000-0000-00000000c003").unwrap()
        );
        assert_eq!(anthropic_name, "cloud:anthropic");
    }

    #[test]
    fn cloud_virtual_node_returns_fallback_for_unknown_provider() {
        use super::cloud_virtual_node;

        let (id, name, _) = cloud_virtual_node("unknown-provider");
        assert_eq!(
            id,
            uuid::Uuid::parse_str("00000000-0000-0000-0000-00000000c0ff").unwrap()
        );
        assert_eq!(name, "cloud:unknown-provider");
    }

    // ===== parse_cloud_model extended tests =====

    #[test]
    fn parse_cloud_model_anthropic_prefix() {
        assert_eq!(
            parse_cloud_model("anthropic:claude-3-opus"),
            Some(("anthropic".to_string(), "claude-3-opus".to_string()))
        );
    }

    #[test]
    fn parse_cloud_model_returns_none_for_plain_model_names() {
        assert_eq!(parse_cloud_model("llama-3-8b"), None);
        assert_eq!(parse_cloud_model("mistral-7b-instruct"), None);
        assert_eq!(parse_cloud_model(""), None);
    }

    #[test]
    fn parse_cloud_model_returns_none_for_empty_model_after_prefix() {
        assert_eq!(parse_cloud_model("openai:"), None);
        assert_eq!(parse_cloud_model("google:"), None);
        assert_eq!(parse_cloud_model("anthropic:"), None);
        assert_eq!(parse_cloud_model("ahtnorpic:"), None);
    }

    // ===== parse_client_ip_from_forwarded_value extended tests =====

    #[test]
    fn parse_client_ip_plain_ipv4() {
        let ip = parse_client_ip_from_forwarded_value("203.0.113.50").unwrap();
        assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_ipv4_with_port() {
        let ip = parse_client_ip_from_forwarded_value("203.0.113.50:8080").unwrap();
        assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_quoted_value() {
        let ip = parse_client_ip_from_forwarded_value("\"10.0.0.1\"").unwrap();
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_unknown_returns_none() {
        assert!(parse_client_ip_from_forwarded_value("unknown").is_none());
        assert!(parse_client_ip_from_forwarded_value("UNKNOWN").is_none());
    }

    #[test]
    fn parse_client_ip_obfuscated_returns_none() {
        assert!(parse_client_ip_from_forwarded_value("_secret").is_none());
    }

    #[test]
    fn parse_client_ip_empty_returns_none() {
        assert!(parse_client_ip_from_forwarded_value("").is_none());
        assert!(parse_client_ip_from_forwarded_value("  ").is_none());
    }

    #[test]
    fn parse_client_ip_bracketed_ipv6() {
        let ip = parse_client_ip_from_forwarded_value("[::1]").unwrap();
        assert_eq!(ip, "::1".parse::<IpAddr>().unwrap());
    }

    // ===== extract_client_ip_from_headers extended tests =====

    #[test]
    fn extract_client_ip_returns_none_with_no_headers() {
        let headers = HeaderMap::new();
        assert!(extract_client_ip_from_headers(&headers).is_none());
    }

    #[test]
    fn extract_client_ip_x_forwarded_for_single_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        let ip = extract_client_ip_from_headers(&headers).unwrap();
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_x_forwarded_for_skips_unknown() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, unknown, 192.168.1.1"),
        );
        let ip = extract_client_ip_from_headers(&headers).unwrap();
        assert_eq!(ip, "192.168.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_forwarded_header_for_key() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=10.20.30.40;proto=https"),
        );
        let ip = extract_client_ip_from_headers(&headers).unwrap();
        assert_eq!(ip, "10.20.30.40".parse::<IpAddr>().unwrap());
    }

    // ===== add_queue_headers tests =====

    #[test]
    fn add_queue_headers_inserts_status_and_wait_time() {
        use super::add_queue_headers;
        use axum::response::IntoResponse;

        let mut response = (StatusCode::OK, "test").into_response();
        add_queue_headers(&mut response, 1500);

        let headers = response.headers();
        assert_eq!(
            headers.get("x-queue-status").unwrap().to_str().unwrap(),
            "queued"
        );
        assert_eq!(
            headers.get("x-queue-wait-ms").unwrap().to_str().unwrap(),
            "1500"
        );
    }

    #[test]
    fn add_queue_headers_zero_wait_time() {
        use super::add_queue_headers;
        use axum::response::IntoResponse;

        let mut response = (StatusCode::OK, "test").into_response();
        add_queue_headers(&mut response, 0);

        assert_eq!(
            response
                .headers()
                .get("x-queue-wait-ms")
                .unwrap()
                .to_str()
                .unwrap(),
            "0"
        );
    }

    // ===== validation_error tests =====

    #[test]
    fn validation_error_creates_app_error() {
        use super::validation_error;
        let err = validation_error("test error message");
        let msg = format!("{:?}", err);
        assert!(msg.contains("test error message"));
    }

    // ===== UNSPECIFIED_IP constant test =====

    #[test]
    fn unspecified_ip_is_ipv4_unspecified() {
        use super::UNSPECIFIED_IP;
        assert_eq!(
            UNSPECIFIED_IP,
            std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
        );
        assert!(UNSPECIFIED_IP.is_unspecified());
    }

    // ===== extract_forwarded_for / extract_x_forwarded_for tests =====

    #[test]
    fn extract_x_forwarded_for_multiple_ips() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.168.1.1, 10.0.0.2, 172.16.0.3"),
        );
        let ip = extract_x_forwarded_for(&headers).unwrap();
        assert_eq!(ip, "192.168.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_x_forwarded_for_missing_header_returns_none() {
        use super::extract_x_forwarded_for;
        let headers = HeaderMap::new();
        assert!(extract_x_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_forwarded_for_missing_header_returns_none() {
        use super::extract_forwarded_for;
        let headers = HeaderMap::new();
        assert!(extract_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_forwarded_for_multiple_entries() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown;proto=https, for=198.51.100.10;proto=http"),
        );
        let ip = extract_forwarded_for(&headers).unwrap();
        assert_eq!(ip, "198.51.100.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_forwarded_for_case_insensitive_key() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("FOR=192.0.2.60;proto=https"),
        );
        let ip = extract_forwarded_for(&headers).unwrap();
        assert_eq!(ip, "192.0.2.60".parse::<IpAddr>().unwrap());
    }

    // ===== extract_client_info tests =====

    #[test]
    fn extract_client_info_with_forwarded_header() {
        use super::extract_client_info;
        use std::net::SocketAddr;

        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.50"));

        let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &None);
        assert_eq!(
            client_ip.unwrap(),
            "203.0.113.50".parse::<IpAddr>().unwrap()
        );
        assert!(api_key_id.is_none());
    }

    #[test]
    fn extract_client_info_without_forwarded_falls_back_to_socket() {
        use super::extract_client_info;
        use std::net::SocketAddr;

        let addr: SocketAddr = "10.0.0.5:9999".parse().unwrap();
        let headers = HeaderMap::new();

        let (client_ip, api_key_id) = extract_client_info(&addr, &headers, &None);
        assert_eq!(client_ip.unwrap(), "10.0.0.5".parse::<IpAddr>().unwrap());
        assert!(api_key_id.is_none());
    }

    // ===== Additional unit tests for increased coverage =====

    // --- parse_cloud_model: ahtnorpic alias ---

    #[test]
    fn parse_cloud_model_ahtnorpic_is_normalized_to_anthropic() {
        let result = parse_cloud_model("ahtnorpic:claude-3.5-sonnet");
        assert_eq!(
            result,
            Some(("anthropic".to_string(), "claude-3.5-sonnet".to_string()))
        );
    }

    #[test]
    fn parse_cloud_model_unsupported_prefix_returns_none() {
        assert_eq!(parse_cloud_model("azure:gpt-4"), None);
        assert_eq!(parse_cloud_model("aws:bedrock-model"), None);
        assert_eq!(parse_cloud_model("cohere:command"), None);
    }

    #[test]
    fn parse_cloud_model_prefix_is_case_sensitive() {
        // The function checks specific lowercase prefixes only
        assert_eq!(parse_cloud_model("OpenAI:gpt-4o"), None);
        assert_eq!(parse_cloud_model("GOOGLE:gemini"), None);
        assert_eq!(parse_cloud_model("Anthropic:claude"), None);
    }

    #[test]
    fn parse_cloud_model_with_slashes_in_model_name() {
        let result = parse_cloud_model("openai:org/model-name");
        assert_eq!(
            result,
            Some(("openai".to_string(), "org/model-name".to_string()))
        );
    }

    #[test]
    fn parse_cloud_model_with_complex_model_name() {
        let result = parse_cloud_model("google:gemini-1.5-pro-002");
        assert_eq!(
            result,
            Some(("google".to_string(), "gemini-1.5-pro-002".to_string()))
        );
    }

    // --- extract_model edge cases ---

    #[test]
    fn extract_model_boolean_value_returns_error() {
        use super::extract_model;
        let payload = json!({"model": true});
        assert!(extract_model(&payload).is_err());
    }

    #[test]
    fn extract_model_array_value_returns_error() {
        use super::extract_model;
        let payload = json!({"model": ["gpt-4"]});
        assert!(extract_model(&payload).is_err());
    }

    #[test]
    fn extract_model_object_value_returns_error() {
        use super::extract_model;
        let payload = json!({"model": {"name": "gpt-4"}});
        assert!(extract_model(&payload).is_err());
    }

    #[test]
    fn extract_model_empty_string_is_valid() {
        use super::extract_model;
        let payload = json!({"model": ""});
        // extract_model returns any string, empty or not
        let result = extract_model(&payload).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn extract_model_whitespace_string_is_valid() {
        use super::extract_model;
        let payload = json!({"model": "  "});
        let result = extract_model(&payload).unwrap();
        assert_eq!(result, "  ");
    }

    // --- extract_model_with_default edge cases ---

    #[test]
    fn extract_model_with_default_non_string_returns_default() {
        use super::extract_model_with_default;
        let payload = json!({"model": 123});
        let result = extract_model_with_default(&payload, "default".to_string());
        assert_eq!(result, "default");
    }

    #[test]
    fn extract_model_with_default_boolean_returns_default() {
        use super::extract_model_with_default;
        let payload = json!({"model": false});
        let result = extract_model_with_default(&payload, "default".to_string());
        assert_eq!(result, "default");
    }

    #[test]
    fn extract_model_with_default_whitespace_string_is_not_empty() {
        use super::extract_model_with_default;
        let payload = json!({"model": "  "});
        let result = extract_model_with_default(&payload, "default".to_string());
        // "  " is not empty so it should be returned
        assert_eq!(result, "  ");
    }

    // --- extract_stream edge cases ---

    #[test]
    fn extract_stream_null_value_returns_false() {
        use super::extract_stream;
        let payload = json!({"model": "x", "stream": null});
        assert!(!extract_stream(&payload));
    }

    #[test]
    fn extract_stream_numeric_value_returns_false() {
        use super::extract_stream;
        let payload = json!({"model": "x", "stream": 1});
        assert!(!extract_stream(&payload));
    }

    // --- payload_requires_image_input edge cases ---

    #[test]
    fn payload_requires_image_input_empty_messages_array() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": []
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_messages_not_array_returns_false() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": "not an array"
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_null_content_is_skipped() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {"role": "user", "content": null}
            ]
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_empty_content_array() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {"role": "user", "content": []}
            ]
        });
        assert!(!payload_requires_image_input(&payload));
    }

    #[test]
    fn payload_requires_image_input_multiple_messages_detects_image_in_second() {
        use super::payload_requires_image_input;
        let payload = json!({
            "model": "llama-3",
            "messages": [
                {"role": "user", "content": "plain text"},
                {
                    "role": "user",
                    "content": [
                        {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                    ]
                }
            ]
        });
        assert!(payload_requires_image_input(&payload));
    }

    // --- parse_client_ip_from_forwarded_value edge cases ---

    #[test]
    fn parse_client_ip_plain_ipv6() {
        let ip = parse_client_ip_from_forwarded_value("::1").unwrap();
        assert_eq!(ip, "::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_full_ipv6() {
        let ip = parse_client_ip_from_forwarded_value("2001:db8::1").unwrap();
        assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_garbage_returns_none() {
        assert!(parse_client_ip_from_forwarded_value("not-an-ip").is_none());
        assert!(parse_client_ip_from_forwarded_value("abc.def.ghi.jkl").is_none());
    }

    #[test]
    fn parse_client_ip_ipv4_mapped_ipv6() {
        // ::ffff:192.168.1.1 is an IPv4-mapped IPv6 address
        let ip = parse_client_ip_from_forwarded_value("::ffff:192.168.1.1").unwrap();
        // normalize_ip should convert this to an IPv4 address
        assert!(ip.is_ipv4());
    }

    #[test]
    fn parse_client_ip_quoted_ipv6() {
        let ip = parse_client_ip_from_forwarded_value("\"[2001:db8::1]\"").unwrap();
        assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_client_ip_whitespace_only_returns_none() {
        assert!(parse_client_ip_from_forwarded_value("   ").is_none());
    }

    #[test]
    fn parse_client_ip_underscore_prefix_returns_none() {
        // Obfuscated identifiers start with underscore per RFC
        assert!(parse_client_ip_from_forwarded_value("_hidden").is_none());
        assert!(parse_client_ip_from_forwarded_value("_obfuscated123").is_none());
    }

    // --- extract_x_forwarded_for edge cases ---

    #[test]
    fn extract_x_forwarded_for_all_unknown() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, unknown"),
        );
        assert!(extract_x_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_x_forwarded_for_single_valid_ip() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("172.16.0.1"));
        let ip = extract_x_forwarded_for(&headers).unwrap();
        assert_eq!(ip, "172.16.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_x_forwarded_for_with_ipv6() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("2001:db8::1, 10.0.0.1"),
        );
        let ip = extract_x_forwarded_for(&headers).unwrap();
        assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    // --- extract_forwarded_for edge cases ---

    #[test]
    fn extract_forwarded_for_no_for_key() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("proto=https;host=example.com"),
        );
        assert!(extract_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_forwarded_for_empty_value() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", HeaderValue::from_static(""));
        assert!(extract_forwarded_for(&headers).is_none());
    }

    // --- add_queue_headers edge cases ---

    #[test]
    fn add_queue_headers_large_wait_time() {
        use super::add_queue_headers;
        use axum::response::IntoResponse;

        let mut response = (StatusCode::OK, "test").into_response();
        add_queue_headers(&mut response, u128::MAX);

        assert_eq!(
            response
                .headers()
                .get("x-queue-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "queued"
        );
        // x-queue-wait-ms should be set if the string is valid for HeaderValue
    }

    // --- validation_error tests ---

    #[test]
    fn validation_error_from_string() {
        use super::validation_error;
        let err = validation_error("custom validation error".to_string());
        let msg = format!("{:?}", err);
        assert!(msg.contains("custom validation error"));
    }

    // --- cloud_virtual_node tests ---

    #[test]
    fn cloud_virtual_node_ids_are_all_distinct() {
        use super::cloud_virtual_node;

        let (openai_id, _, _) = cloud_virtual_node("openai");
        let (google_id, _, _) = cloud_virtual_node("google");
        let (anthropic_id, _, _) = cloud_virtual_node("anthropic");
        let (unknown_id, _, _) = cloud_virtual_node("something-else");

        assert_ne!(openai_id, google_id);
        assert_ne!(openai_id, anthropic_id);
        assert_ne!(openai_id, unknown_id);
        assert_ne!(google_id, anthropic_id);
        assert_ne!(google_id, unknown_id);
        assert_ne!(anthropic_id, unknown_id);
    }

    #[test]
    fn cloud_virtual_node_ip_is_always_unspecified() {
        use super::cloud_virtual_node;
        use super::UNSPECIFIED_IP;

        for provider in &["openai", "google", "anthropic", "custom", ""] {
            let (_, _, ip) = cloud_virtual_node(provider);
            assert_eq!(ip, UNSPECIFIED_IP);
        }
    }

    #[test]
    fn cloud_virtual_node_name_format() {
        use super::cloud_virtual_node;

        let (_, name, _) = cloud_virtual_node("test-provider");
        assert_eq!(name, "cloud:test-provider");

        let (_, name, _) = cloud_virtual_node("");
        assert_eq!(name, "cloud:");
    }

    // --- extract_client_info with auth context ---

    #[test]
    fn extract_client_info_client_ip_is_always_some() {
        use super::extract_client_info;
        use std::net::SocketAddr;

        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let headers = HeaderMap::new();

        let (client_ip, _) = extract_client_info(&addr, &headers, &None);
        assert!(client_ip.is_some());
    }

    #[test]
    fn extract_client_info_prefers_x_forwarded_for_over_socket() {
        use super::extract_client_info;
        use std::net::SocketAddr;

        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));

        let (client_ip, _) = extract_client_info(&addr, &headers, &None);
        assert_eq!(client_ip.unwrap(), "1.2.3.4".parse::<IpAddr>().unwrap());
    }

    // --- extract_client_ip_from_headers edge cases ---

    #[test]
    fn extract_client_ip_x_forwarded_for_takes_priority_over_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        headers.insert("forwarded", HeaderValue::from_static("for=192.168.1.1"));
        let ip = extract_client_ip_from_headers(&headers).unwrap();
        // x-forwarded-for should take priority
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_only_forwarded_header() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", HeaderValue::from_static("for=172.16.0.100"));
        let ip = extract_client_ip_from_headers(&headers).unwrap();
        assert_eq!(ip, "172.16.0.100".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_both_headers_all_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, _obfuscated"),
        );
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown;proto=https"),
        );
        assert!(extract_client_ip_from_headers(&headers).is_none());
    }

    // ===== /v1/models response correction tests (#575 US-001 補正対応) =====

    /// helper: 指定 supported_apis を申告するオンラインエンドポイントを登録する
    async fn add_endpoint_with_supported_apis(
        state: &AppState,
        endpoint_name: &str,
        model_id: &str,
        apis: Vec<crate::types::endpoint::SupportedAPI>,
    ) -> uuid::Uuid {
        use crate::types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType};
        let mut endpoint = Endpoint::new(
            endpoint_name.to_string(),
            "http://127.0.0.1:0".to_string(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        state
            .endpoint_registry
            .add_model(&EndpointModel {
                endpoint_id,
                model_id: model_id.to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: apis,
                canonical_name: None,
            })
            .await
            .expect("add endpoint model");
        endpoint_id
    }

    /// helper: list_models() を呼び出し、JSON ボディを返す
    async fn fetch_list_models(state: AppState) -> serde_json::Value {
        let resp = super::list_models(axum::extract::State(state))
            .await
            .expect("list_models ok");
        let bytes = to_bytes(resp.into_body(), 1_000_000)
            .await
            .expect("read body");
        serde_json::from_slice::<serde_json::Value>(&bytes).expect("parse json")
    }

    /// A-1 RED: 登録済み embedding モデルの supported_apis に "embeddings" が含まれること
    #[tokio::test]
    #[serial]
    async fn list_models_embedding_capability_emits_embeddings_api() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;
        use crate::types::ModelCapability;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        // embedding capability で登録
        let mut model = ModelInfo::with_capabilities(
            "nomic-ai/nomic-embed-text-v1.5".to_string(),
            0,
            "embedding model".to_string(),
            0,
            vec![],
            vec![ModelCapability::Embedding],
        );
        model.repo = Some("nomic-ai/nomic-embed-text-v1.5".to_string());
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        // エンドポイントは ChatCompletions しか申告しない（実環境の典型ケース）
        add_endpoint_with_supported_apis(
            &state,
            "embed-endpoint",
            "nomic-ai/nomic-embed-text-v1.5",
            vec![SupportedAPI::ChatCompletions],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("nomic-ai/nomic-embed-text-v1.5"))
            .expect("embedding model in /v1/models");

        let apis: Vec<&str> = model["supported_apis"]
            .as_array()
            .expect("supported_apis array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            apis.contains(&"embeddings"),
            "embedding-capability model must report 'embeddings' in supported_apis (got: {:?})",
            apis
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// A-2 RED: 登録済みモデルの created は last_modified の Unix epoch を反映すること
    #[tokio::test]
    #[serial]
    async fn list_models_registered_created_reflects_last_modified() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;
        use chrono::TimeZone;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let known_ts = chrono::Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let mut model = ModelInfo::new(
            "vendor/known-time-model".to_string(),
            0,
            "test".to_string(),
            0,
            vec![],
        );
        model.last_modified = Some(known_ts);
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        add_endpoint_with_supported_apis(
            &state,
            "ts-endpoint",
            "vendor/known-time-model",
            vec![SupportedAPI::ChatCompletions],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("vendor/known-time-model"))
            .expect("model in /v1/models");

        let created = model["created"].as_i64().expect("created is integer");
        assert_eq!(
            created,
            known_ts.timestamp(),
            "registered model created must reflect last_modified epoch (got: {})",
            created
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// A-3 RED: supported_apis は as_str() 昇順で並ぶこと（決定論化）
    #[tokio::test]
    #[serial]
    async fn list_models_supported_apis_are_sorted() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;
        use crate::types::ModelCapability;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        // TextGeneration + Embedding の両対応モデル
        let model = ModelInfo::with_capabilities(
            "vendor/multi-cap".to_string(),
            0,
            "multi capability".to_string(),
            0,
            vec![],
            vec![ModelCapability::TextGeneration, ModelCapability::Embedding],
        );
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        add_endpoint_with_supported_apis(
            &state,
            "multi-endpoint",
            "vendor/multi-cap",
            vec![SupportedAPI::ChatCompletions, SupportedAPI::Embeddings],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("vendor/multi-cap"))
            .expect("multi-cap model in /v1/models");

        let apis: Vec<&str> = model["supported_apis"]
            .as_array()
            .expect("supported_apis array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        let mut sorted = apis.clone();
        sorted.sort();
        assert_eq!(
            apis, sorted,
            "supported_apis must be in ascending lexicographic order (got: {:?})",
            apis
        );
        // 期待される3要素を網羅していること（A-1 の効果も込み）
        assert!(apis.contains(&"chat_completions"));
        assert!(apis.contains(&"embeddings"));
        assert!(apis.contains(&"responses"));
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// Regression: capability 由来 set が非空でも endpoint_reported が必ずマージされること
    /// （review #642 — ModelInfo がレガシー行で `[TextGeneration]` を既定返却することにより
    /// endpoint が報告する `embeddings` 等が取りこぼされていた問題の検証）
    #[tokio::test]
    #[serial]
    async fn list_models_endpoint_apis_are_merged_even_when_model_capabilities_default() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        // capabilities を明示せず登録（=> get_capabilities() は [TextGeneration] を既定返却）
        let model = ModelInfo::new(
            "vendor/legacy-row".to_string(),
            0,
            "legacy registration without capabilities".to_string(),
            0,
            vec![],
        );
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        // endpoint は Embeddings をサポートしている（実態を申告）
        add_endpoint_with_supported_apis(
            &state,
            "embed-endpoint",
            "vendor/legacy-row",
            vec![SupportedAPI::Embeddings],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("vendor/legacy-row"))
            .expect("model in /v1/models");

        let apis: Vec<&str> = model["supported_apis"]
            .as_array()
            .expect("supported_apis array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            apis.contains(&"embeddings"),
            "endpoint-reported `embeddings` must not be dropped even when capability-derived set is non-empty (got: {:?})",
            apis
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// G-3 暫定: 量子化サフィックス付きモデル ID から `quantization` フィールドが分離されること
    #[tokio::test]
    #[serial]
    async fn list_models_quantization_suffix_is_emitted_as_separate_field() {
        use crate::types::endpoint::{
            Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI,
        };

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        // 1 つのエンドポイントに「量子化付き」と「量子化なし」両方のモデルを登録して比較
        let mut endpoint = Endpoint::new(
            "quant-endpoint".to_string(),
            "http://127.0.0.1:0".to_string(),
            EndpointType::OpenaiCompatible,
        );
        endpoint.status = EndpointStatus::Online;
        let endpoint_id = endpoint.id;
        state
            .endpoint_registry
            .add(endpoint)
            .await
            .expect("add endpoint");
        for model_id in ["ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M", "openai/gpt-oss-20b"] {
            state
                .endpoint_registry
                .add_model(&EndpointModel {
                    endpoint_id,
                    model_id: model_id.to_string(),
                    capabilities: None,
                    max_tokens: None,
                    last_checked: None,
                    supported_apis: vec![SupportedAPI::ChatCompletions],
                    canonical_name: None,
                })
                .await
                .expect("add endpoint model");
        }

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");

        let gguf = data
            .iter()
            .find(|m| m["id"].as_str() == Some("ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M"))
            .expect("gguf model in /v1/models");
        assert_eq!(
            gguf["quantization"].as_str(),
            Some("Q4_K_M"),
            "quantization suffix must be emitted as separate field"
        );
        assert_eq!(
            gguf["id"].as_str(),
            Some("ggml-org/gemma-4-E4B-it-GGUF:Q4_K_M"),
            "id must remain unchanged for backward compatibility"
        );

        let plain = data
            .iter()
            .find(|m| m["id"].as_str() == Some("openai/gpt-oss-20b"))
            .expect("non-gguf model in /v1/models");
        assert!(
            plain["quantization"].is_null(),
            "non-quantized model must report quantization: null (got: {:?})",
            plain["quantization"]
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// B-1/G-7: endpoint が max_tokens を申告しない場合に既知 canonical テーブルから補填すること
    #[tokio::test]
    #[serial]
    async fn list_models_max_tokens_falls_back_to_known_canonical_table() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        // 既知 canonical のモデルを登録（テーブルには 131072 がある）
        let model = ModelInfo::new(
            "openai/gpt-oss-20b".to_string(),
            0,
            "test".to_string(),
            0,
            vec![],
        );
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        // endpoint は max_tokens を申告しない（add_endpoint_with_supported_apis のヘルパは
        // EndpointModel.max_tokens=None で登録する）
        add_endpoint_with_supported_apis(
            &state,
            "fallback-endpoint",
            "openai/gpt-oss-20b",
            vec![SupportedAPI::ChatCompletions],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("openai/gpt-oss-20b"))
            .expect("model in /v1/models");

        let max_tokens = model["max_tokens"]
            .as_u64()
            .expect("max_tokens fallback applied");
        assert_eq!(
            max_tokens, 131_072,
            "max_tokens must fall back to KNOWN_CONTEXT_LENGTHS for known canonical (got: {})",
            max_tokens
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }

    /// A-5 RED: owned_by は id の組織プレフィックス由来になること（"load balancer" 固定値からの脱却）
    #[tokio::test]
    #[serial]
    async fn list_models_owned_by_uses_id_org_prefix() {
        use crate::registry::models::ModelInfo;
        use crate::types::endpoint::SupportedAPI;

        let _guard = TEST_LOCK.lock().await;
        let (state, _dir) = create_state_with_tempdir().await;

        let model = ModelInfo::new(
            "Qwen/Qwen3-Coder-30B-A3B-Instruct".to_string(),
            0,
            "test".to_string(),
            0,
            vec![],
        );
        let storage = crate::db::models::ModelStorage::new(state.db_pool.clone());
        storage.save_model(&model).await.expect("save model");

        add_endpoint_with_supported_apis(
            &state,
            "qwen-endpoint",
            "Qwen/Qwen3-Coder-30B-A3B-Instruct",
            vec![SupportedAPI::ChatCompletions],
        )
        .await;

        let body = fetch_list_models(state).await;
        let data = body["data"].as_array().expect("data array");
        let model = data
            .iter()
            .find(|m| m["id"].as_str() == Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"))
            .expect("model in /v1/models");

        let owned_by = model["owned_by"].as_str().expect("owned_by string");
        assert_eq!(
            owned_by, "Qwen",
            "owned_by must derive from id organization prefix (got: {})",
            owned_by
        );
        std::env::remove_var("LLMLB_DATA_DIR");
    }
}
