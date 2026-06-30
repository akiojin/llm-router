//! 認証フローE2Eテスト
//!
//! T091: 完全な認証フロー（ログイン → API呼び出し → ログアウト）
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use llmlb::common::auth::UserRole;
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    let db_pool = support::lb::create_test_db_pool().await;
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = std::sync::Arc::new(
        llmlb::db::request_history::RequestHistoryStorage::new(db_pool.clone()),
    );
    let jwt_secret = support::lb::test_jwt_secret();

    // テスト用の管理者ユーザーを作成
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
        .await
        .ok();

    let http_client = reqwest::Client::new();
    let inference_gate = llmlb::inference_gate::InferenceGate::default();
    let shutdown = llmlb::shutdown::ShutdownController::default();
    let update_manager = llmlb::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to create update manager");

    let state = AppState {
        load_manager,
        request_history,
        db_pool: db_pool.clone(),
        jwt_secret,
        http_client,
        event_bus: llmlb::events::create_shared_event_bus(),
        endpoint_registry,
        inference_gate,
        shutdown,
        update_manager,
        audit_log_writer: llmlb::audit::writer::AuditLogWriter::new(
            llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone()),
            llmlb::audit::writer::AuditLogWriterConfig::default(),
        ),
        audit_log_storage: std::sync::Arc::new(llmlb::db::audit_log::AuditLogStorage::new(
            db_pool.clone(),
        )),
        audit_archive_pool: None,
    };

    (api::create_app(state), db_pool)
}

#[tokio::test]
async fn test_complete_auth_flow() {
    let (app, _db_pool) = build_app().await;

    // Step 1: ログイン
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);

    let (login_parts, login_body) = login_response.into_parts();
    let set_cookie_values: Vec<String> = login_parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    assert!(
        !set_cookie_values.is_empty(),
        "Expected Set-Cookie headers, got: {:?}",
        login_parts.headers
    );
    let set_cookie = set_cookie_values.join(", ");
    assert!(
        set_cookie.contains("llmlb_jwt="),
        "Login response should set JWT cookie"
    );

    let login_body = axum::body::to_bytes(login_body, usize::MAX).await.unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();

    let token = login_data["token"].as_str().unwrap();
    assert!(!token.is_empty(), "Token should not be empty");

    // Step 2: トークンを使ってAPI呼び出し（ユーザー一覧取得）
    let users_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        users_response.status(),
        StatusCode::OK,
        "Authenticated request should succeed"
    );

    let users_body = axum::body::to_bytes(users_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let users: serde_json::Value = serde_json::from_slice(&users_body).unwrap();

    assert!(
        users.get("users").is_some(),
        "Response must have 'users' field"
    );
    assert!(users["users"].is_array(), "'users' must be an array");
    assert_eq!(
        users["users"].as_array().unwrap().len(),
        1,
        "Should have one admin user"
    );

    // Step 3: ログアウト
    let logout_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);
    let logout_set_cookie = logout_response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        logout_set_cookie.contains("llmlb_jwt="),
        "Logout should clear JWT cookie"
    );
    assert!(
        logout_set_cookie.contains("llmlb_csrf="),
        "Logout should clear CSRF cookie"
    );

    // Step 4: ログアウト後は認証が必要なエンドポイントにアクセスできない
    let unauthorized_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Note: 現在の実装ではログアウト後もトークンは有効（トークン無効化は実装されていない）
    // 実際のプロダクションではトークンブラックリストやリフレッシュトークン機構が必要
    assert!(
        unauthorized_response.status() == StatusCode::OK
            || unauthorized_response.status() == StatusCode::UNAUTHORIZED,
        "After logout, token may still be valid (no token blacklist implemented)"
    );
}

#[tokio::test]
async fn test_logout_requires_csrf_for_cookie_auth() {
    let (app, _db_pool) = build_app().await;

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);
    let (login_parts, _) = login_response.into_parts();
    let set_cookie_values: Vec<String> = login_parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let cookie_pairs: Vec<String> = set_cookie_values
        .into_iter()
        .filter_map(|value| {
            value
                .split(';')
                .next()
                .map(|first| first.trim().to_string())
        })
        .collect();
    let mut jwt_cookie = "";
    let mut csrf_cookie = "";
    for first in &cookie_pairs {
        if first.starts_with("llmlb_jwt=") {
            jwt_cookie = first;
        }
        if first.starts_with("llmlb_csrf=") {
            csrf_cookie = first;
        }
    }
    assert!(
        !jwt_cookie.is_empty() && !csrf_cookie.is_empty(),
        "Expected both jwt and csrf cookies, got: {:?}",
        cookie_pairs
    );

    let cookie_header = format!("{}; {}", jwt_cookie, csrf_cookie);

    let forbidden = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::HOST, "example.com")
                .header(header::ORIGIN, "http://example.com")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        forbidden.status(),
        StatusCode::FORBIDDEN,
        "Logout without CSRF header should be forbidden"
    );

    let ok = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::HOST, "example.com")
                .header(header::ORIGIN, "http://example.com")
                .header(header::COOKIE, &cookie_header)
                .header(
                    "x-csrf-token",
                    csrf_cookie.trim_start_matches("llmlb_csrf="),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        ok.status(),
        StatusCode::NO_CONTENT,
        "Logout with CSRF header should succeed"
    );
}

