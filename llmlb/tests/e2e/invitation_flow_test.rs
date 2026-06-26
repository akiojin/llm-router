//! 招待コードフローE2Eテスト
//!
//! 完全な招待コードフロー（発行 → ユーザー登録 → ログイン）
//!
//! NOTE: NodeRegistry廃止（SPEC-e8e9326e）に伴い、EndpointRegistryベースに更新済み。

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llmlb::common::auth::UserRole;
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

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
    let password_hash = llmlb::auth::password::hash_password("admin123").unwrap();
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
        queue_config: llmlb::config::QueueConfig::from_env(),
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

/// 管理者としてログインしJWTトークンを取得
async fn login_as_admin(app: Router) -> (Router, String) {
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
                        "password": "admin123"
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
    let jwt_token = login_data["token"].as_str().unwrap().to_string();

    (app, jwt_token)
}

#[tokio::test]
async fn test_complete_invitation_flow() {
    let (app, _db_pool) = build_app().await;

    // Step 1: 管理者としてログイン
    let (app, jwt_token) = login_as_admin(app).await;

    // Step 2: 招待コードを発行
    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/invitations")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
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

    let create_body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: serde_json::Value = serde_json::from_slice(&create_body).unwrap();

    let invitation_code = create_data["code"].as_str().unwrap();
    assert!(
        invitation_code.starts_with("inv_"),
        "Invitation code should start with 'inv_'"
    );

    // Step 3: 招待コードを使って新規ユーザーを登録
    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": invitation_code,
                        "username": "newuser",
                        "password": "Password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::CREATED);

    let register_body = axum::body::to_bytes(register_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let register_data: serde_json::Value = serde_json::from_slice(&register_body).unwrap();

    assert_eq!(register_data["username"], "newuser");
    assert_eq!(register_data["role"], "viewer");

    // Step 4: 登録したユーザーでログイン
    let new_user_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "username": "newuser",
                        "password": "Password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(new_user_login.status(), StatusCode::OK);

    // Step 5: 同じ招待コードを再使用しようとするとエラー
    let reuse_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": invitation_code,
                        "username": "anotheruser",
                        "password": "Password456"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        reuse_response.status(),
        StatusCode::BAD_REQUEST,
        "Used invitation code should be rejected"
    );
}

#[tokio::test]
async fn test_invalid_invitation_code_rejected() {
    let (app, _db_pool) = build_app().await;

    // 無効な招待コードで登録を試みる
    let register_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": "inv_invalidcode12345",
                        "username": "newuser",
                        "password": "Password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        register_response.status(),
        StatusCode::BAD_REQUEST,
        "Invalid invitation code should be rejected"
    );
}

#[tokio::test]
async fn test_invitation_revocation() {
    let (app, _db_pool) = build_app().await;

    // 管理者としてログイン
    let (app, jwt_token) = login_as_admin(app).await;

    // 招待コードを発行
    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/invitations")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let create_body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: serde_json::Value = serde_json::from_slice(&create_body).unwrap();

    let invitation_id = create_data["id"].as_str().unwrap();
    let invitation_code = create_data["code"].as_str().unwrap();

    // 招待コードを無効化
    let revoke_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/invitations/{}", invitation_id))
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(revoke_response.status(), StatusCode::NO_CONTENT);

    // 無効化された招待コードで登録を試みる
    let register_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": invitation_code,
                        "username": "newuser",
                        "password": "Password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        register_response.status(),
        StatusCode::BAD_REQUEST,
        "Revoked invitation code should be rejected"
    );
}

#[tokio::test]
async fn test_list_invitations() {
    let (app, _db_pool) = build_app().await;

    // 管理者としてログイン
    let (app, jwt_token) = login_as_admin(app).await;

    // 招待コードを2つ発行
    for _ in 0..2 {
        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/invitations")
                    .header("authorization", format!("Bearer {}", jwt_token))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(create_response.status(), StatusCode::CREATED);
    }

    // 招待コード一覧を取得
    let list_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/invitations")
                .header("authorization", format!("Bearer {}", jwt_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(list_response.status(), StatusCode::OK);

    let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_data: serde_json::Value = serde_json::from_slice(&list_body).unwrap();

    assert!(list_data["invitations"].is_array());
    assert_eq!(
        list_data["invitations"].as_array().unwrap().len(),
        2,
        "Should have 2 invitations"
    );
}

#[tokio::test]
async fn test_duplicate_username_rejected() {
    let (app, _db_pool) = build_app().await;

    // 管理者としてログイン
    let (app, jwt_token) = login_as_admin(app).await;

    // 招待コードを2つ発行
    let mut codes = Vec::new();
    for _ in 0..2 {
        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/invitations")
                    .header("authorization", format!("Bearer {}", jwt_token))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        codes.push(data["code"].as_str().unwrap().to_string());
    }

    // 最初の招待コードでユーザー登録
    let register_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": codes[0],
                        "username": "testuser",
                        "password": "Password123"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(register_response.status(), StatusCode::CREATED);

    // 同じユーザー名で2つ目の招待コードを使って登録
    let duplicate_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": codes[1],
                        "username": "testuser",
                        "password": "Password456"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        duplicate_response.status(),
        StatusCode::CONFLICT,
        "Duplicate username should return 409 Conflict"
    );
}

#[tokio::test]
async fn test_weak_password_rejected() {
    let (app, _db_pool) = build_app().await;

    // 管理者としてログインし招待コードを発行
    let (app, jwt_token) = login_as_admin(app).await;
    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/invitations")
                .header("authorization", format!("Bearer {}", jwt_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let create_data: serde_json::Value = serde_json::from_slice(&create_body).unwrap();
    let invitation_code = create_data["code"].as_str().unwrap();

    // 要件を満たさない弱いパスワード（短い・大文字なし・数字なし）で登録 → 400
    let register_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "invitation_code": invitation_code,
                        "username": "weakpwuser",
                        "password": "weak"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        register_response.status(),
        StatusCode::BAD_REQUEST,
        "Weak password should be rejected with 400"
    );
}
