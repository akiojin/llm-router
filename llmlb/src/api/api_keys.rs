//! APIキー管理API
//!
//! 認証済みユーザーが自分自身のAPIキーを管理するためのAPI。

use crate::common::auth::{ApiKey, ApiKeyPermission, ApiKeyWithPlaintext, Claims, UserRole};
use crate::common::error::{CommonError, LbError};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};

use super::error::AppError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// APIキー作成リクエスト
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
    /// 付与する権限（adminのみ指定可）
    #[serde(default)]
    pub permissions: Option<Vec<ApiKeyPermission>>,
    /// 旧互換: `scopes` は廃止
    #[serde(default)]
    pub scopes: Option<serde_json::Value>,
}

/// APIキーレスポンス（key_hash除外）
#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    /// APIキーID
    pub id: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: Option<String>,
    /// キーの名前
    pub name: String,
    /// 作成者のユーザーID
    pub created_by: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(api_key: ApiKey) -> Self {
        ApiKeyResponse {
            id: api_key.id.to_string(),
            key_prefix: api_key.key_prefix,
            name: api_key.name,
            created_by: api_key.created_by.to_string(),
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            permissions: api_key.permissions,
        }
    }
}

/// APIキー作成レスポンス（平文キー含む）
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    /// APIキーID
    pub id: String,
    /// 平文のAPIキー（発行時のみ表示）
    pub key: String,
    /// キーの先頭部分（表示用）
    pub key_prefix: String,
    /// キーの名前
    pub name: String,
    /// 作成日時
    pub created_at: String,
    /// 有効期限
    pub expires_at: Option<String>,
    /// 付与された権限
    pub permissions: Vec<ApiKeyPermission>,
}

impl From<ApiKeyWithPlaintext> for CreateApiKeyResponse {
    fn from(api_key: ApiKeyWithPlaintext) -> Self {
        CreateApiKeyResponse {
            id: api_key.id.to_string(),
            key: api_key.key,
            key_prefix: api_key.key_prefix,
            name: api_key.name,
            created_at: api_key.created_at.to_rfc3339(),
            expires_at: api_key.expires_at.map(|dt| dt.to_rfc3339()),
            permissions: api_key.permissions,
        }
    }
}

/// APIキー一覧レスポンス
#[derive(Debug, Serialize)]
pub struct ListApiKeysResponse {
    /// APIキー一覧
    pub api_keys: Vec<ApiKeyResponse>,
}

/// APIキー更新リクエスト
#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    /// キーの名前
    pub name: String,
    /// 有効期限（RFC3339形式、オプション）
    pub expires_at: Option<String>,
}

fn default_viewer_api_key_permissions() -> Vec<ApiKeyPermission> {
    vec![
        ApiKeyPermission::OpenaiInference,
        ApiKeyPermission::OpenaiModelsRead,
    ]
}

#[allow(clippy::result_large_err)]
fn resolve_permissions_for_role(
    role: UserRole,
    requested_permissions: Option<Vec<ApiKeyPermission>>,
) -> Result<Vec<ApiKeyPermission>, Response> {
    match role {
        UserRole::Admin => {
            let permissions = requested_permissions.ok_or_else(|| {
                AppError(LbError::Common(CommonError::Validation(
                    "Field 'permissions' is required for admin users.".to_string(),
                )))
                .into_response()
            })?;

            if permissions.is_empty() {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "Field 'permissions' must contain at least one permission.".to_string(),
                )))
                .into_response());
            }

            Ok(permissions)
        }
        UserRole::Viewer => {
            if requested_permissions.is_some() {
                return Err(AppError(LbError::Common(CommonError::Validation(
                    "Viewer users cannot provide 'permissions'; viewer keys always use fixed OpenAI permissions."
                        .to_string(),
                )))
                .into_response());
            }

            Ok(default_viewer_api_key_permissions())
        }
    }
}

#[allow(clippy::result_large_err)]
fn parse_user_id_from_claims(claims: &Claims) -> Result<Uuid, Response> {
    claims.sub.parse::<Uuid>().map_err(|e| {
        tracing::error!("Failed to parse user ID: {}", e);
        AppError(LbError::Internal(format!("Failed to parse user ID: {}", e))).into_response()
    })
}

