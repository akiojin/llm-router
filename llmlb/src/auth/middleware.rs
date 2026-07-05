// T047-T049: 認証ミドルウェア実装

use crate::audit::middleware::AuditActorSlot;
use crate::common::auth::{ApiKeyPermission, Claims, UserRole};
use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

/// arch-review [M10]: 認証で解決したアクター（Claims / ApiKeyAuthContext）を
/// 監査ミドルウェアが request に挿入したスロットへ記録する（存在時のみ）。
/// これにより監査は response 差し替えに依存せずアクターを取得できる。
fn record_audit_actor(
    request: &Request,
    claims: Option<&Claims>,
    api_key: Option<&ApiKeyAuthContext>,
) {
    if let Some(slot) = request.extensions().get::<AuditActorSlot>() {
        slot.record(claims.cloned(), api_key.cloned());
    }
}
use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod csrf;
mod debug_keys;
mod jwt_token;
#[cfg(test)]
use csrf::{
    default_port_for_scheme, expected_origin, normalize_origin_for_compare, origin_or_referer,
};
use csrf::{extract_csrf_cookie, method_requires_csrf, origin_matches, response_sets_csrf_cookie};
pub(crate) use jwt_token::extract_jwt_cookie;
use jwt_token::{extract_jwt_from_headers, verify_jwt_claims};

/// APIキー認証済みのコンテキスト
#[derive(Debug, Clone)]
pub struct ApiKeyAuthContext {
    /// APIキーID
    pub id: Uuid,
    /// APIキー発行者のユーザーID
    pub created_by: Uuid,
    /// APIキーの権限一覧
    pub permissions: Vec<crate::common::auth::ApiKeyPermission>,
    /// APIキーの有効期限
    pub expires_at: Option<DateTime<Utc>>,
}

fn has_permission(
    permissions: &[crate::common::auth::ApiKeyPermission],
    required: crate::common::auth::ApiKeyPermission,
) -> bool {
    permissions.contains(&required)
}

async fn authenticate_api_key(
    pool: &sqlx::SqlitePool,
    api_key: &str,
) -> Result<ApiKeyAuthContext, Response> {
    if let Some(permissions) = debug_keys::debug_api_key_permissions(api_key) {
        tracing::warn!("Authenticated via debug API key (debug build only)");
        return Ok(ApiKeyAuthContext {
            id: Uuid::nil(),
            created_by: Uuid::nil(),
            permissions,
            expires_at: None,
        });
    }

    let key_hash = hash_with_sha256(api_key);
    let api_key_record = crate::db::api_keys::find_by_hash(pool, &key_hash)
        .await
        .map_err(|e| {
            tracing::warn!("API key verification failed: {}", e);
            (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()).into_response()
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()).into_response())?;

    if let Some(expires_at) = api_key_record.expires_at {
        if expires_at < chrono::Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "API key expired".to_string()).into_response());
        }
    }

    Ok(ApiKeyAuthContext {
        id: api_key_record.id,
        created_by: api_key_record.created_by,
        permissions: api_key_record.permissions,
        expires_at: api_key_record.expires_at,
    })
}

#[allow(clippy::result_large_err)]
fn extract_api_key(request: &Request) -> Result<String, Response> {
    if let Some(api_key) = request
        .headers()
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
    {
        return Ok(api_key.to_string());
    }

    if let Some(auth_header) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            return Ok(token.to_string());
        }
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid Authorization header format. Expected 'Bearer <token>'".to_string(),
        )
            .into_response());
    }

    Err((
        StatusCode::UNAUTHORIZED,
        "Missing X-API-Key header or Authorization header".to_string(),
    )
        .into_response())
}

/// Authorization ヘッダー（Bearer）または JWT Cookie からトークンを取り出す。無ければ 401。
#[allow(clippy::result_large_err)]
fn extract_bearer_or_cookie_token(headers: &HeaderMap) -> Result<String, Response> {
    if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        return auth_header
            .strip_prefix("Bearer ")
            .map(str::to_string)
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    "Invalid Authorization header format".to_string(),
                )
                    .into_response()
            });
    }
    if let Some(cookie_token) = extract_jwt_cookie(headers) {
        return Ok(cookie_token);
    }
    Err((
        StatusCode::UNAUTHORIZED,
        "Missing Authorization header or JWT cookie".to_string(),
    )
        .into_response())
}

