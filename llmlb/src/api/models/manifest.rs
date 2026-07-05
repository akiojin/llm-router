//! モデル配布マニフェスト生成ハンドラ。
//!
//! Node が複数ファイル（safetensors + metadata）としてモデルを取得する
//! ためのマニフェストJSONを生成する。

use super::artifacts::*;
use super::*;
use axum::response::IntoResponse;

///
/// Node がモデルを複数ファイル（safetensors + metadata）として取得するためのマニフェスト。
pub(crate) async fn get_model_registry_manifest(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::response::Response;

    if let Err(e) = validate_model_name(&model_name) {
        return AppError(e).into_response();
    }

    let model = match load_registered_model(&state.db_pool, &model_name).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            return AppError(LbError::NotFound(format!(
                "Model not found: {}",
                model_name
            )))
            .into_response();
        }
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let Some(repo) = model.repo.clone() else {
        return AppError(LbError::Common(CommonError::Validation(
            "repo not set for model".into(),
        )))
        .into_response();
    };

    let siblings = match fetch_repo_siblings(&state.http_client, &repo).await {
        Ok(list) => list,
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let selection = match resolve_primary_artifact(&siblings, model.filename.clone()) {
        Ok(sel) => sel,
        Err(e) => {
            return AppError(e).into_response();
        }
    };

    let runtime_hint = match selection.format {
        ArtifactFormat::Gguf => Some(vec!["llama_cpp".to_string()]),
        ArtifactFormat::Safetensors => infer_runtime_hint(&state.http_client, &repo)
            .await
            .or_else(|| Some(vec!["safetensors_cpp".to_string()])),
    };
    let manifest_quantization = match selection.format {
        ArtifactFormat::Gguf => infer_quantization_from_filename(&selection.filename),
        ArtifactFormat::Safetensors => None,
    };

    let base_url = hf_base_url();
    let mut files: Vec<ManifestFile> = Vec::new();

    match selection.format {
        ArtifactFormat::Gguf => {
            files.push(ManifestFile {
                name: "model.gguf".to_string(),
                priority: None,
                runtimes: runtime_hint.clone(),
                url: Some(hf_resolve_url(&base_url, &repo, &selection.filename)),
                optional: None,
            });
        }
        ArtifactFormat::Safetensors => {
            if let Err(e) = require_safetensors_metadata_files(&siblings) {
                return AppError(e).into_response();
            }

            let mut names: Vec<String> =
                vec!["config.json".to_string(), "tokenizer.json".to_string()];
            names.push(selection.filename.clone());

            if is_safetensors_index_filename(&selection.filename) {
                match fetch_safetensors_index_shards(&state.http_client, &repo, &selection.filename)
                    .await
                {
                    Ok(shards) => {
                        for shard in shards {
                            if !names.contains(&shard) {
                                names.push(shard);
                            }
                        }
                    }
                    Err(e) => {
                        return AppError(e).into_response();
                    }
                }
            }

            for name in names {
                files.push(ManifestFile {
                    name: name.clone(),
                    priority: manifest_file_priority(&name),
                    runtimes: runtime_hint.clone(),
                    url: Some(hf_resolve_url(&base_url, &repo, &name)),
                    optional: None,
                });
            }
        }
    }

    if let Some(metal_path) = find_metal_artifact(&siblings) {
        files.push(ManifestFile {
            name: "model.metal.bin".to_string(),
            priority: manifest_file_priority("model.metal.bin"),
            runtimes: runtime_hint.clone(),
            url: Some(hf_resolve_url(&base_url, &repo, &metal_path)),
            optional: None,
        });
    }

    let body = serde_json::to_string(&Manifest {
        format: manifest_format_label(selection.format).to_string(),
        files,
        quantization: manifest_quantization,
    })
    .unwrap_or_else(|_| "{\"format\":\"unknown\",\"files\":[]}".into());
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("Response builder should not fail with valid status and string body")
}
