//! Integration Test: APIキーのpermissions制御
//!
//! /api と /v1 の各エンドポイントで permissions が正しく強制されることを確認する。
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::{ApiKeyPermission, UserRole};
use serde_json::json;
use serial_test::serial;
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use crate::support;

async fn build_app() -> (Router, sqlx::SqlitePool) {
    // US-013: 認証は常に必須。テスト用 app は共通ヘルパーで構築する。
    let db_pool = support::lb::create_test_db_pool().await;
    let app = support::lb::build_test_router(db_pool.clone()).await;
    (app, db_pool)
}

async fn create_admin_user(db_pool: &sqlx::SqlitePool) -> uuid::Uuid {
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let created =
        llmlb::db::users::create(db_pool, "admin", &password_hash, UserRole::Admin, false).await;
    if let Ok(user) = created {
        return user.id;
    }
    llmlb::db::users::find_by_username(db_pool, "admin")
        .await
        .unwrap()
        .unwrap()
        .id
}

async fn create_api_key(db_pool: &sqlx::SqlitePool, permissions: Vec<ApiKeyPermission>) -> String {
    let admin_id = create_admin_user(db_pool).await;
    let key_name = format!("test-key-{}", uuid::Uuid::new_v4());
    let api_key = llmlb::db::api_keys::create(db_pool, &key_name, admin_id, None, permissions)
        .await
        .expect("create api key");
    api_key.key
}

