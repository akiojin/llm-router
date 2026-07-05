//! エンドポイントデータベース操作
//!
//! SPEC-e8e9326e: llmlb主導エンドポイント登録システム

use crate::types::endpoint::{Endpoint, EndpointStatus};
use sqlx::SqlitePool;
use uuid::Uuid;

mod dto;
mod health_checks;
mod models;
pub use dto::EndpointRequestTotals;
use dto::{EndpointRequestTotalsRow, EndpointRow};
pub use health_checks::*;
pub use models::*;

/// エンドポイントを登録
pub async fn create_endpoint(pool: &SqlitePool, endpoint: &Endpoint) -> Result<(), sqlx::Error> {
    let id = endpoint.id.to_string();
    let status = endpoint.status.as_str();
    let registered_at = endpoint.registered_at.to_rfc3339();
    let last_seen = endpoint.last_seen.map(|dt| dt.to_rfc3339());
    let capabilities = serde_json::to_string(&endpoint.capabilities).unwrap_or_default();
    // SPEC-f8e3a1b7: デバイス情報と推論レイテンシ
    let device_info = endpoint
        .device_info
        .as_ref()
        .and_then(|d| serde_json::to_string(d).ok());

    let endpoint_type = endpoint.endpoint_type.as_str();
    sqlx::query(
        r#"
        INSERT INTO endpoints (
            id, name, base_url, api_key_encrypted, status, endpoint_type,
            health_check_interval_secs, inference_timeout_secs,
            latency_ms, last_seen, last_error, error_count,
            registered_at, notes, capabilities, device_info, inference_latency_ms
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&endpoint.name)
    .bind(&endpoint.base_url)
    .bind(&endpoint.api_key)
    .bind(status)
    .bind(endpoint_type)
    .bind(endpoint.health_check_interval_secs as i32)
    .bind(endpoint.inference_timeout_secs as i32)
    .bind(endpoint.latency_ms.map(|v| v as i32))
    .bind(&last_seen)
    .bind(&endpoint.last_error)
    .bind(endpoint.error_count as i32)
    .bind(&registered_at)
    .bind(&endpoint.notes)
    .bind(&capabilities)
    .bind(&device_info)
    .bind(endpoint.inference_latency_ms)
    .execute(pool)
    .await?;

    Ok(())
}

/// エンドポイント一覧を取得
pub async fn list_endpoints(pool: &SqlitePool) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        ORDER BY registered_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// IDでエンドポイントを取得
pub async fn get_endpoint(pool: &SqlitePool, id: Uuid) -> Result<Option<Endpoint>, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE id = ?
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}

/// エンドポイントを更新
pub async fn update_endpoint(pool: &SqlitePool, endpoint: &Endpoint) -> Result<bool, sqlx::Error> {
    let id = endpoint.id.to_string();
    let status = endpoint.status.as_str();
    let endpoint_type = endpoint.endpoint_type.as_str();
    let last_seen = endpoint.last_seen.map(|dt| dt.to_rfc3339());
    let capabilities = serde_json::to_string(&endpoint.capabilities).unwrap_or_default();
    // SPEC-f8e3a1b7: デバイス情報と推論レイテンシ
    let device_info = endpoint
        .device_info
        .as_ref()
        .and_then(|d| serde_json::to_string(d).ok());

    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            name = ?, base_url = ?, api_key_encrypted = ?, status = ?, endpoint_type = ?,
            health_check_interval_secs = ?, inference_timeout_secs = ?,
            latency_ms = ?, last_seen = ?, last_error = ?, error_count = ?,
            notes = ?, capabilities = ?, device_info = ?, inference_latency_ms = ?
        WHERE id = ?
        "#,
    )
    .bind(&endpoint.name)
    .bind(&endpoint.base_url)
    .bind(&endpoint.api_key)
    .bind(status)
    .bind(endpoint_type)
    .bind(endpoint.health_check_interval_secs as i32)
    .bind(endpoint.inference_timeout_secs as i32)
    .bind(endpoint.latency_ms.map(|v| v as i32))
    .bind(&last_seen)
    .bind(&endpoint.last_error)
    .bind(endpoint.error_count as i32)
    .bind(&endpoint.notes)
    .bind(&capabilities)
    .bind(&device_info)
    .bind(endpoint.inference_latency_ms)
    .bind(&id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントを削除
pub async fn delete_endpoint(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM endpoints WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// 名前でエンドポイントを検索
pub async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Endpoint>, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE name = ?
        "#,
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.into()))
}