#[allow(clippy::result_large_err)]
fn parse_expires_at(
    expires_at: Option<&String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, Response> {
    match expires_at {
        Some(expires_at_str) => Ok(Some(
            chrono::DateTime::parse_from_rfc3339(expires_at_str)
                .map_err(|e| {
                    tracing::warn!("Invalid expires_at format: {}", e);
                    AppError(LbError::Common(CommonError::Validation(
                        "Invalid expires_at format".to_string(),
                    )))
                    .into_response()
                })?
                .with_timezone(&chrono::Utc),
        )),
        None => Ok(None),
    }
}

/// GET /api/me/api-keys - 自分のAPIキー一覧取得
pub async fn list_api_keys(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
) -> Result<Json<ListApiKeysResponse>, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;

    let api_keys = crate::db::api_keys::list_by_creator(&app_state.db_pool, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list API keys: {}", e);
            AppError(LbError::Database(format!("Failed to list API keys: {}", e))).into_response()
        })?;

    Ok(Json(ListApiKeysResponse {
        api_keys: api_keys.into_iter().map(ApiKeyResponse::from).collect(),
    }))
}

/// POST /api/me/api-keys - APIキー発行
pub async fn create_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), Response> {
    if request.scopes.is_some() {
        return Err(AppError(LbError::Common(CommonError::Validation(
            "Field 'scopes' is deprecated and not accepted.".to_string(),
        )))
        .into_response());
    }

    let permissions = resolve_permissions_for_role(claims.role, request.permissions)?;
    let user_id = parse_user_id_from_claims(&claims)?;
    let expires_at = parse_expires_at(request.expires_at.as_ref())?;

    let api_key = crate::db::api_keys::create(
        &app_state.db_pool,
        &request.name,
        user_id,
        expires_at,
        permissions,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create API key: {}", e);
        AppError(e).into_response()
    })?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse::from(api_key)),
    ))
}

/// PUT /api/me/api-keys/:id - 自分のAPIキー更新
pub async fn update_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;
    let expires_at = parse_expires_at(request.expires_at.as_ref())?;

    let updated = crate::db::api_keys::update_by_creator(
        &app_state.db_pool,
        key_id,
        user_id,
        &request.name,
        expires_at,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to update API key: {}", e);
        AppError(e).into_response()
    })?;

    match updated {
        Some(api_key) => Ok(Json(ApiKeyResponse::from(api_key))),
        None => Err(AppError(LbError::NotFound("API key not found".to_string())).into_response()),
    }
}

