//! HuggingFace アーティファクト解決ヘルパー
//!
//! arch-review [H6/M13]: api/models.rs が 2800 行超の god-object 化していたため、
//! HuggingFace リポジトリからのアーティファクト（GGUF / safetensors）解決・
//! 分類・マニフェスト構築に関する凝集した関心をこのサブモジュールへ分離した。
//! いずれも AppState やモデル DB に依存しない純粋／HTTP ヘルパーで、親モジュール
//! （register_model / manifest 配信）とそのテストから利用される。

use crate::common::error::{CommonError, LbError};
use serde::{Deserialize, Serialize};

mod hf_client;
pub(crate) use hf_client::*;

// ===== HuggingFace helpers =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtifactFormat {
    Gguf,
    Safetensors,
}

pub(crate) struct ArtifactSelection {
    pub(crate) format: ArtifactFormat,
    pub(crate) filename: String,
}

pub(crate) fn is_gguf_filename(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".gguf")
}

pub(crate) fn is_safetensors_index_filename(filename: &str) -> bool {
    filename
        .to_ascii_lowercase()
        .ends_with(".safetensors.index.json")
}

pub(crate) fn is_safetensors_filename(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    lower.ends_with(".safetensors") || lower.ends_with(".safetensors.index.json")
}

pub(crate) fn infer_safetensors_index_from_shard(filename: &str) -> Option<String> {
    if is_safetensors_index_filename(filename) {
        return None;
    }
    if !filename.to_ascii_lowercase().ends_with(".safetensors") {
        return None;
    }

    let (dir, file) = match filename.rsplit_once('/') {
        Some((dir, file)) => (format!("{}/", dir), file),
        None => ("".to_string(), filename),
    };

    let stem = file.strip_suffix(".safetensors")?;
    let (left, total) = stem.rsplit_once("-of-")?;
    if left.is_empty() || total.is_empty() {
        return None;
    }
    if !total.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (prefix, shard) = left.rsplit_once('-')?;
    if prefix.is_empty() || shard.is_empty() {
        return None;
    }
    if !shard.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some(format!("{}{}.safetensors.index.json", dir, prefix))
}

pub(crate) fn sibling_size_bytes(s: &HfSibling) -> u64 {
    s.size
        .or_else(|| s.lfs.as_ref().and_then(|l| l.size))
        .unwrap_or(0)
}

pub(crate) fn has_sibling(siblings: &[HfSibling], filename: &str) -> bool {
    siblings.iter().any(|s| s.rfilename == filename)
}

pub(crate) fn require_safetensors_metadata_files(siblings: &[HfSibling]) -> Result<(), LbError> {
    let has_config = has_sibling(siblings, "config.json");
    let has_tokenizer = has_sibling(siblings, "tokenizer.json");
    if !has_config || !has_tokenizer {
        return Err(LbError::Common(CommonError::Validation(
            "config.json and tokenizer.json are required for safetensors models".into(),
        )));
    }
    Ok(())
}

pub(crate) fn resolve_primary_artifact(
    siblings: &[HfSibling],
    filename_hint: Option<String>,
) -> Result<ArtifactSelection, LbError> {
    if let Some(filename) = filename_hint {
        if !has_sibling(siblings, &filename) {
            return Err(LbError::Common(CommonError::Validation(
                "Specified file not found in repository".into(),
            )));
        }
        if is_gguf_filename(&filename) {
            return Ok(ArtifactSelection {
                format: ArtifactFormat::Gguf,
                filename,
            });
        }
        if is_safetensors_filename(&filename) {
            require_safetensors_metadata_files(siblings)?;
            let resolved = resolve_safetensors_primary(siblings, Some(filename))?;
            return Ok(ArtifactSelection {
                format: ArtifactFormat::Safetensors,
                filename: resolved,
            });
        }
        return Err(LbError::Common(CommonError::Validation(
            "filename must be a .gguf or .safetensors file".into(),
        )));
    }

    let ggufs: Vec<_> = siblings
        .iter()
        .filter(|s| is_gguf_filename(&s.rfilename))
        .map(|s| s.rfilename.clone())
        .collect();
    let safetensors: Vec<_> = siblings
        .iter()
        .filter(|s| is_safetensors_filename(&s.rfilename))
        .map(|s| s.rfilename.clone())
        .collect();

    if !ggufs.is_empty() && !safetensors.is_empty() {
        return Err(LbError::Common(CommonError::Validation(
            "Multiple artifact types found; specify filename".into(),
        )));
    }

    if !ggufs.is_empty() {
        if ggufs.len() == 1 {
            return Ok(ArtifactSelection {
                format: ArtifactFormat::Gguf,
                filename: ggufs[0].clone(),
            });
        }
        return Err(LbError::Common(CommonError::Validation(
            "Multiple GGUF files found; specify filename".into(),
        )));
    }

    if !safetensors.is_empty() {
        require_safetensors_metadata_files(siblings)?;
        let filename = resolve_safetensors_primary(siblings, None)?;
        return Ok(ArtifactSelection {
            format: ArtifactFormat::Safetensors,
            filename,
        });
    }

    Err(LbError::Common(CommonError::Validation(
        "No supported model artifacts found (safetensors/gguf)".into(),
    )))
}

