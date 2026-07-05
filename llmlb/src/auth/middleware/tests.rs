use super::debug_keys::*;
use super::jwt_token::token_looks_like_jwt;
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

    let app =
        axum::Router::new()
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
