//! endpoint_models テーブルの CRUD（エンドポイントに紐づくモデル一覧）
//!
//! arch-review [H6]: db/endpoints.rs から endpoint_models 関連の DB 操作を分離。

use crate::types::endpoint::{EndpointModel, SupportedAPI};
use sqlx::SqlitePool;
use uuid::Uuid;

// --- EndpointModel CRUD ---

/// エンドポイントにモデルを追加
pub async fn add_endpoint_model(
    pool: &SqlitePool,
    model: &EndpointModel,
) -> Result<(), sqlx::Error> {
    let capabilities_json = model
        .capabilities
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_default());
    let supported_apis_json = serde_json::to_string(&model.supported_apis).unwrap_or_default();
    let last_checked = model.last_checked.map(|dt| dt.to_rfc3339());

    sqlx::query(
        r#"
        INSERT OR REPLACE INTO endpoint_models (endpoint_id, model_id, capabilities, max_tokens, last_checked, supported_apis, canonical_name)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(model.endpoint_id.to_string())
    .bind(&model.model_id)
    .bind(&capabilities_json)
    .bind(model.max_tokens.map(|v| v as i32))
    .bind(&last_checked)
    .bind(&supported_apis_json)
    .bind(&model.canonical_name)
    .execute(pool)
    .await?;

    Ok(())
}

/// エンドポイントのモデル情報を更新
pub async fn update_endpoint_model(
    pool: &SqlitePool,
    model: &EndpointModel,
) -> Result<bool, sqlx::Error> {
    let capabilities_json = model
        .capabilities
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_default());
    let supported_apis_json = serde_json::to_string(&model.supported_apis).unwrap_or_default();

    let result = sqlx::query(
        r#"
        UPDATE endpoint_models
        SET capabilities = ?, max_tokens = ?, last_checked = ?, supported_apis = ?, canonical_name = ?
        WHERE endpoint_id = ? AND model_id = ?
        "#,
    )
    .bind(&capabilities_json)
    .bind(model.max_tokens.map(|v| v as i32))
    .bind(model.last_checked.map(|dt| dt.to_rfc3339()))
    .bind(&supported_apis_json)
    .bind(&model.canonical_name)
    .bind(model.endpoint_id.to_string())
    .bind(&model.model_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// モデルのmax_tokensのみを更新（SPEC-e8e9326e）
///
/// メタデータ取得後にcontext_lengthをmax_tokensとして保存する。
pub async fn update_model_max_tokens(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
    max_tokens: u32,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoint_models
        SET max_tokens = ?
        WHERE endpoint_id = ? AND model_id = ?
        "#,
    )
    .bind(max_tokens as i32)
    .bind(endpoint_id.to_string())
    .bind(model_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのモデル一覧を取得
pub async fn list_endpoint_models(
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<Vec<EndpointModel>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointModelRow>(
        r#"
        SELECT endpoint_id, model_id, capabilities, max_tokens, last_checked, supported_apis, canonical_name
        FROM endpoint_models
        WHERE endpoint_id = ?
        "#,
    )
    .bind(endpoint_id.to_string())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// エンドポイントからモデルを削除
pub async fn delete_endpoint_model(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM endpoint_models
        WHERE endpoint_id = ? AND model_id = ?
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(model_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントの全モデルを削除
pub async fn delete_all_endpoint_models(
    pool: &SqlitePool,
    endpoint_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM endpoint_models WHERE endpoint_id = ?")
        .bind(endpoint_id.to_string())
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

#[derive(sqlx::FromRow)]
struct EndpointModelRow {
    endpoint_id: String,
    model_id: String,
    capabilities: Option<String>,
    max_tokens: Option<i32>,
    last_checked: Option<String>,
    supported_apis: Option<String>,
    canonical_name: Option<String>,
}

impl From<EndpointModelRow> for EndpointModel {
    fn from(row: EndpointModelRow) -> Self {
        EndpointModel {
            endpoint_id: Uuid::parse_str(&row.endpoint_id).unwrap_or_default(),
            model_id: row.model_id,
            capabilities: row.capabilities.and_then(|s| serde_json::from_str(&s).ok()),
            max_tokens: row.max_tokens.map(|v| v as u32),
            last_checked: row
                .last_checked
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            supported_apis: row
                .supported_apis
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| vec![SupportedAPI::ChatCompletions]),
            canonical_name: row.canonical_name,
        }
    }
}