/// パスワード変更/リセット後の旧 JWT セッションを無効化する。
///
/// token の `password_changed_at` が DB の現在値より小さい場合は 401 を返す
/// （= パスワードが変更/リセットされた後に発行前のトークンが使われている）。
/// dev 固定ログイン（nil UUID）や DB 未登録ユーザーはスキップする。
///
/// DB 参照に失敗した場合は **fail-open**（セッションを有効として扱う）。
/// 無効化は「DB の pca がトークンより新しい」と積極的に確認できた場合のみ行う。
/// DB 障害時に全認証ユーザーをロックアウトすると、ダッシュボードの
/// グレースフルデグレード（キャッシュ応答）を壊し自己 DoS となるため。
/// JWT 自体は 24h で失効するので、障害中の無効化遅延は限定的。
#[allow(clippy::result_large_err)]
pub(crate) async fn enforce_session_not_revoked(
    pool: &sqlx::SqlitePool,
    claims: &Claims,
) -> Result<(), Response> {
    let Ok(user_id) = uuid::Uuid::parse_str(&claims.sub) else {
        return Ok(());
    };
    if user_id.is_nil() {
        return Ok(());
    }
    match crate::db::users::find_by_id(pool, user_id).await {
        Ok(Some(user)) if claims.password_changed_at < user.password_changed_at => Err((
            StatusCode::UNAUTHORIZED,
            "Session revoked: please sign in again".to_string(),
        )
            .into_response()),
        Ok(_) => Ok(()),
        Err(e) => {
            // fail-open: DB 障害時はセッションを有効扱いにして可用性を優先する。
            tracing::warn!("Session revocation check skipped (DB error): {}", e);
            Ok(())
        }
    }
}

/// JWT認証ミドルウェア
///
/// Authorization ヘッダーまたは Cookie からトークンを抽出して JWT 検証を行う。
pub async fn jwt_auth_middleware(
    State(jwt_secret): State<String>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // AuthorizationヘッダーまたはCookieからトークンを取得
    let token = extract_bearer_or_cookie_token(request.headers())?;

    // JWTを検証
    let claims = verify_jwt_claims(&token, &jwt_secret)?;

    // 検証済みのClaimsをrequestの拡張データに格納
    let claims_for_response = claims.clone();
    request.extensions_mut().insert(claims);
    record_audit_actor(&request, Some(&claims_for_response), None);

    // 次のミドルウェア/ハンドラーに進む
    let mut response = next.run(request).await;
    // 監査ログミドルウェア (SPEC-8301d106) がresponse extensionsからアクター情報を取得
    response.extensions_mut().insert(claims_for_response);
    Ok(response)
}

/// JWT認証を必須とするミドルウェア（US-013: 認証必須化・匿名アクセス廃止）。
///
/// ダッシュボード/管理APIのルートで使用する。未認証リクエストは 401 を返す。
/// `AppState` から JWT secret を取り出して `jwt_auth_middleware` に委譲する。
pub async fn require_jwt_auth_middleware(
    State(app_state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    let token = extract_bearer_or_cookie_token(request.headers())?;
    let claims = verify_jwt_claims(&token, &app_state.jwt_secret)?;
    // パスワード変更/リセット後の旧セッションを無効化する
    enforce_session_not_revoked(&app_state.db_pool, &claims).await?;

    let claims_for_response = claims.clone();
    request.extensions_mut().insert(claims);
    record_audit_actor(&request, Some(&claims_for_response), None);
    let mut response = next.run(request).await;
    // 監査ログミドルウェア (SPEC-8301d106) がresponse extensionsからアクター情報を取得
    response.extensions_mut().insert(claims_for_response);
    Ok(response)
}

/// JWT claims に admin ロールを要求するミドルウェア
pub async fn require_admin_role_middleware(
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let claims = request.extensions().get::<Claims>().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Missing authenticated user claims".to_string(),
        )
            .into_response()
    })?;

    if claims.role != UserRole::Admin {
        return Err((StatusCode::FORBIDDEN, "Admin access required".to_string()).into_response());
    }

    Ok(next.run(request).await)
}

/// パスワード変更済みを要求するミドルウェア
///
/// JWTクレームの`must_change_password`が`true`の場合、403を返す。
/// `/auth/me`, `/auth/logout`, `/auth/change-password`は除外済み（ルート構成で分離）。
pub async fn require_password_changed_middleware(
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let claims = request.extensions().get::<Claims>().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Missing authenticated user claims".to_string(),
        )
            .into_response()
    })?;

    if claims.must_change_password {
        return Err((
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({"error": "password_change_required"})),
        )
            .into_response());
    }

    Ok(next.run(request).await)
}