/// ステータスでフィルタしてエンドポイント一覧を取得
pub async fn list_endpoints_by_status(
    pool: &SqlitePool,
    status: EndpointStatus,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE status = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(status.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// タイプでフィルタしてエンドポイント一覧を取得（SPEC-e8e9326e）
pub async fn list_endpoints_by_type(
    pool: &SqlitePool,
    endpoint_type: crate::types::endpoint::EndpointType,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE endpoint_type = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(endpoint_type.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// タイプとステータスでフィルタしてエンドポイント一覧を取得（SPEC-e8e9326e）
pub async fn list_endpoints_by_type_and_status(
    pool: &SqlitePool,
    endpoint_type: crate::types::endpoint::EndpointType,
    status: EndpointStatus,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EndpointRow>(
        r#"
        SELECT id, name, base_url, api_key_encrypted, status, endpoint_type,
               health_check_interval_secs, inference_timeout_secs,
               latency_ms, last_seen, last_error, error_count,
               registered_at, notes, capabilities,
               device_info, inference_latency_ms,
               total_requests, successful_requests, failed_requests
        FROM endpoints
        WHERE endpoint_type = ? AND status = ?
        ORDER BY registered_at DESC
        "#,
    )
    .bind(endpoint_type.as_str())
    .bind(status.as_str())
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.into()).collect())
}

/// エンドポイントのタイプを更新（SPEC-e8e9326e）
pub async fn update_endpoint_type(
    pool: &SqlitePool,
    id: Uuid,
    endpoint_type: crate::types::endpoint::EndpointType,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            endpoint_type = ?
        WHERE id = ?
        "#,
    )
    .bind(endpoint_type.as_str())
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのステータスを更新
pub async fn update_endpoint_status(
    pool: &SqlitePool,
    id: Uuid,
    status: EndpointStatus,
    latency_ms: Option<u32>,
    last_error: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            status = ?,
            latency_ms = COALESCE(?, latency_ms),
            last_seen = ?,
            last_error = ?,
            error_count = CASE WHEN ? = 'error' THEN error_count + 1 ELSE 0 END
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(latency_ms.map(|v| v as i32))
    .bind(&now)
    .bind(last_error)
    .bind(status.as_str())
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントの推論レイテンシを更新（SPEC-f8e3a1b7）
/// EMA (α=0.2) で計算された値を保存
pub async fn update_inference_latency(
    pool: &SqlitePool,
    id: Uuid,
    inference_latency_ms: Option<f64>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            inference_latency_ms = ?
        WHERE id = ?
        "#,
    )
    .bind(inference_latency_ms)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのデバイス情報を更新（SPEC-f8e3a1b7）
/// /api/system APIから取得した情報を保存
pub async fn update_device_info(
    pool: &SqlitePool,
    id: Uuid,
    device_info: Option<&crate::types::endpoint::DeviceInfo>,
) -> Result<bool, sqlx::Error> {
    let device_info_json = device_info.and_then(|d| serde_json::to_string(d).ok());
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            device_info = ?
        WHERE id = ?
        "#,
    )
    .bind(&device_info_json)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントのリクエストカウンタをインクリメント（SPEC-8c32349f）
pub async fn increment_request_counters(
    pool: &SqlitePool,
    id: Uuid,
    success: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE endpoints SET
            total_requests = total_requests + 1,
            successful_requests = successful_requests + CASE WHEN ? THEN 1 ELSE 0 END,
            failed_requests = failed_requests + CASE WHEN ? THEN 0 ELSE 1 END
        WHERE id = ?
        "#,
    )
    .bind(success)
    .bind(success)
    .bind(id.to_string())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// エンドポイントの累計リクエスト統計を集計して取得（TOPカード永続化用）
pub async fn get_request_totals(pool: &SqlitePool) -> Result<EndpointRequestTotals, sqlx::Error> {
    let row = sqlx::query_as::<_, EndpointRequestTotalsRow>(
        r#"
        SELECT
            COALESCE(SUM(total_requests), 0) as total_requests,
            COALESCE(SUM(successful_requests), 0) as successful_requests,
            COALESCE(SUM(failed_requests), 0) as failed_requests
        FROM endpoints
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(EndpointRequestTotals {
        total_requests: row.total_requests,
        successful_requests: row.successful_requests,
        failed_requests: row.failed_requests,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::TEST_LOCK;
    use crate::types::endpoint::{EndpointHealthCheck, EndpointModel, SupportedAPI};

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_endpoint_crud() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // Create
        let endpoint = Endpoint::new(
            "Test Endpoint".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &endpoint).await.unwrap();

        // Read
        let fetched = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Endpoint");
        assert_eq!(fetched.base_url, "http://localhost:8080");
        assert_eq!(fetched.status, EndpointStatus::Pending);

        // List
        let all = list_endpoints(&pool).await.unwrap();
        assert_eq!(all.len(), 1);

        // Update
        let mut updated = fetched;
        updated.status = EndpointStatus::Online;
        updated.latency_ms = Some(50);
        update_endpoint(&pool, &updated).await.unwrap();

        let fetched_again = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(fetched_again.status, EndpointStatus::Online);
        assert_eq!(fetched_again.latency_ms, Some(50));

        // Delete
        let deleted = delete_endpoint(&pool, endpoint.id).await.unwrap();
        assert!(deleted);

        let not_found = get_endpoint(&pool, endpoint.id).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_endpoint_model_crud() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // Create endpoint first
        let endpoint = Endpoint::new(
            "Model Test".to_string(),
            "http://localhost:8081".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &endpoint).await.unwrap();

        // Add model
        let model = EndpointModel {
            endpoint_id: endpoint.id,
            model_id: "llama3:8b".to_string(),
            capabilities: Some(vec!["chat".to_string(), "embeddings".to_string()]),
            max_tokens: None,
            last_checked: Some(chrono::Utc::now()),
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        add_endpoint_model(&pool, &model).await.unwrap();

        // List models
        let models = list_endpoint_models(&pool, endpoint.id).await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "llama3:8b");
        assert_eq!(
            models[0].capabilities,
            Some(vec!["chat".to_string(), "embeddings".to_string()])
        );

        // Delete model
        let deleted = delete_endpoint_model(&pool, endpoint.id, "llama3:8b")
            .await
            .unwrap();
        assert!(deleted);

        let models_after = list_endpoint_models(&pool, endpoint.id).await.unwrap();
        assert!(models_after.is_empty());
    }

    #[tokio::test]
    async fn test_health_check_crud() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // Create endpoint first
        let endpoint = Endpoint::new(
            "Health Test".to_string(),
            "http://localhost:8082".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &endpoint).await.unwrap();

        // Record health check
        let check = EndpointHealthCheck {
            id: 0,
            endpoint_id: endpoint.id,
            checked_at: chrono::Utc::now(),
            success: true,
            latency_ms: Some(25),
            error_message: None,
            status_before: EndpointStatus::Pending,
            status_after: EndpointStatus::Online,
        };
        let inserted_id = record_health_check(&pool, &check).await.unwrap();
        assert!(inserted_id > 0);

        // List health checks
        let checks = list_health_checks(&pool, endpoint.id, 10).await.unwrap();
        assert_eq!(checks.len(), 1);
        assert!(checks[0].success);
        assert_eq!(checks[0].latency_ms, Some(25));
    }

    #[tokio::test]
    async fn test_list_endpoints_by_status() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // Create endpoints with different statuses
        let mut ep1 = Endpoint::new(
            "Online EP".to_string(),
            "http://localhost:8083".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep1.status = EndpointStatus::Online;
        create_endpoint(&pool, &ep1).await.unwrap();

        let mut ep2 = Endpoint::new(
            "Offline EP".to_string(),
            "http://localhost:8084".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep2.status = EndpointStatus::Offline;
        create_endpoint(&pool, &ep2).await.unwrap();

        let ep3 = Endpoint::new(
            "Pending EP".to_string(),
            "http://localhost:8085".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep3).await.unwrap();

        // Filter by status
        let online = list_endpoints_by_status(&pool, EndpointStatus::Online)
            .await
            .unwrap();
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].name, "Online EP");

        let pending = list_endpoints_by_status(&pool, EndpointStatus::Pending)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, "Pending EP");
    }

    /// T001 [US2]: update_endpoint_status で latency_ms=None を渡した場合に
    /// 既存のレイテンシ値が保持されることを検証
    #[tokio::test]
    async fn test_update_status_preserves_latency_on_none() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // (1) エンドポイント登録
        let endpoint = Endpoint::new(
            "Latency Preserve".to_string(),
            "http://localhost:9100".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &endpoint).await.unwrap();

        // (2) latency_ms=Some(120) でステータス更新
        update_endpoint_status(&pool, endpoint.id, EndpointStatus::Online, Some(120), None)
            .await
            .unwrap();
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(ep.latency_ms, Some(120));

        // (3) latency_ms=None でオフラインに遷移（ヘルスチェック失敗を模擬）
        update_endpoint_status(
            &pool,
            endpoint.id,
            EndpointStatus::Offline,
            None,
            Some("health check failed"),
        )
        .await
        .unwrap();

        // (4) DBから読み取り、latency_ms が 120 のまま保持されていることを確認
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(
            ep.latency_ms,
            Some(120),
            "latency_ms should be preserved when None is passed (COALESCE)"
        );
        assert_eq!(ep.status, EndpointStatus::Offline);
    }

    #[tokio::test]
    async fn test_increment_request_counters() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint = Endpoint::new(
            "Counter Test".to_string(),
            "http://localhost:9090".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &endpoint).await.unwrap();

        // Verify initial counters are 0
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(ep.total_requests, 0);
        assert_eq!(ep.successful_requests, 0);
        assert_eq!(ep.failed_requests, 0);

        // Increment with success
        increment_request_counters(&pool, endpoint.id, true)
            .await
            .unwrap();
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(ep.total_requests, 1);
        assert_eq!(ep.successful_requests, 1);
        assert_eq!(ep.failed_requests, 0);

        // Increment with failure
        increment_request_counters(&pool, endpoint.id, false)
            .await
            .unwrap();
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(ep.total_requests, 2);
        assert_eq!(ep.successful_requests, 1);
        assert_eq!(ep.failed_requests, 1);

        // Multiple successes
        increment_request_counters(&pool, endpoint.id, true)
            .await
            .unwrap();
        increment_request_counters(&pool, endpoint.id, true)
            .await
            .unwrap();
        let ep = get_endpoint(&pool, endpoint.id).await.unwrap().unwrap();
        assert_eq!(ep.total_requests, 4);
        assert_eq!(ep.successful_requests, 3);
        assert_eq!(ep.failed_requests, 1);
    }

    #[tokio::test]
    async fn test_get_request_totals() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep1 = Endpoint::new(
            "Totals A".to_string(),
            "http://localhost:9091".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        let ep2 = Endpoint::new(
            "Totals B".to_string(),
            "http://localhost:9092".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep1).await.unwrap();
        create_endpoint(&pool, &ep2).await.unwrap();

        increment_request_counters(&pool, ep1.id, true)
            .await
            .unwrap();
        increment_request_counters(&pool, ep1.id, false)
            .await
            .unwrap();
        increment_request_counters(&pool, ep2.id, true)
            .await
            .unwrap();

        let totals = get_request_totals(&pool).await.unwrap();
        assert_eq!(totals.total_requests, 3);
        assert_eq!(totals.successful_requests, 2);
        assert_eq!(totals.failed_requests, 1);
    }

    // --- 追加テスト ---

    #[tokio::test]
    async fn test_find_by_name_found() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "unique-ep".to_string(),
            "http://localhost:7000".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let found = find_by_name(&pool, "unique-ep").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, ep.id);
    }

    #[tokio::test]
    async fn test_find_by_name_not_found() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let found = find_by_name(&pool, "no-such-name").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_get_endpoint_not_found() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let result = get_endpoint(&pool, Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_endpoint_returns_false() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let deleted = delete_endpoint(&pool, Uuid::new_v4()).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_list_endpoints_by_type() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep_xllm = Endpoint::new(
            "xllm-ep".to_string(),
            "http://localhost:7001".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        let ep_vllm = Endpoint::new(
            "vllm-ep".to_string(),
            "http://localhost:7002".to_string(),
            crate::types::endpoint::EndpointType::Vllm,
        );
        create_endpoint(&pool, &ep_xllm).await.unwrap();
        create_endpoint(&pool, &ep_vllm).await.unwrap();

        let xllm_list = list_endpoints_by_type(&pool, crate::types::endpoint::EndpointType::Xllm)
            .await
            .unwrap();
        assert_eq!(xllm_list.len(), 1);
        assert_eq!(xllm_list[0].name, "xllm-ep");

        let vllm_list = list_endpoints_by_type(&pool, crate::types::endpoint::EndpointType::Vllm)
            .await
            .unwrap();
        assert_eq!(vllm_list.len(), 1);
        assert_eq!(vllm_list[0].name, "vllm-ep");
    }

    #[tokio::test]
    async fn test_list_endpoints_by_type_and_status() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let mut ep1 = Endpoint::new(
            "xllm-online".to_string(),
            "http://localhost:7003".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep1.status = EndpointStatus::Online;
        let ep2 = Endpoint::new(
            "xllm-pending".to_string(),
            "http://localhost:7004".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep1).await.unwrap();
        create_endpoint(&pool, &ep2).await.unwrap();

        let result = list_endpoints_by_type_and_status(
            &pool,
            crate::types::endpoint::EndpointType::Xllm,
            EndpointStatus::Online,
        )
        .await
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "xllm-online");
    }

    #[tokio::test]
    async fn test_update_endpoint_type() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "type-change".to_string(),
            "http://localhost:7005".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let updated =
            update_endpoint_type(&pool, ep.id, crate::types::endpoint::EndpointType::Vllm)
                .await
                .unwrap();
        assert!(updated);

        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(
            fetched.endpoint_type,
            crate::types::endpoint::EndpointType::Vllm
        );
    }

    #[tokio::test]
    async fn test_update_endpoint_type_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let result = update_endpoint_type(
            &pool,
            Uuid::new_v4(),
            crate::types::endpoint::EndpointType::Xllm,
        )
        .await
        .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_update_inference_latency() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "latency-ep".to_string(),
            "http://localhost:7006".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        update_inference_latency(&pool, ep.id, Some(42.5))
            .await
            .unwrap();
        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert!((fetched.inference_latency_ms.unwrap() - 42.5).abs() < f64::EPSILON);

        // Reset to None
        update_inference_latency(&pool, ep.id, None).await.unwrap();
        let fetched2 = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert!(fetched2.inference_latency_ms.is_none());
    }

    #[tokio::test]
    async fn test_update_device_info() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "device-ep".to_string(),
            "http://localhost:7007".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let info = crate::types::endpoint::DeviceInfo {
            device_type: crate::types::endpoint::DeviceType::Gpu,
            gpu_devices: vec![crate::types::endpoint::GpuDevice {
                name: "RTX 4090".to_string(),
                total_memory_bytes: 24_000_000_000,
                used_memory_bytes: 8_000_000_000,
            }],
        };

        update_device_info(&pool, ep.id, Some(&info)).await.unwrap();
        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        let di = fetched.device_info.unwrap();
        assert_eq!(di.gpu_devices.len(), 1);
        assert_eq!(di.gpu_devices[0].name, "RTX 4090");

        // Clear device info
        update_device_info(&pool, ep.id, None).await.unwrap();
        let fetched2 = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert!(fetched2.device_info.is_none());
    }

    #[tokio::test]
    async fn test_update_endpoint_model() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "model-update".to_string(),
            "http://localhost:7008".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let model = EndpointModel {
            endpoint_id: ep.id,
            model_id: "phi3:mini".to_string(),
            capabilities: None,
            max_tokens: Some(2048),
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        add_endpoint_model(&pool, &model).await.unwrap();

        let mut updated_model = model;
        updated_model.max_tokens = Some(4096);
        updated_model.capabilities = Some(vec!["chat".to_string()]);
        let ok = update_endpoint_model(&pool, &updated_model).await.unwrap();
        assert!(ok);

        let models = list_endpoint_models(&pool, ep.id).await.unwrap();
        assert_eq!(models[0].max_tokens, Some(4096));
        assert_eq!(models[0].capabilities, Some(vec!["chat".to_string()]));
    }

    #[tokio::test]
    async fn test_update_model_max_tokens() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "max-tokens-ep".to_string(),
            "http://localhost:7009".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let model = EndpointModel {
            endpoint_id: ep.id,
            model_id: "gemma2:2b".to_string(),
            capabilities: None,
            max_tokens: None,
            last_checked: None,
            supported_apis: vec![SupportedAPI::ChatCompletions],
            canonical_name: None,
        };
        add_endpoint_model(&pool, &model).await.unwrap();

        let ok = update_model_max_tokens(&pool, ep.id, "gemma2:2b", 8192)
            .await
            .unwrap();
        assert!(ok);

        let models = list_endpoint_models(&pool, ep.id).await.unwrap();
        assert_eq!(models[0].max_tokens, Some(8192));
    }

    #[tokio::test]
    async fn test_delete_all_endpoint_models() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "delete-all-models".to_string(),
            "http://localhost:7010".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        for name in &["model-a", "model-b", "model-c"] {
            let m = EndpointModel {
                endpoint_id: ep.id,
                model_id: name.to_string(),
                capabilities: None,
                max_tokens: None,
                last_checked: None,
                supported_apis: vec![SupportedAPI::ChatCompletions],
                canonical_name: None,
            };
            add_endpoint_model(&pool, &m).await.unwrap();
        }

        let count = delete_all_endpoint_models(&pool, ep.id).await.unwrap();
        assert_eq!(count, 3);

        let models = list_endpoint_models(&pool, ep.id).await.unwrap();
        assert!(models.is_empty());
    }

    #[tokio::test]
    async fn test_get_request_totals_empty_db() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let totals = get_request_totals(&pool).await.unwrap();
        assert_eq!(totals.total_requests, 0);
        assert_eq!(totals.successful_requests, 0);
        assert_eq!(totals.failed_requests, 0);
    }

    #[tokio::test]
    async fn test_update_status_error_increments_error_count() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "error-count-ep".to_string(),
            "http://localhost:7011".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        // Error increments error_count
        update_endpoint_status(&pool, ep.id, EndpointStatus::Error, None, Some("timeout"))
            .await
            .unwrap();
        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(fetched.error_count, 1);
        assert_eq!(fetched.last_error, Some("timeout".to_string()));

        // Another error increments again
        update_endpoint_status(
            &pool,
            ep.id,
            EndpointStatus::Error,
            None,
            Some("connection refused"),
        )
        .await
        .unwrap();
        let fetched2 = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(fetched2.error_count, 2);

        // Online resets error_count to 0
        update_endpoint_status(&pool, ep.id, EndpointStatus::Online, Some(10), None)
            .await
            .unwrap();
        let fetched3 = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(fetched3.error_count, 0);
    }

    #[tokio::test]
    async fn test_health_check_with_error_message() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "hc-error".to_string(),
            "http://localhost:7012".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let check = EndpointHealthCheck {
            id: 0,
            endpoint_id: ep.id,
            checked_at: chrono::Utc::now(),
            success: false,
            latency_ms: None,
            error_message: Some("connection refused".to_string()),
            status_before: EndpointStatus::Online,
            status_after: EndpointStatus::Error,
        };
        record_health_check(&pool, &check).await.unwrap();

        let checks = list_health_checks(&pool, ep.id, 10).await.unwrap();
        assert_eq!(checks.len(), 1);
        assert!(!checks[0].success);
        assert_eq!(
            checks[0].error_message,
            Some("connection refused".to_string())
        );
        assert_eq!(checks[0].status_before, EndpointStatus::Online);
        assert_eq!(checks[0].status_after, EndpointStatus::Error);
    }

    #[tokio::test]
    async fn test_list_endpoints_empty_db() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let list = list_endpoints(&pool).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_update_nonexistent_endpoint_returns_false() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "ghost".to_string(),
            "http://localhost:9999".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        // Do not insert, just try to update
        let ok = update_endpoint(&pool, &ep).await.unwrap();
        assert!(!ok);
    }

    #[tokio::test]
    async fn test_update_endpoint_status_nonexistent_returns_false() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ok = update_endpoint_status(
            &pool,
            Uuid::new_v4(),
            EndpointStatus::Online,
            Some(10),
            None,
        )
        .await
        .unwrap();
        assert!(!ok);
    }

    #[tokio::test]
    async fn test_increment_request_counters_nonexistent_returns_false() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ok = increment_request_counters(&pool, Uuid::new_v4(), true)
            .await
            .unwrap();
        assert!(!ok);
    }

    #[tokio::test]
    async fn test_endpoint_with_api_key() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let mut ep = Endpoint::new(
            "key-ep".to_string(),
            "http://localhost:7020".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        ep.api_key = Some("sk-test-key-12345".to_string());
        create_endpoint(&pool, &ep).await.unwrap();

        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(fetched.api_key, Some("sk-test-key-12345".to_string()));
    }

    #[tokio::test]
    async fn test_endpoint_notes_persistence() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let mut ep = Endpoint::new(
            "notes-ep".to_string(),
            "http://localhost:7021".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep.notes = Some("Production server in rack A3".to_string());
        create_endpoint(&pool, &ep).await.unwrap();

        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(
            fetched.notes,
            Some("Production server in rack A3".to_string())
        );
    }

    #[tokio::test]
    async fn test_list_health_checks_respects_limit() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "hc-limit".to_string(),
            "http://localhost:7022".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        // Record 5 health checks
        for _ in 0..5 {
            let check = EndpointHealthCheck {
                id: 0,
                endpoint_id: ep.id,
                checked_at: chrono::Utc::now(),
                success: true,
                latency_ms: Some(10),
                error_message: None,
                status_before: EndpointStatus::Online,
                status_after: EndpointStatus::Online,
            };
            record_health_check(&pool, &check).await.unwrap();
        }

        let checks = list_health_checks(&pool, ep.id, 3).await.unwrap();
        assert_eq!(checks.len(), 3);
    }

    #[tokio::test]
    async fn test_list_health_checks_empty() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let checks = list_health_checks(&pool, Uuid::new_v4(), 10).await.unwrap();
        assert!(checks.is_empty());
    }

    #[tokio::test]
    async fn test_delete_endpoint_model_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let deleted = delete_endpoint_model(&pool, Uuid::new_v4(), "no-model")
            .await
            .unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_update_model_max_tokens_nonexistent() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ok = update_model_max_tokens(&pool, Uuid::new_v4(), "no-model", 4096)
            .await
            .unwrap();
        assert!(!ok);
    }

    #[tokio::test]
    async fn test_endpoint_row_into_endpoint_default_capabilities() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        // Create an endpoint with default capabilities
        let ep = Endpoint::new(
            "default-cap".to_string(),
            "http://localhost:7023".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert!(fetched
            .capabilities
            .contains(&crate::types::endpoint::EndpointCapability::ChatCompletion));
    }

    #[tokio::test]
    async fn test_list_endpoints_ordered_by_registered_at() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep1 = Endpoint::new(
            "first".to_string(),
            "http://localhost:7024".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep1).await.unwrap();

        // Small delay to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let ep2 = Endpoint::new(
            "second".to_string(),
            "http://localhost:7025".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep2).await.unwrap();

        let list = list_endpoints(&pool).await.unwrap();
        assert_eq!(list.len(), 2);
        // DESC order: most recently registered first
        assert_eq!(list[0].name, "second");
        assert_eq!(list[1].name, "first");
    }

    #[tokio::test]
    async fn test_cleanup_old_health_checks() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep = Endpoint::new(
            "cleanup-ep".to_string(),
            "http://localhost:7026".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        create_endpoint(&pool, &ep).await.unwrap();

        // Record a health check with a very old timestamp
        let old_time = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        sqlx::query(
            "INSERT INTO endpoint_health_checks (endpoint_id, checked_at, success, status_before, status_after) VALUES (?, ?, 1, 'online', 'online')",
        )
        .bind(ep.id.to_string())
        .bind(&old_time)
        .execute(&pool)
        .await
        .unwrap();

        // Record a recent health check
        let check = EndpointHealthCheck {
            id: 0,
            endpoint_id: ep.id,
            checked_at: chrono::Utc::now(),
            success: true,
            latency_ms: Some(5),
            error_message: None,
            status_before: EndpointStatus::Online,
            status_after: EndpointStatus::Online,
        };
        record_health_check(&pool, &check).await.unwrap();

        let cleaned = cleanup_old_health_checks(&pool).await.unwrap();
        assert_eq!(cleaned, 1);

        // Only the recent one should remain
        let checks = list_health_checks(&pool, ep.id, 10).await.unwrap();
        assert_eq!(checks.len(), 1);
    }

    #[tokio::test]
    async fn test_endpoint_multiple_capabilities() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let mut ep = Endpoint::new(
            "multi-cap".to_string(),
            "http://localhost:7027".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep.capabilities = vec![
            crate::types::endpoint::EndpointCapability::ChatCompletion,
            crate::types::endpoint::EndpointCapability::Embeddings,
            crate::types::endpoint::EndpointCapability::ImageGeneration,
        ];
        create_endpoint(&pool, &ep).await.unwrap();

        let fetched = get_endpoint(&pool, ep.id).await.unwrap().unwrap();
        assert_eq!(fetched.capabilities.len(), 3);
    }

    #[tokio::test]
    async fn test_endpoint_request_totals_struct() {
        let totals = EndpointRequestTotals {
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
        };
        assert_eq!(totals.total_requests, 100);
        assert_eq!(totals.successful_requests, 90);
        assert_eq!(totals.failed_requests, 10);
    }
}
