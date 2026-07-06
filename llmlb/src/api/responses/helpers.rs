//! /v1/responses のリクエストフィールド抽出と HTTP レスポンス構築ヘルパー
//!
//! arch-review [H6] round2: api/responses.rs からステートレスなヘルパーを分離。

use crate::api::error::AppError;
use crate::common::error::LbError;
use serde_json::Value;

/// リクエストからモデル名を抽出
pub(crate) fn extract_model(payload: &Value) -> Result<String, AppError> {
    payload["model"].as_str().map(String::from).ok_or_else(|| {
        AppError::from(LbError::Common(
            crate::common::error::CommonError::Validation("Missing required field: model".into()),
        ))
    })
}

/// リクエストからstreamフラグを抽出
pub(crate) fn extract_stream(payload: &Value) -> bool {
    payload["stream"].as_bool().unwrap_or(false)
}
