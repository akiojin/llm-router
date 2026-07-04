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
use jsonwebtoken::decode_header;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use uuid::Uuid;

#[cfg(debug_assertions)]
const DEBUG_API_KEY_ALL: &str = "sk_debug";
#[cfg(debug_assertions)]
const DEBUG_API_KEY_RUNTIME: &str = "sk_debug_runtime";
#[cfg(debug_assertions)]
const DEBUG_API_KEY_API: &str = "sk_debug_api";
#[cfg(debug_assertions)]
const DEBUG_API_KEY_ADMIN: &str = "sk_debug_admin";

#[cfg(debug_assertions)]
fn debug_api_key_permissions(
    request_key: &str,
) -> Option<Vec<crate::common::auth::ApiKeyPermission>> {
    match request_key {
        DEBUG_API_KEY_ALL => Some(crate::common::auth::ApiKeyPermission::all()),
        DEBUG_API_KEY_RUNTIME => Some(vec![crate::common::auth::ApiKeyPermission::RegistryRead]),
        DEBUG_API_KEY_API => Some(vec![
            crate::common::auth::ApiKeyPermission::OpenaiInference,
            crate::common::auth::ApiKeyPermission::OpenaiModelsRead,
        ]),
        DEBUG_API_KEY_ADMIN => Some(crate::common::auth::ApiKeyPermission::all()),
        _ => None,
    }
}

#[cfg(not(debug_assertions))]
fn debug_api_key_permissions(
    _request_key: &str,
) -> Option<Vec<crate::common::auth::ApiKeyPermission>> {
    None
}

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

fn token_looks_like_jwt(token: &str) -> bool {
    let mut parts = token.split('.');
    let (first, second, third, extra) = (parts.next(), parts.next(), parts.next(), parts.next());
    if extra.is_some() {
        return false;
    }
    if matches!((first, second, third), (Some(a), Some(b), Some(c)) if !a.is_empty() && !b.is_empty() && !c.is_empty())
    {
        return decode_header(token).is_ok();
    }
    false
}

fn extract_jwt_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if token_looks_like_jwt(token) {
                return Some(token.to_string());
            }
        }
    }
    extract_jwt_cookie(headers)
}

#[allow(clippy::result_large_err)]
fn verify_jwt_claims(token: &str, jwt_secret: &str) -> Result<Claims, Response> {
    crate::auth::jwt::verify_jwt(token, jwt_secret).map_err(|e| {
        tracing::warn!("JWT verification failed: {}", e);
        (StatusCode::UNAUTHORIZED, format!("Invalid token: {}", e)).into_response()
    })
}

pub(crate) fn extract_jwt_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) =
            trimmed.strip_prefix(&format!("{}=", crate::auth::DASHBOARD_JWT_COOKIE))
        {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub(crate) fn extract_csrf_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) =
            trimmed.strip_prefix(&format!("{}=", crate::auth::DASHBOARD_CSRF_COOKIE))
        {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn method_requires_csrf(method: &axum::http::Method) -> bool {
    matches!(
        *method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    )
}

fn expected_origin(headers: &HeaderMap) -> Option<String> {
    let host_raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())?;
    let host = host_raw
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let proto_raw = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let proto = proto_raw
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    Some(format!("{}://{}", proto, host))
}

fn origin_or_referer(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        return Some(origin.to_string());
    }
    let referer = headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())?;
    if let Some((scheme, rest)) = referer.split_once("://") {
        let host = rest.split('/').next().unwrap_or_default();
        if !host.is_empty() {
            return Some(format!("{}://{}", scheme, host));
        }
    }
    None
}

fn origin_matches(headers: &HeaderMap) -> bool {
    let expected = match expected_origin(headers) {
        Some(value) => value,
        None => return false,
    };
    let provided = match origin_or_referer(headers) {
        Some(value) => value,
        None => return false,
    };
    match (
        normalize_origin_for_compare(&provided),
        normalize_origin_for_compare(&expected),
    ) {
        (Some(provided), Some(expected)) => provided == expected,
        _ => false,
    }
}

