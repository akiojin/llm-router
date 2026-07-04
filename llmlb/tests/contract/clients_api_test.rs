//! Clients API の Contract Tests
//!
//! T018: IPランキングAPI統合テスト
//! SPEC-62ac4b68: Clientsタブ基本分析

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
use llmlb::common::auth::UserRole;
use llmlb::common::protocol::{RecordStatus, RequestResponseRecord, RequestType};
use llmlb::db::request_history::RequestHistoryStorage;
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn build_app() -> (Router, SqlitePool, String) {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin_user =
        llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
            .await
            .expect("create admin user");
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin_user.id.to_string(),
        UserRole::Admin,
        &crate::support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create admin jwt");

    (app, db_pool, jwt)
}

fn admin_request(jwt: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", jwt))
}

async fn insert_record(db_pool: &SqlitePool, record: &RequestResponseRecord) {
    let storage = RequestHistoryStorage::new(db_pool.clone());
    storage.save_record(record).await.unwrap();
}

fn create_test_record(
    model: &str,
    endpoint_id: Uuid,
    timestamp: chrono::DateTime<Utc>,
    client_ip: Option<std::net::IpAddr>,
) -> RequestResponseRecord {
    RequestResponseRecord {
        id: Uuid::new_v4(),
        timestamp,
        request_type: RequestType::Chat,
        model: model.to_string(),
        endpoint_id,
        endpoint_name: "test-node".to_string(),
        endpoint_ip: "127.0.0.1".parse().unwrap(),
        client_ip,
        request_body: json!({"model": model, "messages": [{"role": "user", "content": "hello"}]}),
        response_body: Some(
            json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}),
        ),
        duration_ms: 42,
        status: RecordStatus::Success,
        completed_at: timestamp + Duration::seconds(1),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        api_key_id: None,
    }
}

/// T018: GET /api/dashboard/clients - 空の場合
#[tokio::test]
#[serial]
async fn test_clients_api_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["rankings"].is_array());
    assert_eq!(body["rankings"].as_array().unwrap().len(), 0);
    assert_eq!(body["total_count"], 0);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 20);
}

/// T018: GET /api/dashboard/clients - IPランキング（リクエスト数降順）
#[tokio::test]
#[serial]
async fn test_clients_api_ranking_order() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // IP-A: 3リクエスト
    for i in 0..3 {
        let record = create_test_record(
            "model-a",
            endpoint_id,
            now - Duration::minutes(i as i64),
            Some("10.0.0.1".parse().unwrap()),
        );
        insert_record(&db_pool, &record).await;
    }

    // IP-B: 5リクエスト
    for i in 0..5 {
        let record = create_test_record(
            "model-b",
            endpoint_id,
            now - Duration::minutes(i as i64),
            Some("10.0.0.2".parse().unwrap()),
        );
        insert_record(&db_pool, &record).await;
    }

    // IP-C: 1リクエスト
    let record = create_test_record(
        "model-c",
        endpoint_id,
        now - Duration::minutes(1),
        Some("10.0.0.3".parse().unwrap()),
    );
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let rankings = body["rankings"].as_array().unwrap();
    assert_eq!(rankings.len(), 3);
    assert_eq!(body["total_count"], 3);

    // リクエスト数降順
    assert_eq!(rankings[0]["ip"], "10.0.0.2");
    assert_eq!(rankings[0]["request_count"], 5);
    assert_eq!(rankings[1]["ip"], "10.0.0.1");
    assert_eq!(rankings[1]["request_count"], 3);
    assert_eq!(rankings[2]["ip"], "10.0.0.3");
    assert_eq!(rankings[2]["request_count"], 1);

    // レスポンス型にlast_seenフィールド存在確認
    assert!(rankings[0]["last_seen"].is_string());
}

/// T018: GET /api/dashboard/clients - ページネーション
#[tokio::test]
#[serial]
async fn test_clients_api_pagination() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 25個の異なるIPを作成
    for i in 0..25 {
        let ip: std::net::IpAddr = format!("10.0.1.{}", i).parse().unwrap();
        let record =
            create_test_record("model-x", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    // ページ1（デフォルト20件）
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients?page=1&per_page=20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["rankings"].as_array().unwrap().len(), 20);
    assert_eq!(body["total_count"], 25);
    assert_eq!(body["page"], 1);

    // ページ2
    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients?page=2&per_page=20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["rankings"].as_array().unwrap().len(), 5);
    assert_eq!(body["page"], 2);
}

/// T018: GET /api/dashboard/clients - IPv6の/64グルーピング
#[tokio::test]
#[serial]
async fn test_clients_api_ipv6_grouping() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 同じ/64プレフィックスの異なるIPv6アドレス
    let ipv6_a: std::net::IpAddr = "2001:db8:1234:5678::1".parse().unwrap();
    let ipv6_b: std::net::IpAddr = "2001:db8:1234:5678::2".parse().unwrap();
    let ipv6_c: std::net::IpAddr = "2001:db8:1234:5678:abcd::1".parse().unwrap();

    // 異なる/64プレフィックスのIPv6
    let ipv6_d: std::net::IpAddr = "2001:db8:aaaa:bbbb::1".parse().unwrap();

    for ip in [ipv6_a, ipv6_b, ipv6_c] {
        let record = create_test_record(
            "model-v6",
            endpoint_id,
            now - Duration::minutes(1),
            Some(ip),
        );
        insert_record(&db_pool, &record).await;
    }

    let record = create_test_record(
        "model-v6",
        endpoint_id,
        now - Duration::minutes(2),
        Some(ipv6_d),
    );
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let rankings = body["rankings"].as_array().unwrap();
    // /64グルーピングにより、2001:db8:1234:5678::/64 は1つにまとまる → 2件
    assert_eq!(
        rankings.len(),
        2,
        "IPv6は/64でグルーピングされ2グループになるべき"
    );

    // 3リクエストのグループが先（降順）
    assert_eq!(rankings[0]["request_count"], 3);
    assert_eq!(rankings[1]["request_count"], 1);
}

/// T018: 認証なしでは401
#[tokio::test]
#[serial]
async fn test_clients_api_requires_auth() {
    let (app, _db_pool, _jwt) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
