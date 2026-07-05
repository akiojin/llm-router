//! ダッシュボードのモデルカタログ集約（canonical/detail 表示 + クラウドモデル統合）
//!
//! arch-review [H6]: api/dashboard.rs からモデル一覧集約ハンドラを分離。

use super::*;

/// ダッシュボードモデル一覧の表示モード（US-029）。
///
/// - `canonical`（既定）: 論理モデル単位に集約した Canonical 表示
/// - `detail`: 全 variant（owner/量子化違い）を個別に列挙する詳細表示
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ModelsViewQuery {
    /// `canonical` | `detail`（未指定時は `canonical`）
    #[serde(default)]
    pub view: Option<String>,
}

impl ModelsViewQuery {
    fn is_detail(&self) -> bool {
        self.view.as_deref() == Some("detail")
    }
}

/// GET /api/dashboard/models - ダッシュボード向けモデル一覧
///
/// `?view=canonical`（既定）で論理モデル単位に集約、`?view=detail` で全 variant を列挙する。
pub async fn get_models(
    State(state): State<AppState>,
    Query(query): Query<ModelsViewQuery>,
) -> Result<Response, AppError> {
    use crate::api::models::{list_registered_models, LifecycleStatus};
    use crate::types::endpoint::SupportedAPI;

    let detail_view = query.is_detail();

    let mut registered_map: HashMap<String, crate::registry::models::ModelInfo> = HashMap::new();
    for model in list_registered_models(&state.db_pool).await? {
        registered_map.insert(model.name.clone(), model);
    }

    let endpoints = crate::db::endpoints::list_endpoints(&state.db_pool)
        .await
        .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;

    let mut endpoint_model_apis: HashMap<String, HashSet<SupportedAPI>> = HashMap::new();
    let mut endpoint_model_max_tokens: HashMap<String, Option<u32>> = HashMap::new();
    let mut endpoint_model_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut ready_models: HashSet<String> = HashSet::new();

    // 各エンドポイントのモデルを一度だけ取得する。
    // 従来は canonical 構築ループと表示構築ループで list_endpoint_models を
    // エンドポイントごとに2回問い合わせていた（2N クエリ）。ここで N クエリに削減する。
    let mut endpoints_with_models = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let endpoint_models =
            crate::db::endpoints::list_endpoint_models(&state.db_pool, endpoint.id)
                .await
                .map_err(|e| AppError(crate::common::error::LbError::Database(e.to_string())))?;
        endpoints_with_models.push((endpoint, endpoint_models));
    }

    // canonical name解決マップを構築（取得済みキャッシュから）
    let mut all_models_raw: Vec<(String, Option<String>)> = Vec::new();
    for (_, endpoint_models) in &endpoints_with_models {
        for model in endpoint_models {
            all_models_raw.push((model.model_id.clone(), model.canonical_name.clone()));
        }
    }
    let canonical_resolution = crate::models::mapping::build_canonical_maps(
        all_models_raw
            .iter()
            .map(|(id, cn)| (id.as_str(), cn.as_deref())),
    );

    for (endpoint, endpoint_models) in endpoints_with_models {
        for model in endpoint_models {
            // 表示用キーの決定（US-029）:
            // canonical 表示は一次配布元 canonical へ集約、detail 表示は model_id 単位で個別列挙。
            let display_key = if detail_view {
                model.model_id.clone()
            } else {
                canonical_resolution.canonical_for(&model.model_id)
            };

            endpoint_model_ids
                .entry(display_key.clone())
                .or_default()
                .insert(endpoint.id.to_string());
            // エイリアス名でもエンドポイントIDを登録（ルーティング用）
            if display_key != model.model_id {
                endpoint_model_ids
                    .entry(model.model_id.clone())
                    .or_default()
                    .insert(endpoint.id.to_string());
            }

            let apis = endpoint_model_apis.entry(display_key.clone()).or_default();
            for api in model.supported_apis {
                apis.insert(api);
            }
            apis.insert(SupportedAPI::Responses);

            let entry = endpoint_model_max_tokens
                .entry(display_key.clone())
                .or_insert(None);
            if let Some(mt) = model.max_tokens {
                *entry = Some(entry.map_or(mt, |existing| existing.max(mt)));
            }

            if endpoint.status == EndpointStatus::Online {
                ready_models.insert(display_key);
            }
        }
    }

    let mut available_models: Vec<String> = endpoint_model_apis.keys().cloned().collect();
    available_models.sort();

    let mut seen_models: HashSet<String> = HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for model_id in &available_models {
        seen_models.insert(model_id.clone());
        let ready = ready_models.contains(model_id);

        let supported_apis: Vec<String> = endpoint_model_apis
            .get(model_id)
            .map(|apis| apis.iter().map(|a| a.as_str().to_string()).collect())
            .unwrap_or_else(|| vec!["chat_completions".to_string()]);
        let endpoint_ids: Vec<String> = endpoint_model_ids
            .get(model_id)
            .map(|ids| {
                let mut ids: Vec<String> = ids.iter().cloned().collect();
                ids.sort();
                ids
            })
            .unwrap_or_default();

        let aliases = canonical_resolution.aliases_for(model_id);
        let canonical_name = canonical_resolution.canonical_for(model_id);
        let max_tokens = crate::models::mapping::resolve_max_tokens(
            &canonical_name,
            endpoint_model_max_tokens.get(model_id).copied().flatten(),
        );
        let (_, quantization) = crate::models::mapping::split_quantization_suffix(model_id);
        let quantization = quantization.map(|s| s.to_string());

        if let Some(m) = registered_map.get(model_id) {
            let caps: crate::types::model::ModelCapabilities = m.get_capabilities().into();
            data.push(json!({
                "id": m.name,
                "object": "model",
                "created": 0,
                "owned_by": "load balancer",
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
            }));
        } else {
            data.push(json!({
                "id": model_id,
                "object": "model",
                "created": 0,
                "owned_by": "load balancer",
                "lifecycle_status": LifecycleStatus::Registered,
                "download_progress": null,
                "ready": ready,
                "supported_apis": supported_apis,
                "max_tokens": max_tokens,
                "quantization": quantization,
                "endpoint_ids": endpoint_ids,
                "canonical_name": canonical_name,
                "aliases": aliases,
            }));
        }
    }

    for (model_id, apis) in &endpoint_model_apis {
        if seen_models.contains(model_id) {
            continue;
        }
        seen_models.insert(model_id.clone());

        let supported_apis: Vec<String> = apis.iter().map(|a| a.as_str().to_string()).collect();
        let endpoint_ids: Vec<String> = endpoint_model_ids
            .get(model_id)
            .map(|ids| {
                let mut ids: Vec<String> = ids.iter().cloned().collect();
                ids.sort();
                ids
            })
            .unwrap_or_default();
        let aliases = canonical_resolution.aliases_for(model_id);
        let canonical_name = canonical_resolution.canonical_for(model_id);
        let max_tokens = crate::models::mapping::resolve_max_tokens(
            &canonical_name,
            endpoint_model_max_tokens.get(model_id).copied().flatten(),
        );
        let (_, quantization) = crate::models::mapping::split_quantization_suffix(model_id);
        let quantization = quantization.map(|s| s.to_string());
        data.push(json!({
            "id": model_id,
            "object": "model",
            "created": 0,
            "owned_by": "endpoint",
            "lifecycle_status": LifecycleStatus::Registered,
            "download_progress": null,
            "ready": ready_models.contains(model_id),
            "supported_apis": supported_apis,
            "max_tokens": max_tokens,
            "quantization": quantization,
            "endpoint_ids": endpoint_ids,
            "canonical_name": canonical_name,
            "aliases": aliases,
        }));
    }

    let cloud_models = crate::api::cloud_models::get_cached_models(&state.http_client).await;
    for cm in cloud_models {
        data.push(json!({
            "id": cm.id,
            "object": cm.object,
            "created": cm.created,
            "owned_by": cm.owned_by,
            "lifecycle_status": LifecycleStatus::Registered,
            "download_progress": null,
            "ready": true,
            "supported_apis": vec!["chat_completions"],
            "max_tokens": null,
            "endpoint_ids": Vec::<String>::new(),
        }));
    }

    let body = json!({
        "object": "list",
        "data": data,
    });

    Ok((StatusCode::OK, Json(body)).into_response())
}
