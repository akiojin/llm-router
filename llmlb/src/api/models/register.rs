//! HFモデル登録ハンドラ。
//!
//! 登録リクエストの検証・アーティファクト解決・GPU 警告算出を経て
//! モデルを登録する。

use super::artifacts::*;
use super::*;
use crate::registry::models::{extract_repo_id, generate_model_id};
use serde::Deserialize;

#[derive(Deserialize)]
/// HFモデル登録リクエスト
pub(crate) struct RegisterModelRequest {
    /// HFリポジトリ名 (e.g., TheBloke/Llama-2-7B-GGUF)
    pub repo: String,
    /// ファイル名 (e.g., llama-2-7b.Q4_K_M.gguf)
    pub filename: Option<String>,
    /// 表示名（任意）
    #[serde(default)]
    pub display_name: Option<String>,
    /// オプションのchat_template（GGUFに含まれない場合の補助）
    #[serde(default)]
    pub chat_template: Option<String>,
}

async fn compute_gpu_warnings(
    registry: &crate::registry::endpoints::EndpointRegistry,
    required_memory: u64,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if required_memory == 0 {
        return warnings;
    }

    let endpoints = registry.list().await;
    let mut memories: Vec<u64> = Vec::new();
    for endpoint in endpoints {
        if let Some(mem) = endpoint.gpu_total_memory_bytes {
            memories.push(mem);
        }
    }

    // 安全: max()はSomeを返すことが保証（空でないことを上でチェック済み）
    let Some(max_mem) = memories.iter().max().copied() else {
        warnings.push("No GPU memory info available from registered endpoints".into());
        return warnings;
    };

    if required_memory > max_mem {
        warnings.push(format!(
            "Model requires {:.1}GB but max endpoint GPU memory is {:.1}GB",
            required_memory as f64 / (1024.0 * 1024.0 * 1024.0),
            max_mem as f64 / (1024.0 * 1024.0 * 1024.0),
        ));
    }

    warnings
}

/// POST /api/models/register - HFモデルを対応モデルに登録（メタデータのみ）
///
/// 方針:
/// - llmlbは変換・バイナリ保存を行わない
/// - `filename` を指定するとそのアーティファクトを主として登録
/// - 未指定の場合、リポジトリ内のアーティファクトが一意であれば自動選択
/// - safetensors では `config.json` / `tokenizer.json` が必須
pub(crate) async fn register_model(
    State(state): State<AppState>,
    Json(req): Json<RegisterModelRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if req.repo.trim().is_empty() {
        return Err(LbError::Common(CommonError::Validation("repo is required".into())).into());
    }

    // URLからrepo_idを抽出（フルURLが渡された場合はrepo_id形式に正規化）
    let repo = extract_repo_id(&req.repo);

    let name = generate_model_id(&repo);
    if load_registered_model(&state.db_pool, &name)
        .await?
        .is_some()
    {
        return Err(
            LbError::Common(CommonError::Validation("Model already registered".into())).into(),
        );
    }

    let filename_hint = req
        .filename
        .clone()
        .or_else(|| extract_filename_from_hf_url(&req.repo));

    if let Some(fname) = filename_hint.as_ref() {
        validate_artifact_path(fname)?;
    }

    let siblings = fetch_repo_siblings(&state.http_client, &repo).await?;
    let selection = resolve_primary_artifact(&siblings, filename_hint)?;

    let (content_length, required_memory, warnings) = {
        let (size, required) = match selection.format {
            ArtifactFormat::Gguf => {
                let size = siblings
                    .iter()
                    .find(|s| s.rfilename == selection.filename)
                    .map(sibling_size_bytes)
                    .unwrap_or(0);
                const REQUIRED_MEMORY_RATIO: f64 = 1.5;
                let required = if size > 0 {
                    ((size as f64) * REQUIRED_MEMORY_RATIO).ceil() as u64
                } else {
                    0
                };
                (size, required)
            }
            ArtifactFormat::Safetensors => {
                let total = siblings
                    .iter()
                    .filter(|s| s.rfilename.to_ascii_lowercase().ends_with(".safetensors"))
                    .map(sibling_size_bytes)
                    .sum::<u64>();
                const REQUIRED_MEMORY_RATIO: f64 = 1.5;
                let required = if total > 0 {
                    ((total as f64) * REQUIRED_MEMORY_RATIO).ceil() as u64
                } else {
                    0
                };
                (total, required)
            }
        };
        let warnings = compute_gpu_warnings(&state.endpoint_registry, required).await;
        (size, required, warnings)
    };

    let chat_template = if req.chat_template.is_some() {
        req.chat_template.clone()
    } else {
        fetch_chat_template_from_hf(&state.http_client, &repo).await
    };

    let mut tags = Vec::new();
    match selection.format {
        ArtifactFormat::Gguf => tags.push("gguf".to_string()),
        ArtifactFormat::Safetensors => tags.push("safetensors".to_string()),
    }
    let description = req.display_name.clone().unwrap_or_else(|| repo.clone());
    let capabilities = vec![crate::types::ModelCapability::TextGeneration];
    let size_bytes = content_length;
    let required_memory_bytes = required_memory;

    let source = match selection.format {
        ArtifactFormat::Gguf => crate::registry::models::ModelSource::HfGguf,
        ArtifactFormat::Safetensors => crate::registry::models::ModelSource::HfSafetensors,
    };

    let model = ModelInfo {
        name: name.clone(),
        size: size_bytes,
        description,
        required_memory: required_memory_bytes,
        tags,
        capabilities,
        source,
        chat_template,
        repo: Some(repo.clone()),
        filename: Some(selection.filename.clone()),
        last_modified: None,
        status: Some("registered".to_string()),
    };

    let storage = ModelStorage::new(state.db_pool.clone());
    storage.save_model(&model).await?;

    tracing::info!(
        repo = %repo,
        filename = %selection.filename,
        size_bytes = content_length,
        required_memory_bytes = required_memory,
        warnings = warnings.len(),
        "hf_model_registered"
    );

    let response = serde_json::json!({
        "name": name,
        "status": "registered",
        "filename": selection.filename,
        "size_bytes": content_length,
        "required_memory_bytes": required_memory,
        "warnings": warnings,
    });

    Ok((StatusCode::CREATED, Json(response)))
}