/// DELETE /api/me/api-keys/:id - 自分のAPIキー削除
pub async fn delete_api_key(
    Extension(claims): Extension<Claims>,
    State(app_state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, Response> {
    let user_id = parse_user_id_from_claims(&claims)?;

    let deleted = crate::db::api_keys::delete_by_creator(&app_state.db_pool, key_id, user_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete API key: {}", e);
            AppError(LbError::Database(format!(
                "Failed to delete API key: {}",
                e
            )))
            .into_response()
        })?;

    if !deleted {
        return Err(AppError(LbError::NotFound("API key not found".to_string())).into_response());
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Utc};

    // --- CreateApiKeyRequest deserialization ---

    #[test]
    fn create_api_key_request_minimal() {
        let json = r#"{"name":"my-key"}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-key");
        assert!(req.expires_at.is_none());
        assert!(req.permissions.is_none());
        assert!(req.scopes.is_none());
    }

    #[test]
    fn create_api_key_request_with_expires_at() {
        let json = r#"{"name":"key1","expires_at":"2026-12-31T23:59:59Z"}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.expires_at, Some("2026-12-31T23:59:59Z".to_string()));
    }

    #[test]
    fn create_api_key_request_with_permissions() {
        let json = r#"{"name":"key2","permissions":["openai.inference","endpoints.read"]}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        let perms = req.permissions.unwrap();
        assert_eq!(perms.len(), 2);
        assert_eq!(perms[0], ApiKeyPermission::OpenaiInference);
        assert_eq!(perms[1], ApiKeyPermission::EndpointsRead);
    }

    #[test]
    fn create_api_key_request_with_scopes_deprecated() {
        let json = r#"{"name":"key3","scopes":["read","write"]}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert!(req.scopes.is_some());
    }

    #[test]
    fn create_api_key_request_missing_name_fails() {
        let json = r#"{"expires_at":"2026-12-31T23:59:59Z"}"#;
        let result = serde_json::from_str::<CreateApiKeyRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn create_api_key_request_empty_name() {
        let json = r#"{"name":""}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "");
    }

    // --- UpdateApiKeyRequest deserialization ---

    #[test]
    fn update_api_key_request_all_fields() {
        let json = r#"{"name":"updated-key","expires_at":"2027-01-01T00:00:00Z"}"#;
        let req: UpdateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "updated-key");
        assert_eq!(req.expires_at, Some("2027-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn update_api_key_request_name_only() {
        let json = r#"{"name":"just-name"}"#;
        let req: UpdateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "just-name");
        assert!(req.expires_at.is_none());
    }

    #[test]
    fn update_api_key_request_missing_name_fails() {
        let json = r#"{"expires_at":"2027-01-01T00:00:00Z"}"#;
        let result = serde_json::from_str::<UpdateApiKeyRequest>(json);
        assert!(result.is_err());
    }

    // --- ApiKeyResponse serialization / From<ApiKey> ---

    #[test]
    fn api_key_response_from_api_key() {
        let now = Utc::now();
        let key = ApiKey {
            id: Uuid::new_v4(),
            key_hash: "sha256hash".to_string(),
            key_prefix: Some("sk-abc".to_string()),
            name: "test-key".to_string(),
            created_by: Uuid::new_v4(),
            created_at: now,
            expires_at: None,
            permissions: vec![ApiKeyPermission::OpenaiInference],
        };
        let resp = ApiKeyResponse::from(key.clone());
        assert_eq!(resp.id, key.id.to_string());
        assert_eq!(resp.key_prefix, Some("sk-abc".to_string()));
        assert_eq!(resp.name, "test-key");
        assert_eq!(resp.created_by, key.created_by.to_string());
        assert!(resp.expires_at.is_none());
        assert_eq!(resp.permissions.len(), 1);
        assert_eq!(resp.permissions[0], ApiKeyPermission::OpenaiInference);
    }

    #[test]
    fn api_key_response_with_expires_at() {
        let now = Utc::now();
        let key = ApiKey {
            id: Uuid::new_v4(),
            key_hash: "hash".to_string(),
            key_prefix: None,
            name: "expiring-key".to_string(),
            created_by: Uuid::new_v4(),
            created_at: now,
            expires_at: Some(now),
            permissions: vec![],
        };
        let resp = ApiKeyResponse::from(key);
        assert!(resp.expires_at.is_some());
        assert!(resp.expires_at.unwrap().contains('T'));
    }

    #[test]
    fn api_key_response_serialization() {
        let resp = ApiKeyResponse {
            id: "key-id-1".to_string(),
            key_prefix: Some("sk-ab".to_string()),
            name: "my-key".to_string(),
            created_by: "user-id-1".to_string(),
            created_at: "2025-01-01T00:00:00+00:00".to_string(),
            expires_at: None,
            permissions: vec![
                ApiKeyPermission::OpenaiInference,
                ApiKeyPermission::OpenaiModelsRead,
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":\"key-id-1\""));
        assert!(json.contains("\"name\":\"my-key\""));
        assert!(json.contains("\"openai.inference\""));
        assert!(json.contains("\"openai.models.read\""));
    }

    // --- CreateApiKeyResponse serialization / From<ApiKeyWithPlaintext> ---

    #[test]
    fn create_api_key_response_from_plaintext() {
        let now = Utc::now();
        let key = ApiKeyWithPlaintext {
            id: Uuid::new_v4(),
            key: "sk-full-plaintext-key-value".to_string(),
            key_prefix: "sk-full".to_string(),
            name: "new-key".to_string(),
            created_at: now,
            expires_at: None,
            permissions: vec![ApiKeyPermission::EndpointsRead],
        };
        let resp = CreateApiKeyResponse::from(key.clone());
        assert_eq!(resp.id, key.id.to_string());
        assert_eq!(resp.key, "sk-full-plaintext-key-value");
        assert_eq!(resp.key_prefix, "sk-full");
        assert_eq!(resp.name, "new-key");
        assert!(resp.expires_at.is_none());
        assert_eq!(resp.permissions, vec![ApiKeyPermission::EndpointsRead]);
    }

    #[test]
    fn create_api_key_response_with_expiry() {
        let now = Utc::now();
        let key = ApiKeyWithPlaintext {
            id: Uuid::new_v4(),
            key: "sk-key".to_string(),
            key_prefix: "sk-ke".to_string(),
            name: "expiring".to_string(),
            created_at: now,
            expires_at: Some(now),
            permissions: vec![],
        };
        let resp = CreateApiKeyResponse::from(key);
        assert!(resp.expires_at.is_some());
    }

    // --- ListApiKeysResponse serialization ---

    #[test]
    fn list_api_keys_response_empty() {
        let resp = ListApiKeysResponse { api_keys: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"api_keys\":[]"));
    }

    #[test]
    fn list_api_keys_response_multiple() {
        let resp = ListApiKeysResponse {
            api_keys: vec![
                ApiKeyResponse {
                    id: "1".to_string(),
                    key_prefix: Some("sk-a".to_string()),
                    name: "key-a".to_string(),
                    created_by: "user-1".to_string(),
                    created_at: "2025-01-01T00:00:00+00:00".to_string(),
                    expires_at: None,
                    permissions: vec![],
                },
                ApiKeyResponse {
                    id: "2".to_string(),
                    key_prefix: None,
                    name: "key-b".to_string(),
                    created_by: "user-2".to_string(),
                    created_at: "2025-06-01T00:00:00+00:00".to_string(),
                    expires_at: Some("2026-06-01T00:00:00+00:00".to_string()),
                    permissions: vec![ApiKeyPermission::OpenaiInference],
                },
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("key-a"));
        assert!(json.contains("key-b"));
    }

    // --- default_viewer_api_key_permissions ---

    #[test]
    fn default_viewer_permissions_contains_two_permissions() {
        let perms = default_viewer_api_key_permissions();
        assert_eq!(perms.len(), 2);
        assert!(perms.contains(&ApiKeyPermission::OpenaiInference));
        assert!(perms.contains(&ApiKeyPermission::OpenaiModelsRead));
    }

    // --- resolve_permissions_for_role ---

    #[test]
    fn resolve_permissions_admin_with_valid_permissions() {
        let perms = resolve_permissions_for_role(
            UserRole::Admin,
            Some(vec![
                ApiKeyPermission::OpenaiInference,
                ApiKeyPermission::EndpointsRead,
            ]),
        );
        assert!(perms.is_ok());
        let perms = perms.unwrap();
        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn resolve_permissions_admin_without_permissions_fails() {
        let result = resolve_permissions_for_role(UserRole::Admin, None);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_permissions_admin_empty_permissions_fails() {
        let result = resolve_permissions_for_role(UserRole::Admin, Some(vec![]));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_permissions_viewer_without_permissions() {
        let perms = resolve_permissions_for_role(UserRole::Viewer, None);
        assert!(perms.is_ok());
        let perms = perms.unwrap();
        assert_eq!(perms, default_viewer_api_key_permissions());
    }

    #[test]
    fn resolve_permissions_viewer_with_permissions_fails() {
        let result = resolve_permissions_for_role(
            UserRole::Viewer,
            Some(vec![ApiKeyPermission::OpenaiInference]),
        );
        assert!(result.is_err());
    }

    // --- parse_user_id_from_claims ---

    #[test]
    fn parse_user_id_valid_uuid() {
        let id = Uuid::new_v4();
        let claims = Claims {
            sub: id.to_string(),
            role: UserRole::Admin,
            exp: 9999999999,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };
        let result = parse_user_id_from_claims(&claims);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), id);
    }

    #[test]
    fn parse_user_id_invalid_uuid_fails() {
        let claims = Claims {
            sub: "not-a-uuid".to_string(),
            role: UserRole::Admin,
            exp: 9999999999,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };
        let result = parse_user_id_from_claims(&claims);
        assert!(result.is_err());
    }

    #[test]
    fn parse_user_id_empty_string_fails() {
        let claims = Claims {
            sub: "".to_string(),
            role: UserRole::Viewer,
            exp: 9999999999,
            must_change_password: false,
            password_changed_at: 0,
            username: None,
        };
        let result = parse_user_id_from_claims(&claims);
        assert!(result.is_err());
    }

    // --- parse_expires_at ---

    #[test]
    fn parse_expires_at_none() {
        let result = parse_expires_at(None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn parse_expires_at_valid_rfc3339() {
        let dt_str = "2026-12-31T23:59:59+00:00".to_string();
        let result = parse_expires_at(Some(&dt_str));
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn parse_expires_at_valid_rfc3339_z() {
        let dt_str = "2026-06-15T12:00:00Z".to_string();
        let result = parse_expires_at(Some(&dt_str));
        assert!(result.is_ok());
        let parsed = result.unwrap().unwrap();
        assert_eq!(parsed.month(), 6);
    }

    #[test]
    fn parse_expires_at_invalid_format_fails() {
        let dt_str = "not-a-date".to_string();
        let result = parse_expires_at(Some(&dt_str));
        assert!(result.is_err());
    }

    #[test]
    fn parse_expires_at_empty_string_fails() {
        let dt_str = "".to_string();
        let result = parse_expires_at(Some(&dt_str));
        assert!(result.is_err());
    }
}