/// CookieベースのJWT認証時にCSRFトークンを要求するミドルウェア
pub async fn csrf_protect_middleware(request: Request, next: Next) -> Result<Response, Response> {
    if !method_requires_csrf(request.method()) {
        return Ok(next.run(request).await);
    }

    let headers_snapshot = request.headers().clone();

    // ヘッダー認証（APIキー/Authorization）はCSRF対象外（CookieベースのJWT認証のみ保護する）
    if request.headers().contains_key(header::AUTHORIZATION)
        || request.headers().contains_key("X-API-Key")
    {
        return Ok(next.run(request).await);
    }

    let csrf_cookie = extract_csrf_cookie(request.headers()).ok_or_else(|| {
        (StatusCode::FORBIDDEN, "Missing CSRF cookie".to_string()).into_response()
    })?;
    let csrf_header = request
        .headers()
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            (StatusCode::FORBIDDEN, "Missing CSRF header".to_string()).into_response()
        })?;

    if csrf_cookie != csrf_header {
        return Err((StatusCode::FORBIDDEN, "Invalid CSRF token".to_string()).into_response());
    }

    if !origin_matches(request.headers()) {
        return Err((
            StatusCode::FORBIDDEN,
            "Origin validation failed".to_string(),
        )
            .into_response());
    }

    let mut response = next.run(request).await;
    if response.status().is_success() && !response_sets_csrf_cookie(&response) {
        let new_token = crate::auth::generate_random_token(32);
        let secure = super::request_is_secure(&headers_snapshot);
        let cookie = crate::auth::build_csrf_cookie(&new_token, 86400, secure);
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.parse().unwrap());
    }
    Ok(response)
}

/// APIキー認証ミドルウェア
///
/// X-API-KeyヘッダーまたはAuthorization: Bearer形式でキーを抽出してSHA-256で検証を行う
///
/// # Arguments
/// * `State(pool)` - データベース接続プール
/// * `request` - HTTPリクエスト
/// * `next` - 次のミドルウェア/ハンドラー
///
/// # Returns
/// * `Ok(Response)` - 認証成功
/// * `Err(Response)` - 認証失敗、401 Unauthorized
pub async fn api_key_auth_middleware(
    State(pool): State<sqlx::SqlitePool>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    let api_key = extract_api_key(&request)?;
    let auth_context = authenticate_api_key(&pool, &api_key).await?;
    let auth_context_for_response = auth_context.clone();
    request.extensions_mut().insert(auth_context);
    record_audit_actor(&request, None, Some(&auth_context_for_response));

    let mut response = next.run(request).await;
    // 監査ログミドルウェア (SPEC-8301d106) がresponse extensionsからアクター情報を取得
    response.extensions_mut().insert(auth_context_for_response);
    Ok(response)
}

fn anthropic_error_response(
    status: StatusCode,
    error_type: impl Into<String>,
    message: impl Into<String>,
) -> Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type.into(),
                "message": message.into()
            }
        })),
    )
        .into_response()
}

#[allow(clippy::result_large_err)]
fn require_anthropic_version_header(request: &Request) -> Result<(), Response> {
    let value = request
        .headers()
        .get("anthropic-version")
        .and_then(|header| header.to_str().ok())
        .map(str::trim);
    match value {
        Some(value) if !value.is_empty() => Ok(()),
        _ => Err(anthropic_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "Missing required header: anthropic-version",
        )),
    }
}

/// Authenticate Anthropic-native `/v1/messages` requests and preserve Anthropic error shape.
pub async fn anthropic_api_key_auth_middleware(
    State(pool): State<sqlx::SqlitePool>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    require_anthropic_version_header(&request)?;

    let api_key = extract_api_key(&request).map_err(|_| {
        anthropic_error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "Invalid or missing x-api-key",
        )
    })?;
    let auth_context = authenticate_api_key(&pool, &api_key).await.map_err(|_| {
        anthropic_error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "Invalid or expired x-api-key",
        )
    })?;
    let auth_context_for_response = auth_context.clone();
    request.extensions_mut().insert(auth_context);
    record_audit_actor(&request, None, Some(&auth_context_for_response));

    let mut response = next.run(request).await;
    response.extensions_mut().insert(auth_context_for_response);
    Ok(response)
}

/// APIキーの権限を要求するミドルウェア
pub async fn require_api_key_permission_middleware(
    State(required_permission): State<ApiKeyPermission>,
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let auth_context = request
        .extensions()
        .get::<ApiKeyAuthContext>()
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing API key authentication".to_string(),
            )
                .into_response()
        })?;

    if !has_permission(&auth_context.permissions, required_permission) {
        return Err((
            StatusCode::FORBIDDEN,
            "Insufficient API key permission".to_string(),
        )
            .into_response());
    }

    Ok(next.run(request).await)
}

/// Require an API-key permission for Anthropic-native `/v1/messages` requests.
pub async fn require_anthropic_api_key_permission_middleware(
    State(required_permission): State<ApiKeyPermission>,
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let auth_context = request
        .extensions()
        .get::<ApiKeyAuthContext>()
        .ok_or_else(|| {
            anthropic_error_response(
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "Missing API key authentication",
            )
        })?;

    if !has_permission(&auth_context.permissions, required_permission) {
        return Err(anthropic_error_response(
            StatusCode::FORBIDDEN,
            "permission_error",
            "Insufficient API key permission",
        ));
    }

    Ok(next.run(request).await)
}