pub(crate) fn extract_runtime_from_config(value: &serde_json::Value) -> Option<String> {
    if let Some(arr) = value.get("architectures").and_then(|x| x.as_array()) {
        for a in arr {
            let Some(s) = a.as_str() else { continue };
            if s.contains("GptOss") || s.contains("GPTOSS") {
                return Some("gptoss_cpp".to_string());
            }
            if s.contains("Nemotron") {
                return Some("nemotron_cpp".to_string());
            }
        }
    }

    if let Some(mt) = value.get("model_type").and_then(|x| x.as_str()) {
        let mt = mt.to_ascii_lowercase();
        if mt.contains("gpt_oss") || mt.contains("gptoss") {
            return Some("gptoss_cpp".to_string());
        }
        if mt.contains("nemotron") {
            return Some("nemotron_cpp".to_string());
        }
    }
    None
}

pub(crate) async fn infer_runtime_hint(
    http_client: &reqwest::Client,
    repo: &str,
) -> Option<Vec<String>> {
    if !repo.is_empty() {
        if let Ok(bytes) = fetch_hf_file_bytes(http_client, repo, "config.json").await {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(rt) = extract_runtime_from_config(&v) {
                    return Some(vec![rt]);
                }
            }
        }
    }
    None
}

pub(crate) fn find_metal_artifact(siblings: &[HfSibling]) -> Option<String> {
    let candidates = ["model.metal.bin", "metal/model.bin"];
    for name in candidates {
        if has_sibling(siblings, name) {
            return Some(name.to_string());
        }
    }
    None
}

pub(crate) fn validate_artifact_path(path: &str) -> Result<(), LbError> {
    if path.is_empty() {
        return Err(LbError::Common(CommonError::Validation(
            "filename must not be empty".into(),
        )));
    }
    if path.contains("..") || path.contains('\0') {
        return Err(LbError::Common(CommonError::Validation(
            "filename contains invalid path segment".into(),
        )));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(LbError::Common(CommonError::Validation(
            "filename must be a relative path".into(),
        )));
    }
    Ok(())
}

pub(crate) fn extract_filename_from_hf_url(input: &str) -> Option<String> {
    for marker in ["/resolve/", "/blob/", "/raw/"] {
        if let Some(rest) = input.split(marker).nth(1) {
            let mut parts = rest.splitn(2, '/');
            let _revision = parts.next();
            if let Some(path) = parts.next() {
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

#[derive(Serialize)]
pub(crate) struct ManifestFile {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) runtimes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) optional: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct Manifest {
    pub(crate) format: String,
    pub(crate) files: Vec<ManifestFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) quantization: Option<String>,
}

pub(crate) fn manifest_format_label(format: ArtifactFormat) -> &'static str {
    match format {
        ArtifactFormat::Gguf => "gguf",
        ArtifactFormat::Safetensors => "safetensors",
    }
}

pub(crate) fn manifest_file_priority(name: &str) -> Option<i32> {
    match name {
        "config.json" | "tokenizer.json" => Some(10),
        _ if is_safetensors_index_filename(name) => Some(5),
        "model.metal.bin" => Some(5),
        _ => None,
    }
}

pub(crate) fn is_quantization_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    let upper = token.to_ascii_uppercase();
    let has_digit = |s: &str| s.chars().any(|c| c.is_ascii_digit());
    let starts_with_digit = |s: &str| s.chars().next().is_some_and(|c| c.is_ascii_digit());

    if let Some(rest) = upper.strip_prefix("IQ") {
        return starts_with_digit(rest);
    }
    if let Some(rest) = upper.strip_prefix('Q') {
        return starts_with_digit(rest);
    }
    if let Some(rest) = upper.strip_prefix("BF") {
        return starts_with_digit(rest);
    }
    if let Some(rest) = upper.strip_prefix("FP") {
        return starts_with_digit(rest);
    }
    if let Some(rest) = upper.strip_prefix('F') {
        return starts_with_digit(rest);
    }
    if let Some(rest) = upper.strip_prefix("MX") {
        return has_digit(rest);
    }
    false
}

