//! Integration Test: dashboard WebSocket のセッション無効化を強制する
//!
//! 背景(Codex review, PR #679): `/ws/dashboard` は JWT 署名と admin ロールのみ検証し、
//! HTTP ダッシュボードAPIが適用する `password_changed_at` ベースのセッション無効化
//! （パスワード変更/リセット後の旧トークン拒否）を適用していなかった。
//! その結果、パスワード変更後も旧 admin トークンで WebSocket を開けてしまう。
//! 本テストは、無効化済み admin トークンが WS upgrade で 401 になることを検証する。

use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

use crate::support::lb::spawn_test_lb_with_db;

async fn login(client: &Client, addr: std::net::SocketAddr, username: &str) -> String {
    let resp = client
        .post(format!("http://{}/api/auth/login", addr))
        .json(&json!({ "username": username, "password": "password123" }))
        .send()
        .await
        .expect("login request");
    assert_eq!(
        resp.status().as_u16(),
        200,
        "login should succeed for {username}"
    );
    let data: Value = resp.json().await.expect("login json");
    data["token"]
        .as_str()
        .unwrap_or_else(|| panic!("login token for {username}"))
        .to_string()
}

fn ws_request(
    client: &Client,
    addr: std::net::SocketAddr,
    bearer: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client
        .get(format!("http://{}/ws/dashboard", addr))
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
    if let Some(token) = bearer {
        req = req.header("authorization", format!("Bearer {}", token));
    }
    req
}

/// パスワード変更で無効化された admin トークンは WS upgrade で 401 になる。
#[tokio::test]
async fn test_dashboard_ws_rejects_revoked_admin_token() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // admin ユーザー作成（password_changed_at=0）→ ログインでトークン取得
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin = llmlb::db::users::create(
        &db_pool,
        "ws_admin",
        &password_hash,
        llmlb::common::auth::UserRole::Admin,
        false,
    )
    .await
    .expect("create ws_admin");
    let token = login(&client, server.addr(), "ws_admin").await;

    // パスワード変更で password_changed_at を厳密単調増加させ、旧トークンを無効化
    let new_hash = llmlb::auth::password::hash_password("Password456!").unwrap();
    llmlb::db::users::update(&db_pool, admin.id, None, Some(&new_hash), None)
        .await
        .expect("update admin password");

    // 無効化済みトークンで WS upgrade を試行 → 401
    let resp = ws_request(&client, server.addr(), Some(&token))
        .send()
        .await
        .expect("ws request should complete with an HTTP response");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "revoked admin token must be rejected by the dashboard WebSocket"
    );
}

/// viewer ロールは従来通り WS で 403（リグレッション）。
#[tokio::test]
async fn test_dashboard_ws_rejects_viewer() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(
        &db_pool,
        "ws_viewer",
        &password_hash,
        llmlb::common::auth::UserRole::Viewer,
        false,
    )
    .await
    .expect("create ws_viewer");
    let token = login(&client, server.addr(), "ws_viewer").await;

    let resp = ws_request(&client, server.addr(), Some(&token))
        .send()
        .await
        .expect("ws request should complete");

    assert_eq!(
        resp.status().as_u16(),
        403,
        "viewer must be forbidden from the dashboard WebSocket"
    );
}