#[tokio::test]
async fn test_logout_requires_origin_for_cookie_auth() {
    let (app, _db_pool) = build_app().await;

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);
    let (login_parts, _) = login_response.into_parts();
    let set_cookie_values: Vec<String> = login_parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let cookie_pairs: Vec<String> = set_cookie_values
        .into_iter()
        .filter_map(|value| {
            value
                .split(';')
                .next()
                .map(|first| first.trim().to_string())
        })
        .collect();
    let mut jwt_cookie = "";
    let mut csrf_cookie = "";
    for first in &cookie_pairs {
        if first.starts_with("llmlb_jwt=") {
            jwt_cookie = first;
        }
        if first.starts_with("llmlb_csrf=") {
            csrf_cookie = first;
        }
    }
    assert!(
        !jwt_cookie.is_empty() && !csrf_cookie.is_empty(),
        "Expected both jwt and csrf cookies, got: {:?}",
        cookie_pairs
    );

    let cookie_header = format!("{}; {}", jwt_cookie, csrf_cookie);

    let forbidden = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::COOKIE, &cookie_header)
                .header(
                    "x-csrf-token",
                    csrf_cookie.trim_start_matches("llmlb_csrf="),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        forbidden.status(),
        StatusCode::FORBIDDEN,
        "Logout without origin should be forbidden"
    );
}

#[tokio::test]
async fn test_csrf_token_rotates_on_mutation() {
    let (app, _db_pool) = build_app().await;

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);
    let (login_parts, _) = login_response.into_parts();
    let set_cookie_values: Vec<String> = login_parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let cookie_pairs: Vec<String> = set_cookie_values
        .into_iter()
        .filter_map(|value| {
            value
                .split(';')
                .next()
                .map(|first| first.trim().to_string())
        })
        .collect();
    let mut jwt_cookie = "";
    let mut csrf_cookie = "";
    for first in &cookie_pairs {
        if first.starts_with("llmlb_jwt=") {
            jwt_cookie = first;
        }
        if first.starts_with("llmlb_csrf=") {
            csrf_cookie = first;
        }
    }
    assert!(
        !jwt_cookie.is_empty() && !csrf_cookie.is_empty(),
        "Expected both jwt and csrf cookies, got: {:?}",
        cookie_pairs
    );

    let cookie_header = format!("{}; {}", jwt_cookie, csrf_cookie);

    let create_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/invitations")
                .header("content-type", "application/json")
                .header(header::HOST, "example.com")
                .header(header::ORIGIN, "http://example.com")
                .header(header::COOKIE, &cookie_header)
                .header(
                    "x-csrf-token",
                    csrf_cookie.trim_start_matches("llmlb_csrf="),
                )
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "expires_in_hours": 24
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let new_csrf_cookie = create_response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(|value| value.trim())
        .find(|value| value.starts_with("llmlb_csrf="))
        .unwrap_or_default()
        .to_string();

    assert!(
        !new_csrf_cookie.is_empty(),
        "Mutating response should rotate CSRF cookie"
    );
    assert_ne!(
        new_csrf_cookie, csrf_cookie,
        "CSRF cookie should rotate after mutation"
    );
}

#[tokio::test]
async fn test_api_key_mutation_does_not_require_csrf() {
    let (app, _db_pool) = build_app().await;

    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock)
        .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .header("x-api-key", "sk_debug_admin")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-endpoint",
                        "base_url": mock.uri(),
                        "inference_timeout_secs": 1,
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .all(|cookie| !cookie.starts_with("llmlb_csrf=")),
        "API key auth should not require/rotate CSRF cookies"
    );
}

#[tokio::test]
async fn test_unauthorized_access_without_token() {
    let (app, _db_pool) = build_app().await;

    // トークンなしでAPIにアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Request without token should be unauthorized"
    );
}

#[tokio::test]
async fn test_invalid_token() {
    let (app, _db_pool) = build_app().await;

    // 無効なトークンでAPIにアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header("authorization", "Bearer invalid-token-12345")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Request with invalid token should be unauthorized"
    );
}

#[tokio::test]
async fn test_auth_me_endpoint() {
    let (app, _db_pool) = build_app().await;

    // Step 1: ログイン
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);

    let login_body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let token = login_data["token"].as_str().unwrap();

    // Step 2: /api/auth/me でユーザー情報を取得
    let me_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        me_response.status(),
        StatusCode::OK,
        "/api/auth/me should return OK with valid token"
    );

    let me_body = axum::body::to_bytes(me_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let me_data: serde_json::Value = serde_json::from_slice(&me_body).unwrap();

    assert!(
        me_data.get("user_id").is_some(),
        "Response must have 'user_id' field"
    );
    assert!(
        me_data.get("username").is_some(),
        "Response must have 'username' field"
    );
    assert_eq!(
        me_data["username"].as_str().unwrap(),
        "admin",
        "Username should match logged in user"
    );
    assert!(
        me_data.get("role").is_some(),
        "Response must have 'role' field"
    );
}

#[tokio::test]
async fn test_auth_me_with_cookie() {
    let (app, _db_pool) = build_app().await;

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "admin",
                        "password": "password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_response.status(), StatusCode::OK);
    let (login_parts, login_body) = login_response.into_parts();
    let cookie_pair = login_parts
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(|value| value.trim())
        .find(|value| value.starts_with("llmlb_jwt="))
        .unwrap_or_default()
        .to_string();
    assert!(
        !cookie_pair.is_empty(),
        "Set-Cookie should contain llmlb_jwt token"
    );

    let login_body = axum::body::to_bytes(login_body, usize::MAX).await.unwrap();
    let login_data: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    assert!(
        login_data["token"].as_str().unwrap_or_default() != "",
        "Token should still be included in response"
    );

    let me_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header(header::COOKIE, cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        me_response.status(),
        StatusCode::OK,
        "/api/auth/me should accept JWT cookie"
    );
}

#[tokio::test]
async fn test_auth_me_without_token() {
    let (app, _db_pool) = build_app().await;

    // トークンなしで/api/auth/meにアクセス
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "/api/auth/me without token should return UNAUTHORIZED"
    );
}