fn normalize_origin_for_compare(origin: &str) -> Option<(String, String, u16)> {
    let (scheme, rest) = origin.split_once("://")?;
    let authority = rest.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let authority = axum::http::uri::Authority::from_str(authority).ok()?;
    let scheme = scheme.trim().to_ascii_lowercase();
    let host = authority.host().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let port = authority
        .port_u16()
        .or_else(|| default_port_for_scheme(&scheme))?;
    Some((scheme, host, port))
}

fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

fn response_sets_csrf_cookie(response: &Response) -> bool {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.starts_with(crate::auth::DASHBOARD_CSRF_COOKIE))
}

async fn authenticate_api_key(
    pool: &sqlx::SqlitePool,
    api_key: &str,
) -> Result<ApiKeyAuthContext, Response> {
    if let Some(permissions) = debug_api_key_permissions(api_key) {
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
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware as axum_middleware, routing::get, Router};
    use tower::ServiceExt;

    #[test]
    fn test_hash_with_sha256() {
        let input = "test_api_key_12345";
        let hash = hash_with_sha256(input);

        // SHA-256ハッシュは64文字の16進数
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // 同じ入力は同じハッシュを生成
        let hash2 = hash_with_sha256(input);
        assert_eq!(hash, hash2);

        // 異なる入力は異なるハッシュを生成
        let hash3 = hash_with_sha256("different_input");
        assert_ne!(hash, hash3);
    }

    #[test]
    fn origin_matches_accepts_default_https_port_variants() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "example.com:443".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());

        assert!(origin_matches(&headers));
    }

    #[test]
    fn origin_matches_rejects_different_non_default_port() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "example.com:8443".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());

        assert!(!origin_matches(&headers));
    }

    #[test]
    fn origin_matches_handles_forwarded_lists() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-host",
            "example.com:443, proxy.internal".parse().unwrap(),
        );
        headers.insert("x-forwarded-proto", "https, http".parse().unwrap());
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());

        assert!(origin_matches(&headers));
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn admin_middleware_allows_bearer_api_key() {
        let state = crate::db::test_utils::TestAppStateBuilder::new()
            .await
            .build()
            .await;

        let cfg = JwtOrApiKeyPermissionConfig {
            app_state: state,
            required_permission: ApiKeyPermission::UsersManage,
            jwt_required_role: Some(UserRole::Admin),
            api_key_role: UserRole::Admin,
        };
        let app = Router::new().route("/admin", get(|| async { "ok" })).layer(
            axum_middleware::from_fn_with_state(cfg, jwt_or_api_key_permission_middleware),
        );

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header("authorization", format!("Bearer {}", DEBUG_API_KEY_ADMIN))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn admin_middleware_rejects_invalid_jwt_even_with_api_key() {
        let state = crate::db::test_utils::TestAppStateBuilder::new()
            .await
            .build()
            .await;

        let cfg = JwtOrApiKeyPermissionConfig {
            app_state: state,
            required_permission: ApiKeyPermission::UsersManage,
            jwt_required_role: Some(UserRole::Admin),
            api_key_role: UserRole::Admin,
        };
        let app = Router::new().route("/admin", get(|| async { "ok" })).layer(
            axum_middleware::from_fn_with_state(cfg, jwt_or_api_key_permission_middleware),
        );

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header(
                        "authorization",
                        "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhZG1pbiIsInJvbGUiOiJhZG1pbiIsImV4cCI6MjAwMDAwMDAwMH0.invalidsig",
                    )
                    .header("x-api-key", DEBUG_API_KEY_ADMIN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn debug_api_key_is_accepted_in_debug_build_without_db() {
        let pool = crate::db::test_utils::test_db_pool().await;

        let app = axum::Router::new()
            .route(
                "/t",
                axum::routing::get(
                    |axum::extract::Extension(auth): axum::extract::Extension<
                        ApiKeyAuthContext,
                    >| async move {
                        format!("{}:{}", auth.id, auth.permissions.len())
                    },
                ),
            )
            .layer(axum::middleware::from_fn_with_state(
                pool,
                api_key_auth_middleware,
            ));

        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/t")
                    .header("x-api-key", DEBUG_API_KEY_ALL)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(body_str.starts_with(&Uuid::nil().to_string()));
        assert!(body_str.contains(&ApiKeyPermission::all().len().to_string()));
    }

    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn debug_api_key_is_rejected_in_release_build() {
        let pool = crate::db::test_utils::test_db_pool().await;

        let app = axum::Router::new()
            .route("/t", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                pool,
                api_key_auth_middleware,
            ));

        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/t")
                    .header("x-api-key", "sk_debug")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // =========================================================================
    // token_looks_like_jwt tests
    // =========================================================================

    #[test]
    fn token_looks_like_jwt_rejects_empty_string() {
        assert!(!token_looks_like_jwt(""));
    }

    #[test]
    fn token_looks_like_jwt_rejects_plain_text() {
        assert!(!token_looks_like_jwt("sk_debug"));
    }

    #[test]
    fn token_looks_like_jwt_rejects_two_parts() {
        assert!(!token_looks_like_jwt("header.payload"));
    }

    #[test]
    fn token_looks_like_jwt_rejects_four_parts() {
        assert!(!token_looks_like_jwt("a.b.c.d"));
    }

    #[test]
    fn token_looks_like_jwt_rejects_three_empty_parts() {
        assert!(!token_looks_like_jwt(".."));
    }

    #[test]
    fn token_looks_like_jwt_rejects_one_empty_segment() {
        assert!(!token_looks_like_jwt("a..c"));
    }

    #[test]
    fn token_looks_like_jwt_accepts_valid_jwt() {
        let token = crate::auth::jwt::create_jwt(
            "user1",
            crate::common::auth::UserRole::Admin,
            "secret",
            false,
            0,
            None,
        )
        .unwrap();
        assert!(token_looks_like_jwt(&token));
    }

    #[test]
    fn token_looks_like_jwt_rejects_non_base64_three_parts() {
        // Three non-empty parts but not valid JWT header
        assert!(!token_looks_like_jwt("not.a.jwt"));
    }

    // =========================================================================
    // extract_jwt_from_headers tests
    // =========================================================================

    #[test]
    fn extract_jwt_from_headers_returns_none_for_empty_headers() {
        let headers = HeaderMap::new();
        assert!(extract_jwt_from_headers(&headers).is_none());
    }

    #[test]
    fn extract_jwt_from_headers_returns_jwt_from_bearer() {
        let token = crate::auth::jwt::create_jwt(
            "user1",
            crate::common::auth::UserRole::Admin,
            "secret",
            false,
            0,
            None,
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
        let result = extract_jwt_from_headers(&headers);
        assert_eq!(result, Some(token));
    }

    #[test]
    fn extract_jwt_from_headers_ignores_non_jwt_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer sk_debug_plain_token".parse().unwrap(),
        );
        // sk_debug_plain_token is not a valid JWT, so should fall through to cookie check
        assert!(extract_jwt_from_headers(&headers).is_none());
    }

    #[test]
    fn extract_jwt_from_headers_falls_back_to_cookie() {
        let token = crate::auth::jwt::create_jwt(
            "u",
            crate::common::auth::UserRole::Viewer,
            "s",
            false,
            0,
            None,
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}={}", crate::auth::DASHBOARD_JWT_COOKIE, token)
                .parse()
                .unwrap(),
        );
        let result = extract_jwt_from_headers(&headers);
        assert_eq!(result, Some(token));
    }

    // =========================================================================
    // extract_jwt_cookie tests
    // =========================================================================

    #[test]
    fn extract_jwt_cookie_returns_none_no_cookie_header() {
        let headers = HeaderMap::new();
        assert!(extract_jwt_cookie(&headers).is_none());
    }

    #[test]
    fn extract_jwt_cookie_returns_none_empty_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}=", crate::auth::DASHBOARD_JWT_COOKIE)
                .parse()
                .unwrap(),
        );
        assert!(extract_jwt_cookie(&headers).is_none());
    }

    #[test]
    fn extract_jwt_cookie_returns_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}=mytoken123", crate::auth::DASHBOARD_JWT_COOKIE)
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_jwt_cookie(&headers), Some("mytoken123".to_string()));
    }

    #[test]
    fn extract_jwt_cookie_from_multiple_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!(
                "other=abc; {}=thetoken; another=xyz",
                crate::auth::DASHBOARD_JWT_COOKIE
            )
            .parse()
            .unwrap(),
        );
        assert_eq!(extract_jwt_cookie(&headers), Some("thetoken".to_string()));
    }

    // =========================================================================
    // extract_csrf_cookie tests
    // =========================================================================

    #[test]
    fn extract_csrf_cookie_returns_none_no_cookie_header() {
        let headers = HeaderMap::new();
        assert!(extract_csrf_cookie(&headers).is_none());
    }

    #[test]
    fn extract_csrf_cookie_returns_none_empty_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}=", crate::auth::DASHBOARD_CSRF_COOKIE)
                .parse()
                .unwrap(),
        );
        assert!(extract_csrf_cookie(&headers).is_none());
    }

    #[test]
    fn extract_csrf_cookie_returns_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}=csrftoken", crate::auth::DASHBOARD_CSRF_COOKIE)
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_csrf_cookie(&headers), Some("csrftoken".to_string()));
    }

    #[test]
    fn extract_csrf_cookie_ignores_other_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "foo=bar; baz=qux".parse().unwrap());
        assert!(extract_csrf_cookie(&headers).is_none());
    }

    // =========================================================================
    // method_requires_csrf tests
    // =========================================================================

    #[test]
    fn method_requires_csrf_for_post() {
        assert!(method_requires_csrf(&axum::http::Method::POST));
    }

    #[test]
    fn method_requires_csrf_for_put() {
        assert!(method_requires_csrf(&axum::http::Method::PUT));
    }

    #[test]
    fn method_requires_csrf_for_patch() {
        assert!(method_requires_csrf(&axum::http::Method::PATCH));
    }

    #[test]
    fn method_requires_csrf_for_delete() {
        assert!(method_requires_csrf(&axum::http::Method::DELETE));
    }

    #[test]
    fn method_requires_csrf_not_for_get() {
        assert!(!method_requires_csrf(&axum::http::Method::GET));
    }

    #[test]
    fn method_requires_csrf_not_for_head() {
        assert!(!method_requires_csrf(&axum::http::Method::HEAD));
    }

    #[test]
    fn method_requires_csrf_not_for_options() {
        assert!(!method_requires_csrf(&axum::http::Method::OPTIONS));
    }

    // =========================================================================
    // expected_origin tests
    // =========================================================================

    #[test]
    fn expected_origin_from_host_header() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.com".parse().unwrap());
        assert_eq!(
            expected_origin(&headers),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn expected_origin_prefers_x_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "internal.local".parse().unwrap());
        headers.insert("x-forwarded-host", "example.com".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(
            expected_origin(&headers),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn expected_origin_returns_none_without_host() {
        let headers = HeaderMap::new();
        assert!(expected_origin(&headers).is_none());
    }

    #[test]
    fn expected_origin_defaults_proto_to_http() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.com".parse().unwrap());
        let result = expected_origin(&headers).unwrap();
        assert!(result.starts_with("http://"));
    }

    // =========================================================================
    // origin_or_referer tests
    // =========================================================================

    #[test]
    fn origin_or_referer_returns_origin_header() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());
        assert_eq!(
            origin_or_referer(&headers),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn origin_or_referer_falls_back_to_referer() {
        let mut headers = HeaderMap::new();
        headers.insert(header::REFERER, "https://example.com/page".parse().unwrap());
        assert_eq!(
            origin_or_referer(&headers),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn origin_or_referer_returns_none_without_headers() {
        let headers = HeaderMap::new();
        assert!(origin_or_referer(&headers).is_none());
    }

    #[test]
    fn origin_or_referer_extracts_origin_from_referer_path() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::REFERER,
            "http://localhost:3000/dashboard/settings".parse().unwrap(),
        );
        assert_eq!(
            origin_or_referer(&headers),
            Some("http://localhost:3000".to_string())
        );
    }

    // =========================================================================
    // normalize_origin_for_compare tests
    // =========================================================================

    #[test]
    fn normalize_origin_strips_default_http_port() {
        let result = normalize_origin_for_compare("http://example.com:80");
        assert!(result.is_some());
        let (scheme, host, port) = result.unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
    }

    #[test]
    fn normalize_origin_strips_default_https_port() {
        let result = normalize_origin_for_compare("https://example.com:443");
        assert!(result.is_some());
        let (scheme, host, port) = result.unwrap();
        assert_eq!(scheme, "https");
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn normalize_origin_preserves_non_default_port() {
        let result = normalize_origin_for_compare("http://example.com:8080");
        let (_, _, port) = result.unwrap();
        assert_eq!(port, 8080);
    }

    #[test]
    fn normalize_origin_adds_default_port_for_http() {
        let result = normalize_origin_for_compare("http://example.com");
        let (_, _, port) = result.unwrap();
        assert_eq!(port, 80);
    }

    #[test]
    fn normalize_origin_adds_default_port_for_https() {
        let result = normalize_origin_for_compare("https://example.com");
        let (_, _, port) = result.unwrap();
        assert_eq!(port, 443);
    }

    #[test]
    fn normalize_origin_returns_none_for_missing_scheme() {
        assert!(normalize_origin_for_compare("example.com").is_none());
    }

    #[test]
    fn normalize_origin_returns_none_for_empty_host() {
        assert!(normalize_origin_for_compare("http://").is_none());
    }

    #[test]
    fn normalize_origin_lowercases_host() {
        let result = normalize_origin_for_compare("https://EXAMPLE.COM");
        let (_, host, _) = result.unwrap();
        assert_eq!(host, "example.com");
    }

    #[test]
    fn normalize_origin_strips_trailing_dot_from_host() {
        let result = normalize_origin_for_compare("https://example.com.");
        let (_, host, _) = result.unwrap();
        assert_eq!(host, "example.com");
    }

    // =========================================================================
    // default_port_for_scheme tests
    // =========================================================================

    #[test]
    fn default_port_http() {
        assert_eq!(default_port_for_scheme("http"), Some(80));
    }

    #[test]
    fn default_port_https() {
        assert_eq!(default_port_for_scheme("https"), Some(443));
    }

    #[test]
    fn default_port_unknown_scheme() {
        assert_eq!(default_port_for_scheme("ftp"), None);
        assert_eq!(default_port_for_scheme("ws"), None);
    }

    // =========================================================================
    // origin_matches tests (additional)
    // =========================================================================

    #[test]
    fn origin_matches_returns_false_without_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());
        assert!(!origin_matches(&headers));
    }

    #[test]
    fn origin_matches_returns_false_without_origin_or_referer() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "example.com".parse().unwrap());
        assert!(!origin_matches(&headers));
    }

    #[test]
    fn origin_matches_case_insensitive_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "EXAMPLE.COM".parse().unwrap());
        headers.insert(header::ORIGIN, "http://example.com".parse().unwrap());
        assert!(origin_matches(&headers));
    }

    // =========================================================================
    // has_permission tests
    // =========================================================================

    #[test]
    fn has_permission_returns_true_when_present() {
        let perms = vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::EndpointsRead,
        ];
        assert!(has_permission(&perms, ApiKeyPermission::OpenaiInference));
    }

    #[test]
    fn has_permission_returns_false_when_absent() {
        let perms = vec![ApiKeyPermission::OpenaiInference];
        assert!(!has_permission(&perms, ApiKeyPermission::UsersManage));
    }

    #[test]
    fn has_permission_empty_permissions() {
        let perms: Vec<ApiKeyPermission> = vec![];
        assert!(!has_permission(&perms, ApiKeyPermission::OpenaiInference));
    }

    // =========================================================================
    // hash_with_sha256 tests (additional)
    // =========================================================================

    #[test]
    fn hash_with_sha256_empty_input() {
        let hash = hash_with_sha256("");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_with_sha256_known_value() {
        // SHA-256 of "hello" is well-known
        let hash = hash_with_sha256("hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // =========================================================================
    // extract_api_key tests
    // =========================================================================

    #[test]
    fn extract_api_key_from_x_api_key_header() {
        let request = Request::builder()
            .header("X-API-Key", "sk_test123")
            .body(Body::empty())
            .unwrap();
        let result = extract_api_key(&request);
        assert_eq!(result.unwrap(), "sk_test123");
    }

    #[test]
    fn extract_api_key_from_bearer_header() {
        let request = Request::builder()
            .header(header::AUTHORIZATION, "Bearer sk_test456")
            .body(Body::empty())
            .unwrap();
        let result = extract_api_key(&request);
        assert_eq!(result.unwrap(), "sk_test456");
    }

    #[test]
    fn extract_api_key_missing_both_headers() {
        let request = Request::builder().body(Body::empty()).unwrap();
        let result = extract_api_key(&request);
        assert!(result.is_err());
    }

    #[test]
    fn extract_api_key_invalid_auth_format() {
        let request = Request::builder()
            .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        let result = extract_api_key(&request);
        assert!(result.is_err());
    }

    #[test]
    fn extract_api_key_prefers_x_api_key_over_bearer() {
        let request = Request::builder()
            .header("X-API-Key", "from_x_api_key")
            .header(header::AUTHORIZATION, "Bearer from_bearer")
            .body(Body::empty())
            .unwrap();
        let result = extract_api_key(&request);
        assert_eq!(result.unwrap(), "from_x_api_key");
    }

    // =========================================================================
    // response_sets_csrf_cookie tests
    // =========================================================================

    #[test]
    fn response_sets_csrf_cookie_true_when_present() {
        let mut response = axum::response::Response::new(Body::empty());
        let cookie_value = format!("{}=token123; Path=/", crate::auth::DASHBOARD_CSRF_COOKIE);
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie_value.parse().unwrap());
        assert!(response_sets_csrf_cookie(&response));
    }

    #[test]
    fn response_sets_csrf_cookie_false_when_absent() {
        let response = axum::response::Response::new(Body::empty());
        assert!(!response_sets_csrf_cookie(&response));
    }

    #[test]
    fn response_sets_csrf_cookie_false_for_other_cookie() {
        let mut response = axum::response::Response::new(Body::empty());
        response
            .headers_mut()
            .append(header::SET_COOKIE, "other_cookie=val".parse().unwrap());
        assert!(!response_sets_csrf_cookie(&response));
    }

    // =========================================================================
    // ApiKeyAuthContext tests
    // =========================================================================

    #[test]
    fn api_key_auth_context_clone() {
        let ctx = ApiKeyAuthContext {
            id: Uuid::new_v4(),
            created_by: Uuid::new_v4(),
            permissions: vec![ApiKeyPermission::OpenaiInference],
            expires_at: None,
        };
        let cloned = ctx.clone();
        assert_eq!(ctx.id, cloned.id);
        assert_eq!(ctx.created_by, cloned.created_by);
        assert_eq!(ctx.permissions, cloned.permissions);
    }

    #[test]
    fn api_key_auth_context_debug() {
        let ctx = ApiKeyAuthContext {
            id: Uuid::nil(),
            created_by: Uuid::nil(),
            permissions: vec![],
            expires_at: None,
        };
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("ApiKeyAuthContext"));
    }

    // =========================================================================
    // debug_api_key_permissions tests (debug build only)
    // =========================================================================

    #[cfg(debug_assertions)]
    #[test]
    fn debug_api_key_all_returns_all_permissions() {
        let perms = debug_api_key_permissions(DEBUG_API_KEY_ALL);
        assert!(perms.is_some());
        assert_eq!(perms.unwrap().len(), ApiKeyPermission::all().len());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_api_key_runtime_returns_registry_read() {
        let perms = debug_api_key_permissions(DEBUG_API_KEY_RUNTIME);
        assert!(perms.is_some());
        let perms = perms.unwrap();
        assert_eq!(perms.len(), 1);
        assert_eq!(perms[0], ApiKeyPermission::RegistryRead);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_api_key_api_returns_openai_permissions() {
        let perms = debug_api_key_permissions(DEBUG_API_KEY_API);
        assert!(perms.is_some());
        let perms = perms.unwrap();
        assert_eq!(perms.len(), 2);
        assert!(perms.contains(&ApiKeyPermission::OpenaiInference));
        assert!(perms.contains(&ApiKeyPermission::OpenaiModelsRead));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_api_key_admin_returns_all_permissions() {
        let perms = debug_api_key_permissions(DEBUG_API_KEY_ADMIN);
        assert!(perms.is_some());
        assert_eq!(perms.unwrap().len(), ApiKeyPermission::all().len());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_api_key_unknown_returns_none() {
        let perms = debug_api_key_permissions("sk_unknown");
        assert!(perms.is_none());
    }
}
