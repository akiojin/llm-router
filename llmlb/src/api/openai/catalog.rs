//! /v1/models と /v1/models/:id のモデルカタログ列挙
//!
//! arch-review [H6]: api/openai.rs から、登録済みモデルメタデータ＋エンドポイント
//! 申告の supported_apis/max_tokens/canonical＋クラウドモデルを OpenAI 互換の
//! モデル一覧へ集約するロジックを分離。親は pub use でルーター参照を維持する。

use crate::api::error::AppError;
use crate::api::models::{list_registered_models, LifecycleStatus};
use crate::types::model::ModelCapabilities;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

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
        // arch-review [M13]: ModelCapability→SupportedAPI の対応表は
        // ModelCapability::supported_apis() へ集約済み（型付き変換）。
        for cap in m.get_capabilities() {
            set.extend(cap.supported_apis());
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
                    // 表示用キーの決定（US-029 FR-027/FR-028）:
                    // 明示 canonical → ヒューリスティック一次配布元 canonical → self の順で解決する。
                    let display_key = canonical_resolution.canonical_for(&model.model_id);

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

    let cloud_models = crate::api::cloud_models::get_cached_models(&state.http_client).await;
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

/// GET /v1/models/:id - モデル詳細取得（Azure capabilities 形式）
///
/// SPEC-0f1de549: Endpoints APIで登録されたモデルも検索対象に含める
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
