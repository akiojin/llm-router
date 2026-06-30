//! Clients Alert Threshold API の Contract Tests
//!
//! T052-T053: 閾値設定・閾値超過検出テスト
//! SPEC-62ac4b68: 閾値ベースの異常検知

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

/// T052: GET /api/dashboard/settings/ip_alert_threshold - デフォルト値
#[tokio::test]
#[serial]
async fn test_alert_threshold_default() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/settings/ip_alert_threshold")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // デフォルト値100
    assert_eq!(body["value"], "100");
}

/// T052: PUT /api/dashboard/settings/ip_alert_threshold - 値更新
#[tokio::test]
#[serial]
async fn test_alert_threshold_update() {
    let (app, _db_pool, jwt) = build_app().await;

    // PUT で更新
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("PUT")
                .uri("/api/dashboard/settings/ip_alert_threshold")
                .header("content-type", "application/json")
                .body(Body::from(json!({"value": "50"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // GET で確認
    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/settings/ip_alert_threshold")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["value"], "50");
}

/// T053: 閾値超過時にis_alertがtrue
#[tokio::test]
#[serial]
async fn test_alert_threshold_detection() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 閾値を5に設定
    let _response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("PUT")
                .uri("/api/dashboard/settings/ip_alert_threshold")
                .header("content-type", "application/json")
                .body(Body::from(json!({"value": "5"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    // IP-A: 10リクエスト（閾値超過）
    let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
    for i in 0..10 {
        let record =
            create_test_record("model-a", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    // IP-C: 5リクエスト（閾値と等しい）
    let ip_c: std::net::IpAddr = "10.0.0.3".parse().unwrap();
    for i in 0..5 {
        let record = create_test_record(
            "model-a",
            endpoint_id,
            now - Duration::minutes(i),
            Some(ip_c),
        );
        insert_record(&db_pool, &record).await;
    }

    // IP-B: 3リクエスト（閾値未満）
    let ip_b: std::net::IpAddr = "10.0.0.2".parse().unwrap();
    for i in 0..3 {
        let record = create_test_record(
            "model-a",
            endpoint_id,
            now - Duration::minutes(i),
            Some(ip_b),
        );
        insert_record(&db_pool, &record).await;
    }

    // ランキング取得
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

    // IP-A（10件）は閾値(5)超過 → is_alert = true
    assert_eq!(rankings[0]["ip"], "10.0.0.1");
    assert_eq!(rankings[0]["is_alert"], true);

    // IP-C（5件）は閾値(5)と等しい → is_alert = true
    assert_eq!(rankings[1]["ip"], "10.0.0.3");
    assert_eq!(rankings[1]["is_alert"], true);

    // IP-B（3件）は閾値未満 → is_alert = false
    assert_eq!(rankings[2]["ip"], "10.0.0.2");
    assert_eq!(rankings[2]["is_alert"], false);
}