#[tokio::test]
#[serial]
async fn v1_models_requires_auth_by_default() {
    // US-013: 認証は常に必須。未認証の /v1/models は 401。
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn invalid_client_key_is_rejected() {
    // US-013: 無効な API キーは 401（旧 optional モードのような無視はしない）。
    let (app, _db_pool) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("authorization", "Bearer invalid-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn jwt_identity_preserved_for_me_api_keys() {
    let (app, db_pool) = build_app().await;
    let admin_id = create_admin_user(&db_pool).await;
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin_id.to_string(),
        UserRole::Admin,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create jwt");
    let key_name = format!("me-key-{}", uuid::Uuid::new_v4());
    let permissions = vec!["openai.inference"];

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", jwt))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": key_name,
                        "permissions": permissions
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let api_keys = llmlb::db::api_keys::list_by_creator(&db_pool, admin_id)
        .await
        .expect("list api keys by creator");
    assert_eq!(api_keys.len(), 1);
    assert_eq!(api_keys[0].name, key_name);
    assert_eq!(api_keys[0].created_by, admin_id);
}

#[tokio::test]
#[serial]
async fn invalid_jwt_rejected_for_me_api_keys() {
    let (app, _db_pool) = build_app().await;
    let jwt = llmlb::auth::jwt::create_jwt(
        &uuid::Uuid::new_v4().to_string(),
        UserRole::Admin,
        "wrong-secret",
        false,
        0,
        None,
    )
    .expect("create jwt with wrong secret");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn endpoints_create_requires_endpoints_manage_permission() {
    let (app, db_pool) = build_app().await;

    let registry_key = create_api_key(&db_pool, vec![ApiKeyPermission::RegistryRead]).await;
    let openai_key = create_api_key(
        &db_pool,
        vec![
            ApiKeyPermission::OpenaiInference,
            ApiKeyPermission::OpenaiModelsRead,
        ],
    )
    .await;
    let endpoints_manage_key =
        create_api_key(&db_pool, vec![ApiKeyPermission::EndpointsManage]).await;

    let mock_server = MockServer::start().await;
    // 検出経路のタイムアウトを避けるため最小限のレスポンスを用意
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;
    // SPEC-93536000: 空のモデルリストは登録拒否されるため、少なくとも1つのモデルを返す
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{"id": "test-model", "object": "model"}]
        })))
        .mount(&mock_server)
        .await;

    let payload = json!({
        "name": "scope-endpoint",
        "base_url": format!("http://{}", mock_server.address()),
        "health_check_interval_secs": 30
    });

    // No API key -> 401
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Wrong scope -> 403
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", openai_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // registry.read -> 403 (endpoints.manage required)
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", registry_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // endpoints.manage -> 201
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/endpoints")
                .header("authorization", format!("Bearer {}", endpoints_manage_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn v1_inference_requires_openai_inference_permission() {
    let (app, db_pool) = build_app().await;

    let registry_key = create_api_key(&db_pool, vec![ApiKeyPermission::RegistryRead]).await;
    let openai_key = create_api_key(&db_pool, vec![ApiKeyPermission::OpenaiInference]).await;

    let payload = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    // Wrong scope -> 403
    let response = app
        .clone()
        .oneshot(support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", registry_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Correct scope -> authenticated (503 if no nodes)
    let response = app
        .oneshot(support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", openai_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert!(
        matches!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::OK | StatusCode::NOT_FOUND
        ),
        "expected authenticated response, got {}",
        response.status()
    );
}

#[tokio::test]
#[serial]
async fn dashboard_overview_requires_jwt() {
    let (app, db_pool) = build_app().await;
    let admin_id = create_admin_user(&db_pool).await;
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin_id.to_string(),
        UserRole::Admin,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create jwt");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/overview")
                .header("authorization", format!("Bearer {}", jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn api_models_requires_registry_read_permission() {
    let (app, db_pool) = build_app().await;

    let registry_key = create_api_key(&db_pool, vec![ApiKeyPermission::RegistryRead]).await;
    let openai_key = create_api_key(&db_pool, vec![ApiKeyPermission::OpenaiInference]).await;

    // No API key -> 401
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Wrong scope -> 403
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models")
                .header("authorization", format!("Bearer {}", openai_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Correct permission -> 200
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/models")
                .header("authorization", format!("Bearer {}", registry_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn v1_models_requires_openai_models_read_permission() {
    let (app, db_pool) = build_app().await;

    let inference_key = create_api_key(&db_pool, vec![ApiKeyPermission::OpenaiInference]).await;
    let models_key = create_api_key(&db_pool, vec![ApiKeyPermission::OpenaiModelsRead]).await;

    // Wrong scope -> 403
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("authorization", format!("Bearer {}", inference_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Correct scope -> 200
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .header("authorization", format!("Bearer {}", models_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn me_api_keys_routes_require_jwt_and_owner_scope() {
    let (app, db_pool) = build_app().await;

    let admin_id = create_admin_user(&db_pool).await;
    let viewer_password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let viewer = llmlb::db::users::create(
        &db_pool,
        "viewer-user",
        &viewer_password_hash,
        UserRole::Viewer,
        false,
    )
    .await
    .unwrap();

    let admin_jwt = llmlb::auth::jwt::create_jwt(
        &admin_id.to_string(),
        UserRole::Admin,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create admin jwt");
    let viewer_jwt = llmlb::auth::jwt::create_jwt(
        &viewer.id.to_string(),
        UserRole::Viewer,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create viewer jwt");

    // No JWT -> 401
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // viewer can create own key
    let create_payload = json!({
        "name": "viewer-key",
        "expires_at": null
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", viewer_jwt))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let created_key = created["key"].as_str().unwrap();
    let created_id = created["id"].as_str().unwrap();
    assert!(created_key.starts_with("sk_"));

    // viewer list includes the key
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", viewer_jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list["api_keys"].as_array().unwrap().len(), 1);

    // admin cannot delete viewer's key from self-scope endpoint
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/me/api-keys/{}", created_id))
                .header("authorization", format!("Bearer {}", admin_jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // owner can delete
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/me/api-keys/{}", created_id))
                .header("authorization", format!("Bearer {}", viewer_jwt))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Deleted key cannot be used
    let payload = json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = app
        .oneshot(support::lb::with_connect_info(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", created_key))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn me_api_keys_create_enforces_role_based_permissions_payload() {
    let (app, db_pool) = build_app().await;

    let admin_id = create_admin_user(&db_pool).await;
    let viewer_password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let viewer = llmlb::db::users::create(
        &db_pool,
        "viewer-permissions-user",
        &viewer_password_hash,
        UserRole::Viewer,
        false,
    )
    .await
    .unwrap();

    let admin_jwt = llmlb::auth::jwt::create_jwt(
        &admin_id.to_string(),
        UserRole::Admin,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create admin jwt");
    let viewer_jwt = llmlb::auth::jwt::create_jwt(
        &viewer.id.to_string(),
        UserRole::Viewer,
        &support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create viewer jwt");

    // admin: permissions is required
    let admin_missing_permissions = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", admin_jwt))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "admin-key-missing-permissions",
                        "expires_at": null
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_missing_permissions.status(), StatusCode::BAD_REQUEST);

    // admin: explicit non-empty permissions is accepted
    let admin_permissions = vec!["openai.inference", "openai.models.read", "endpoints.read"];
    let admin_create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", admin_jwt))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "admin-key-with-permissions",
                        "expires_at": null,
                        "permissions": admin_permissions
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_create.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(admin_create.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        created["permissions"],
        json!(["openai.inference", "openai.models.read", "endpoints.read"])
    );

    // viewer: permissions must not be provided
    let viewer_with_permissions = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", viewer_jwt))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "viewer-key-with-permissions",
                        "expires_at": null,
                        "permissions": ["openai.inference"]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(viewer_with_permissions.status(), StatusCode::BAD_REQUEST);

    // viewer: no permissions payload creates fixed OpenAI key
    let viewer_create = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/api-keys")
                .header("authorization", format!("Bearer {}", viewer_jwt))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "viewer-key-fixed-permissions",
                        "expires_at": null
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(viewer_create.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(viewer_create.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        created["permissions"],
        json!(["openai.inference", "openai.models.read"])
    );
}