pub(crate) fn infer_quantization_from_filename(filename: &str) -> Option<String> {
    let file = std::path::Path::new(filename)
        .file_name()?
        .to_string_lossy();
    if !file.to_ascii_lowercase().ends_with(".gguf") {
        return None;
    }
    let stem = file.strip_suffix(".gguf").unwrap_or(&file);
    for token in stem.split(['-', '.']).rev() {
        if is_quantization_token(token) {
            return Some(token.to_string());
        }
    }
    None
}
pub(crate) fn resolve_safetensors_primary(
    siblings: &[HfSibling],
    requested: Option<String>,
) -> Result<String, LbError> {
    if let Some(filename) = requested {
        if !is_safetensors_filename(&filename) {
            return Err(LbError::Common(CommonError::Validation(
                "filename must be a safetensors or safetensors index file".into(),
            )));
        }
        if !has_sibling(siblings, &filename) {
            return Err(LbError::Common(CommonError::Validation(
                "Specified safetensors file not found in repository".into(),
            )));
        }
        if !is_safetensors_index_filename(&filename) {
            if let Some(candidate) = infer_safetensors_index_from_shard(&filename) {
                if has_sibling(siblings, &candidate) {
                    return Ok(candidate);
                }
                let index_files: Vec<_> = siblings
                    .iter()
                    .map(|s| s.rfilename.clone())
                    .filter(|f| is_safetensors_index_filename(f))
                    .collect();
                if index_files.len() == 1 {
                    return Ok(index_files[0].clone());
                }
                if index_files.len() > 1 {
                    return Err(LbError::Common(CommonError::Validation(
                        "Multiple safetensors index files found; specify filename".into(),
                    )));
                }
            }
        }
        return Ok(filename);
    }

    let index_files: Vec<_> = siblings
        .iter()
        .map(|s| s.rfilename.clone())
        .filter(|f| is_safetensors_index_filename(f))
        .collect();
    if index_files.len() == 1 {
        return Ok(index_files[0].clone());
    }
    if index_files.len() > 1 {
        return Err(LbError::Common(CommonError::Validation(
            "Multiple safetensors index files found; specify filename".into(),
        )));
    }

    let st_files: Vec<_> = siblings
        .iter()
        .map(|s| s.rfilename.clone())
        .filter(|f| {
            // .safetensors だが index は除外
            f.to_ascii_lowercase().ends_with(".safetensors") && !is_safetensors_index_filename(f)
        })
        .collect();
    if st_files.len() == 1 {
        return Ok(st_files[0].clone());
    }
    if st_files.is_empty() {
        return Err(LbError::Common(CommonError::Validation(
            "No safetensors file found in repository".into(),
        )));
    }
    Err(LbError::Common(CommonError::Validation(
        "Multiple safetensors files found; specify filename".into(),
    )))
}

/// HFリポジトリのsiblings情報
#[derive(Deserialize)]
pub(crate) struct HfSibling {
    #[serde(rename = "rfilename")]
    pub(crate) rfilename: String,
    /// ファイルサイズ（オプション）
    #[serde(default)]
    pub(crate) size: Option<u64>,
    /// LFS情報（オプション）
    pub(crate) lfs: Option<HfLfs>,
}

/// HF LFS情報
#[derive(Deserialize)]
pub(crate) struct HfLfs {
    /// ファイルサイズ
    pub(crate) size: Option<u64>,
}
