//! モデル管理API
//!
//! モデル一覧取得、登録、マニフェスト配信のエンドポイント
//!
//! このモジュールはEndpointRegistry/Endpoint型を使用しています。

use super::error::AppError;
use crate::common::error::{CommonError, LbError};
use crate::{db::models::ModelStorage, registry::models::ModelInfo, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

mod artifacts;
mod hf_info;
mod listing;
mod manifest;
mod register;
pub use listing::*;
pub(crate) use manifest::get_model_registry_manifest;
pub(crate) use register::register_model;

// NOTE: supported_models.json は廃止されました (2026-01-25)
// モデルアーキテクチャ認識はエンドポイント（xLLM）側の config.json ベースで行われます
// 詳細は SPEC-6cd7f960, SPEC-48678000 を参照

/// モデル名の妥当性を検証
///
/// 有効なモデル名の形式:
/// - `gpt-oss-20b`, `mistral-7b-instruct-v0.2` のようなファイル名ベース形式
/// - `openai/gpt-oss-20b` のような階層形式（HuggingFace互換）
///
/// SPEC-dcaeaec4 FR-2: 階層形式を許可
fn validate_model_name(model_name: &str) -> Result<(), LbError> {
    if model_name.is_empty() {
        return Err(LbError::InvalidModelName("Model name is empty".to_string()));
    }

    // 危険なパターンを禁止（パストラバーサル対策）
    if model_name.contains("..") || model_name.contains('\0') {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name (contains dangerous pattern): {}",
            model_name
        )));
    }

    // 先頭・末尾のスラッシュは禁止
    if model_name.starts_with('/') || model_name.ends_with('/') {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name (leading/trailing slash): {}",
            model_name
        )));
    }

    // 許可する文字: 小文字英数字、'-', '_', '.', '/'（ディレクトリセパレータ）
    if !model_name.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.' || c == '/'
    }) {
        return Err(LbError::InvalidModelName(format!(
            "Invalid model name: {}",
            model_name
        )));
    }

    Ok(())
}

// NOTE: AvailableModelView, AvailableModelsResponse, Pagination, model_info_to_view() は
// /api/models/available 廃止に伴い削除されました。

// NOTE: GET /api/models/available は廃止されました。
// HFカタログは直接 https://huggingface.co を参照してください。

/// DELETE /api/models/:model_name - 登録モデル削除
///
/// 登録情報のみ削除し、Nodeは次回同期でキャッシュを削除する。
pub async fn delete_model(
    State(state): State<AppState>,
    Path(model_name): Path<String>,
) -> Result<StatusCode, AppError> {
    let storage = ModelStorage::new(state.db_pool.clone());
    if storage.load_model(&model_name).await?.is_none() {
        return Err(LbError::Common(CommonError::Validation("model not found".into())).into());
    }
    storage.delete_model(&model_name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