/// JWTまたはAPIキー(permissions)で認証し、必要な権限を満たすことを要求するミドルウェア。
///
/// - JWTが存在する場合はJWTを優先（Authorization Bearer / Cookie）。
/// - APIキーは `X-API-Key` または `Authorization: Bearer sk_...` を許可。
///
/// NOTE:
/// - `jwt_required_role` が `Some(Admin)` の場合、JWTはadminのみ許可。
/// - APIキーは `required_permission` を必須とし、成功時に `api_key_role` で Claims を注入する。
#[derive(Clone)]
pub struct JwtOrApiKeyPermissionConfig {
    /// アプリケーション状態（DB/JWT secret 参照用）
    pub app_state: AppState,
    /// APIキーに要求する権限
    pub required_permission: ApiKeyPermission,
    /// JWTに要求するロール（Noneの場合は任意ロールを許可）
    pub jwt_required_role: Option<UserRole>,
    /// APIキー認証成功時に注入するClaimsのロール
    pub api_key_role: UserRole,
}

impl JwtOrApiKeyPermissionConfig {
    /// 設定を構築する。
    ///
    /// arch-review [L10]: create_app で 8 箇所繰り返されていた設定構築を集約する。
    pub fn new(
        app_state: AppState,
        required_permission: ApiKeyPermission,
        jwt_required_role: Option<UserRole>,
        api_key_role: UserRole,
    ) -> Self {
        Self {
            app_state,
            required_permission,
            jwt_required_role,
            api_key_role,
        }
    }
}

/// `JwtOrApiKeyPermissionConfig` に従って、JWTまたはAPIキーで認証・認可を行う。
pub async fn jwt_or_api_key_permission_middleware(
    State(config): State<JwtOrApiKeyPermissionConfig>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // JWTがあれば優先
    if let Some(token) = extract_jwt_from_headers(request.headers()) {
        let claims = verify_jwt_claims(&token, &config.app_state.jwt_secret)?;
        // パスワード変更/リセット後の旧セッションを無効化する
        enforce_session_not_revoked(&config.app_state.db_pool, &claims).await?;

        if let Some(required_role) = config.jwt_required_role {
            if claims.role != required_role {
                return Err(
                    (StatusCode::FORBIDDEN, "Admin access required".to_string()).into_response()
                );
            }
        }

        let claims_for_response = claims.clone();
        request.extensions_mut().insert(claims);
        record_audit_actor(&request, Some(&claims_for_response), None);
        let mut response = next.run(request).await;
        // 監査ログミドルウェア (SPEC-8301d106) がresponse extensionsからアクター情報を取得
        response.extensions_mut().insert(claims_for_response);
        return Ok(response);
    }

    // JWTがない/無効ならAPIキーで認証
    let api_key = extract_api_key(&request)?;
    let auth_context = authenticate_api_key(&config.app_state.db_pool, &api_key).await?;

    if !has_permission(&auth_context.permissions, config.required_permission) {
        let permission_str = serde_json::to_string(&config.required_permission)
            .unwrap_or_else(|_| "\"unknown\"".to_string());
        let permission_str = permission_str.trim_matches('"');
        return Err((
            StatusCode::FORBIDDEN,
            format!("Missing required permission: {}", permission_str),
        )
            .into_response());
    }

    // APIキーの発行者の情報でClaimsを構築
    let exp = auth_context
        .expires_at
        .map(|dt| dt.timestamp() as usize)
        .unwrap_or_else(|| (Utc::now() + chrono::Duration::hours(24)).timestamp() as usize);
    let claims = Claims {
        sub: auth_context.created_by.to_string(),
        role: config.api_key_role,
        exp,
        must_change_password: false,
        password_changed_at: 0,
        // API キー actor はユーザー名を持たない。
        username: None,
    };
    let claims_for_response = claims.clone();
    let auth_context_for_response = auth_context.clone();
    request.extensions_mut().insert(claims);
    request.extensions_mut().insert(auth_context);
    record_audit_actor(
        &request,
        Some(&claims_for_response),
        Some(&auth_context_for_response),
    );

    let mut response = next.run(request).await;
    // 監査ログミドルウェア (SPEC-8301d106) がresponse extensionsからアクター情報を取得
    response.extensions_mut().insert(claims_for_response);
    response.extensions_mut().insert(auth_context_for_response);
    Ok(response)
}

// SPEC-e8e9326e: APIキー or ノードトークン認証ミドルウェアは廃止されました
// api_key_or_node_token_auth_middleware と node_token_auth_middleware は削除されました
// 新しい実装は POST /api/endpoints を使用してください

/// SHA-256ハッシュ化ヘルパー関数
///
/// # Arguments
/// * `input` - ハッシュ化する文字列
///
/// # Returns
/// * `String` - 16進数表現のSHA-256ハッシュ（64文字）
fn hash_with_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
