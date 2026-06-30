//! Integration Test: LB Playground の Load Test を admin ロール限定にする
//!
//! 背景: Load Test は LB 経由で実推論リクエストを大量に発行する運用・診断ツール。
//! viewer（読み取り専用）が実行できると、登録エンドポイント（ローカル推論機など）へ
//! 過負荷を掛けられてしまう。そのため Load Test 専用エンドポイント
//! `/api/dashboard/playground/load-test/chat/completions` は admin ロールのみ許可する。
//! 一方、通常の Chat プレイグラウンド（`/api/dashboard/playground/chat/completions`）は
//! 全認証ユーザーが利用可能のまま（リグレッション防止）。

use reqwest::Client;
use serde_json::{json, Value};

use crate::support::lb::spawn_test_lb_with_db;

const LOAD_TEST_PATH: &str = "/api/dashboard/playground/load-test/chat/completions";
const CHAT_PATH: &str = "/api/dashboard/playground/chat/completions";

/// db に指定ロールのユーザーを作成する（パスワードは "password123"、変更要求なし）。
async fn create_user(
    db_pool: &sqlx::SqlitePool,
    username: &str,
    role: llmlb::common::auth::UserRole,
) {
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    llmlb::db::users::create(db_pool, username, &password_hash, role, false)
        .await
        .unwrap_or_else(|_| panic!("create user {username}"));
}

/// `/api/auth/login` でログインし JWT トークンを取得する。
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

fn chat_body() -> Value {
    json!({
        "model": "test-model",
        "messages": [{ "role": "user", "content": "load test ping" }],
        "stream": false,
    })
}

/// viewer は Load Test エンドポイントを実行できない（403）。
#[tokio::test]
async fn test_viewer_cannot_run_load_test() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    create_user(&db_pool, "lt_viewer", llmlb::common::auth::UserRole::Viewer).await;
    let viewer_jwt = login(&client, server.addr(), "lt_viewer").await;

    let resp = client
        .post(format!("http://{}{}", server.addr(), LOAD_TEST_PATH))
        .header("authorization", format!("Bearer {}", viewer_jwt))
        .json(&chat_body())
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status().as_u16(),
        403,
        "viewer must be forbidden from running the load test"
    );
}

/// admin は Load Test エンドポイントのロールゲートを通過できる（403 にならない）。
/// 実エンドポイント未登録のため下流はエラーになり得るが、admin が role gate で弾かれないことを確認する。
#[tokio::test]
async fn test_admin_can_reach_load_test_route() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    create_user(&db_pool, "lt_admin", llmlb::common::auth::UserRole::Admin).await;
    let admin_jwt = login(&client, server.addr(), "lt_admin").await;

    let resp = client
        .post(format!("http://{}{}", server.addr(), LOAD_TEST_PATH))
        .header("authorization", format!("Bearer {}", admin_jwt))
        .json(&chat_body())
        .send()
        .await
        .unwrap();

    // 実エンドポイント未登録のため下流は 404（model not found）等になり得るが、
    // 重要なのは admin が role gate（403）で弾かれないこと。
    assert_ne!(
        resp.status().as_u16(),
        403,
        "admin must pass the load test role gate"
    );
}

/// 通常の Chat プレイグラウンドは viewer でも利用可能（403/404 にならない＝リグレッション防止）。
#[tokio::test]
async fn test_viewer_can_still_use_chat_playground() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    create_user(&db_pool, "lt_viewer", llmlb::common::auth::UserRole::Viewer).await;
    let viewer_jwt = login(&client, server.addr(), "lt_viewer").await;

    let resp = client
        .post(format!("http://{}{}", server.addr(), CHAT_PATH))
        .header("authorization", format!("Bearer {}", viewer_jwt))
        .json(&chat_body())
        .send()
        .await
        .unwrap();

    // Chat は viewer でも role gate で弾かれない（下流は model not found 等になり得る）。
    assert_ne!(
        resp.status().as_u16(),
        403,
        "chat playground must remain available to viewers"
    );
}
