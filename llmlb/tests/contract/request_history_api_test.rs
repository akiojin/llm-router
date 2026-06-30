//! リクエスト履歴API の Contract Tests
//!
//! T004-T006: API契約を検証

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::{Duration, SecondsFormat, Utc};
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

    // /api/dashboard/* は JWT のみのため、テスト用JWTを発行して返す。
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

/// T004: List API の contract test
#[tokio::test]
#[serial]
async fn test_list_request_responses_contract_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body["records"].is_array());
    assert_eq!(body["records"].as_array().unwrap().len(), 0);
    assert_eq!(body["total_count"], 0);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 10);
}

/// T004: List API - フィルタリングのコントラクトテスト
#[tokio::test]
#[serial]
async fn test_list_request_responses_with_filters() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let node_a = Uuid::new_v4();
    let node_b = Uuid::new_v4();

    let record_a = create_test_record("alpha-1", node_a, now - Duration::hours(3), true);
    let record_b = create_test_record("alpha-2", node_a, now - Duration::hours(2), false);
    let record_c = create_test_record("beta-1", node_b, now - Duration::hours(1), true);
    let record_d = create_test_record("beta-2", node_b, now - Duration::minutes(30), true);

    insert_record(&db_pool, &record_a).await;
    insert_record(&db_pool, &record_b).await;
    insert_record(&db_pool, &record_c).await;
    insert_record(&db_pool, &record_d).await;

    // モデル名フィルタ
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses?model=alpha")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total_count"], 2);

    // ノードIDフィルタ
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!(
                    "/api/dashboard/request-responses?node_id={}",
                    node_b
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total_count"], 2);

    // ステータスフィルタ
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses?status=error")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total_count"], 1);

    // 時間範囲フィルタ
    let start_time = (now - Duration::hours(2)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let end_time = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let response = app
        .clone()
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!(
                    "/api/dashboard/request-responses?start_time={}&end_time={}",
                    start_time, end_time
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total_count"], 3);

    // ページネーション（per_pageは許可された値のみ有効）
    let mut extra_records = Vec::new();
    for i in 0..12 {
        extra_records.push(create_test_record(
            "page-model",
            node_a,
            now - Duration::minutes(i as i64),
            true,
        ));
    }
    for record in &extra_records {
        insert_record(&db_pool, record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses?per_page=10&page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["page"], 2);
    assert_eq!(body["per_page"], 10);
    assert_eq!(body["total_count"], 16);
    assert_eq!(body["records"].as_array().unwrap().len(), 6);
}

/// T005: Detail API の contract test
#[tokio::test]
#[serial]
async fn test_get_request_response_detail_contract_not_found() {
    let (app, _db_pool, jwt) = build_app().await;
    let missing_id = Uuid::new_v4();

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!("/api/dashboard/request-responses/{}", missing_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// T005: Detail API - 存在するレコードのテスト
#[tokio::test]
#[serial]
async fn test_get_request_response_detail_contract_found() {
    let (app, db_pool, jwt) = build_app().await;
    let record = create_test_record("detail-model", Uuid::new_v4(), Utc::now(), true);
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!("/api/dashboard/request-responses/{}", record.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["id"], record.id.to_string());
    assert_eq!(body["model"], "detail-model");
}

/// T006: Export API の contract test - JSON 形式
#[tokio::test]
#[serial]
async fn test_export_request_responses_json_contract() {
    let (app, db_pool, jwt) = build_app().await;
    let record = create_test_record("export-json", Uuid::new_v4(), Utc::now(), true);
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses/export?format=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("Content-Type").unwrap(),
        "application/json"
    );
    assert_eq!(
        response.headers().get("Content-Disposition").unwrap(),
        "attachment; filename=\"request_history.json\""
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["model"] == "export-json"));
}

/// T006: Export API の contract test - CSV 形式
#[tokio::test]
#[serial]
async fn test_export_request_responses_csv_contract() {
    let (app, db_pool, jwt) = build_app().await;
    let record = create_test_record("export-csv", Uuid::new_v4(), Utc::now(), true);
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses/export?format=csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("Content-Type").unwrap(), "text/csv");
    assert_eq!(
        response.headers().get("Content-Disposition").unwrap(),
        "attachment; filename=\"request_history.csv\""
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("request_type"));
    assert!(body_str.contains("export-csv"));
}

/// T006: Export API - 無効なフォーマットのテスト
#[tokio::test]
#[serial]
async fn test_export_request_responses_invalid_format() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses/export?format=invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// T013: client_ipフィルターの統合テスト
/// SPEC-62ac4b68: IPフィルターで該当IPのレコードのみ返却されることを検証
#[tokio::test]
#[serial]
async fn test_list_request_responses_with_client_ip_filter() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 異なるIPのレコードを挿入
    let mut record_a = create_test_record("model-a", endpoint_id, now - Duration::hours(2), true);
    record_a.client_ip = Some("192.168.1.10".parse().unwrap());

    let mut record_b = create_test_record("model-b", endpoint_id, now - Duration::hours(1), true);
    record_b.client_ip = Some("192.168.1.20".parse().unwrap());

    let mut record_c =
        create_test_record("model-c", endpoint_id, now - Duration::minutes(30), true);
    record_c.client_ip = Some("192.168.1.10".parse().unwrap());

    insert_record(&db_pool, &record_a).await;
    insert_record(&db_pool, &record_b).await;
    insert_record(&db_pool, &record_c).await;

    // client_ip=192.168.1.10 でフィルター
    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/request-responses?client_ip=192.168.1.10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 192.168.1.10のレコードのみ返却（2件）
    assert_eq!(
        body["total_count"], 2,
        "client_ipフィルターで該当IPのレコードのみ返却されること"
    );
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);

    // 全レコードのclient_ipが指定値であること
    for record in records {
        assert_eq!(
            record["client_ip"].as_str().unwrap(),
            "192.168.1.10",
            "フィルター結果のclient_ipが一致すること"
        );
    }
}

fn create_test_record(
    model: &str,
    endpoint_id: Uuid,
    timestamp: chrono::DateTime<Utc>,
    success: bool,
) -> RequestResponseRecord {
    RequestResponseRecord {
        id: Uuid::new_v4(),
        timestamp,
        request_type: RequestType::Chat,
        model: model.to_string(),
        endpoint_id,
        endpoint_name: "test-node".to_string(),
        endpoint_ip: "127.0.0.1".parse().unwrap(),
        client_ip: Some("10.0.0.1".parse().unwrap()),
        request_body: json!({
            "model": model,
            "messages": [{"role": "user", "content": "hello"}]
        }),
        response_body: Some(json!({
            "choices": [{"message": {"role": "assistant", "content": "ok"}}]
        })),
        duration_ms: 42,
        status: if success {
            RecordStatus::Success
        } else {
            RecordStatus::Error {
                message: "error".to_string(),
            }
        },
        completed_at: timestamp + Duration::seconds(1),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        api_key_id: None,
    }
}
