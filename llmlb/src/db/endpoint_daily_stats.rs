//! エンドポイント日次統計データベース操作
//!
//! SPEC-8c32349f: エンドポイント単位リクエスト統計
//! endpoint_daily_stats テーブルへのCRUD操作を提供する。
//! 日付はサーバーローカル時間 (chrono::Local::now().format("%Y-%m-%d").to_string()) を使用。

use sqlx::SqlitePool;
use uuid::Uuid;

mod queries;
mod seeding;
pub use queries::{get_all_model_stats, get_daily_stats, get_model_stats, get_today_stats};
pub use seeding::{get_today_stats_all, TpsSeedEntry};

/// 日次集計エントリ（日付ごとの集計結果）
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyStatEntry {
    /// 日付（YYYY-MM-DD形式）
    pub date: String,
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
}

/// モデル別集計エントリ（モデルごとの集計結果）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelStatEntry {
    /// モデルID
    pub model_id: String,
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 成功リクエスト数
    pub successful_requests: i64,
    /// 失敗リクエスト数
    pub failed_requests: i64,
    /// 出力トークン累計（SPEC-4bb5b55f）
    pub total_output_tokens: i64,
    /// 処理時間累計（ミリ秒、SPEC-4bb5b55f）
    pub total_duration_ms: i64,
}

/// 日次統計をUPSERT（挿入または更新）
///
/// 指定のエンドポイント・モデル・日付の組み合わせでレコードが存在しない場合は新規挿入、
/// 存在する場合はカウンタをインクリメントする。
pub async fn upsert_daily_stats(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
    date: &str,
    success: bool,
    output_tokens: u64,
    duration_ms: u64,
) -> Result<(), sqlx::Error> {
    upsert_daily_stats_with_api_kind(
        pool,
        endpoint_id,
        model_id,
        date,
        "chat_completions",
        success,
        output_tokens,
        duration_ms,
    )
    .await
}

/// api_kind指定付きの日次統計UPSERT
#[allow(clippy::too_many_arguments)]
pub async fn upsert_daily_stats_with_api_kind(
    pool: &SqlitePool,
    endpoint_id: Uuid,
    model_id: &str,
    date: &str,
    api_kind: &str,
    success: bool,
    output_tokens: u64,
    duration_ms: u64,
) -> Result<(), sqlx::Error> {
    let success_increment: i64 = if success { 1 } else { 0 };
    let failure_increment: i64 = if success { 0 } else { 1 };

    sqlx::query(
        r#"
        INSERT INTO endpoint_daily_stats (endpoint_id, model_id, date, api_kind, total_requests, successful_requests, failed_requests, total_output_tokens, total_duration_ms)
        VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?)
        ON CONFLICT(endpoint_id, model_id, date, api_kind) DO UPDATE SET
            total_requests = total_requests + 1,
            successful_requests = successful_requests + excluded.successful_requests,
            failed_requests = failed_requests + excluded.failed_requests,
            total_output_tokens = total_output_tokens + excluded.total_output_tokens,
            total_duration_ms = total_duration_ms + excluded.total_duration_ms
        "#,
    )
    .bind(endpoint_id.to_string())
    .bind(model_id)
    .bind(date)
    .bind(api_kind)
    .bind(success_increment)
    .bind(failure_increment)
    .bind(output_tokens as i64)
    .bind(duration_ms as i64)
    .execute(pool)
    .await?;

    Ok(())
}

/// 日次統計バッチタスクを開始（SPEC-8c32349f）
///
/// サーバーローカル時間の0:00に前日分の統計をログ出力する。
/// リアルタイムUPSERTで統計は更新済みのため、
/// このタスクは日次マーカーとログ記録の役割を担う。
pub fn start_daily_stats_task(pool: SqlitePool) {
    tokio::spawn(async move {
        loop {
            // 次の0:00までスリープ
            let now = chrono::Local::now();
            let tomorrow = (now + chrono::Duration::days(1))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("valid midnight time");
            let tomorrow = tomorrow
                .and_local_timezone(chrono::Local)
                .single()
                .unwrap_or_else(|| {
                    (now + chrono::Duration::days(1))
                        .date_naive()
                        .and_hms_opt(0, 0, 1)
                        .expect("valid midnight+1s")
                        .and_local_timezone(chrono::Local)
                        .latest()
                        .expect("valid local time")
                });
            let sleep_duration = (tomorrow - now).to_std().unwrap_or_default();
            tokio::time::sleep(sleep_duration).await;

            let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            tracing::info!("Daily stats batch: finalizing {}", yesterday);

            // 前日分のレコード数をログ出力
            match sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM endpoint_daily_stats WHERE date = ?",
            )
            .bind(&yesterday)
            .fetch_one(&pool)
            .await
            {
                Ok(count) => {
                    tracing::info!(
                        "Daily stats batch complete: {} records for {}",
                        count,
                        yesterday
                    );
                }
                Err(e) => {
                    tracing::error!("Daily stats batch failed: {}", e);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::TEST_LOCK;

    async fn setup_test_db() -> SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    #[tokio::test]
    async fn test_upsert_daily_stats_new_record() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_id = "llama3:8b";
        let date = "2025-01-15";

        // 成功リクエストを1件挿入
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 0, 0)
            .await
            .unwrap();

        // 挿入されたレコードを確認
        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.date, date);
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.failed_requests, 0);

        // 失敗リクエストを別のモデルで1件挿入
        let model_id_2 = "gpt-4";
        upsert_daily_stats(&pool, endpoint_id, model_id_2, date, false, 0, 0)
            .await
            .unwrap();

        // 日付でグループ化して取得（2モデル合計）
        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.failed_requests, 1);
    }

    #[tokio::test]
    async fn test_upsert_daily_stats_increment() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_id = "llama3:8b";
        let date = "2025-01-15";

        // 同一キーで複数回upsert
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_id, date, false, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_id, date, false, 0, 0)
            .await
            .unwrap();

        // カウンタが累積されていることを確認
        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.successful_requests, 3);
        assert_eq!(stats.failed_requests, 2);
    }

    #[tokio::test]
    async fn test_get_daily_stats() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_a = "llama3:8b";
        let model_b = "gpt-4";

        // 複数日にわたるデータを挿入
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-13", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-14", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-14", false, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-15", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-15", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-15", false, 0, 0)
            .await
            .unwrap();

        // 十分な日数で全データを取得（SQLiteのdate()関数は'now'基準のため、
        // テストデータが確実に範囲内に入るよう大きな値を指定）
        let stats = get_daily_stats(&pool, endpoint_id, 36500).await.unwrap();

        // 3日分のデータが日付昇順で返る
        assert_eq!(stats.len(), 3);

        // 2025-01-13: model_a成功1件
        assert_eq!(stats[0].date, "2025-01-13");
        assert_eq!(stats[0].total_requests, 1);
        assert_eq!(stats[0].successful_requests, 1);
        assert_eq!(stats[0].failed_requests, 0);

        // 2025-01-14: model_a成功1件 + model_b失敗1件
        assert_eq!(stats[1].date, "2025-01-14");
        assert_eq!(stats[1].total_requests, 2);
        assert_eq!(stats[1].successful_requests, 1);
        assert_eq!(stats[1].failed_requests, 1);

        // 2025-01-15: model_a成功2件 + model_b失敗1件
        assert_eq!(stats[2].date, "2025-01-15");
        assert_eq!(stats[2].total_requests, 3);
        assert_eq!(stats[2].successful_requests, 2);
        assert_eq!(stats[2].failed_requests, 1);

        // 別のエンドポイントでは空結果
        let other_id = Uuid::new_v4();
        let empty = get_daily_stats(&pool, other_id, 36500).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_get_model_stats() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_a = "llama3:8b";
        let model_b = "gpt-4";
        let model_c = "mistral:7b";

        // 複数モデル・複数日にわたるデータを挿入
        // model_a: 合計5件（成功4、失敗1）
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-14", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-14", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-15", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-15", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-15", false, 0, 0)
            .await
            .unwrap();

        // model_b: 合計3件（成功1、失敗2）
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-14", false, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-15", true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-15", false, 0, 0)
            .await
            .unwrap();

        // model_c: 合計1件（成功1）
        upsert_daily_stats(&pool, endpoint_id, model_c, "2025-01-15", true, 0, 0)
            .await
            .unwrap();

        let stats = get_model_stats(&pool, endpoint_id).await.unwrap();

        // total_requests降順で3モデル
        assert_eq!(stats.len(), 3);

        // model_a が最多
        assert_eq!(stats[0].model_id, model_a);
        assert_eq!(stats[0].total_requests, 5);
        assert_eq!(stats[0].successful_requests, 4);
        assert_eq!(stats[0].failed_requests, 1);

        // model_b が次
        assert_eq!(stats[1].model_id, model_b);
        assert_eq!(stats[1].total_requests, 3);
        assert_eq!(stats[1].successful_requests, 1);
        assert_eq!(stats[1].failed_requests, 2);

        // model_c が最少
        assert_eq!(stats[2].model_id, model_c);
        assert_eq!(stats[2].total_requests, 1);
        assert_eq!(stats[2].successful_requests, 1);
        assert_eq!(stats[2].failed_requests, 0);

        // 別のエンドポイントでは空結果
        let other_id = Uuid::new_v4();
        let empty = get_model_stats(&pool, other_id).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_get_all_model_stats_excludes_deleted_endpoints() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let mut endpoint_active = crate::types::endpoint::Endpoint::new(
            "active-endpoint".to_string(),
            "http://127.0.0.1:18001".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        endpoint_active.status = crate::types::endpoint::EndpointStatus::Online;
        crate::db::endpoints::create_endpoint(&pool, &endpoint_active)
            .await
            .unwrap();

        let mut endpoint_deleted = crate::types::endpoint::Endpoint::new(
            "deleted-endpoint".to_string(),
            "http://127.0.0.1:18002".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        endpoint_deleted.status = crate::types::endpoint::EndpointStatus::Offline;
        crate::db::endpoints::create_endpoint(&pool, &endpoint_deleted)
            .await
            .unwrap();

        upsert_daily_stats(
            &pool,
            endpoint_active.id,
            "model-a",
            "2026-02-26",
            true,
            0,
            0,
        )
        .await
        .unwrap();
        upsert_daily_stats(
            &pool,
            endpoint_deleted.id,
            "model-a",
            "2026-02-26",
            true,
            0,
            0,
        )
        .await
        .unwrap();
        upsert_daily_stats(
            &pool,
            endpoint_deleted.id,
            "model-b",
            "2026-02-26",
            false,
            0,
            0,
        )
        .await
        .unwrap();

        crate::db::endpoints::delete_endpoint(&pool, endpoint_deleted.id)
            .await
            .unwrap();

        let stats = get_all_model_stats(&pool).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model_id, "model-a");
        assert_eq!(stats[0].total_requests, 1);
        assert_eq!(stats[0].successful_requests, 1);
        assert_eq!(stats[0].failed_requests, 0);
    }

    #[tokio::test]
    async fn test_get_today_stats() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let today = "2025-01-15";

        // データがない場合はカウンタ0のエントリが返る
        let stats = get_today_stats(&pool, endpoint_id, today).await.unwrap();
        assert_eq!(stats.date, today);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 0);

        // データを挿入
        upsert_daily_stats(&pool, endpoint_id, "llama3:8b", today, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, "llama3:8b", today, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, "gpt-4", today, false, 0, 0)
            .await
            .unwrap();

        // 当日の集計を取得（全モデル合計）
        let stats = get_today_stats(&pool, endpoint_id, today).await.unwrap();
        assert_eq!(stats.date, today);
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.successful_requests, 2);
        assert_eq!(stats.failed_requests, 1);

        // 別の日付ではカウンタ0
        let other_date = "2025-01-16";
        let stats = get_today_stats(&pool, endpoint_id, other_date)
            .await
            .unwrap();
        assert_eq!(stats.date, other_date);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 0);
    }

    // SPEC-4bb5b55f T003: upsert_daily_statsにoutput_tokens/duration_msが累積加算されるテスト

    #[tokio::test]
    async fn test_upsert_daily_stats_tps_accumulation() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_id = "llama3:8b";
        let date = "2025-01-20";

        // output_tokens=100, duration_ms=2000 で1件目
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 100, 2000)
            .await
            .unwrap();

        // output_tokens=200, duration_ms=3000 で2件目（累積加算される）
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 200, 3000)
            .await
            .unwrap();

        // DBから直接クエリして累積値を検証
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT total_output_tokens, total_duration_ms FROM endpoint_daily_stats WHERE endpoint_id = ? AND model_id = ? AND date = ?"
        )
        .bind(endpoint_id.to_string())
        .bind(model_id)
        .bind(date)
        .fetch_one(&pool)
        .await
        .expect("TPS columns should exist after migration 016");

        // 累積加算: 100+200=300, 2000+3000=5000
        assert_eq!(row.0, 300, "total_output_tokens should be 300");
        assert_eq!(row.1, 5000, "total_duration_ms should be 5000");

        // 既存のリクエストカウント動作に影響しないことを確認
        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.successful_requests, 2);
        assert_eq!(stats.failed_requests, 0);
    }

    #[tokio::test]
    async fn test_upsert_daily_stats_tps_zero_values_no_impact() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_id = "gpt-4";
        let date = "2025-01-20";

        // output_tokens=0, duration_ms=0 で挿入（既存の呼び出しパターン）
        upsert_daily_stats(&pool, endpoint_id, model_id, date, true, 0, 0)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_id, date, false, 0, 0)
            .await
            .unwrap();

        // DBから直接クエリして値が0のまま
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT total_output_tokens, total_duration_ms FROM endpoint_daily_stats WHERE endpoint_id = ? AND model_id = ? AND date = ?"
        )
        .bind(endpoint_id.to_string())
        .bind(model_id)
        .bind(date)
        .fetch_one(&pool)
        .await
        .expect("TPS columns should exist after migration 016");

        assert_eq!(row.0, 0, "total_output_tokens should be 0");
        assert_eq!(row.1, 0, "total_duration_ms should be 0");

        // 通常のカウントは正常
        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 2);
    }

    // SPEC-4bb5b55f T008: get_model_statsにTPS情報が含まれることを検証

    #[tokio::test]
    async fn test_get_model_stats_includes_tps_data() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let model_a = "llama3:8b";
        let model_b = "gpt-4";

        // model_a: 2件、tokens=300, duration=5000ms
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-20", true, 100, 2000)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, model_a, "2025-01-20", true, 200, 3000)
            .await
            .unwrap();

        // model_b: 1件、tokens=50, duration=1000ms
        upsert_daily_stats(&pool, endpoint_id, model_b, "2025-01-20", true, 50, 1000)
            .await
            .unwrap();

        let stats = get_model_stats(&pool, endpoint_id).await.unwrap();
        assert_eq!(stats.len(), 2);

        // model_a: total_output_tokens=300, total_duration_ms=5000
        let a = &stats[0];
        assert_eq!(a.model_id, model_a);
        assert_eq!(a.total_requests, 2);
        assert_eq!(
            a.total_output_tokens, 300,
            "ModelStatEntry should include total_output_tokens"
        );
        assert_eq!(
            a.total_duration_ms, 5000,
            "ModelStatEntry should include total_duration_ms"
        );

        // model_b: total_output_tokens=50, total_duration_ms=1000
        let b = &stats[1];
        assert_eq!(b.model_id, model_b);
        assert_eq!(
            b.total_output_tokens, 50,
            "ModelStatEntry should include total_output_tokens"
        );
        assert_eq!(
            b.total_duration_ms, 1000,
            "ModelStatEntry should include total_duration_ms"
        );

        // 日次平均TPSが計算可能であることを確認
        // model_a: 300 / (5000/1000) = 60 tok/s
        let tps_a = a.total_output_tokens as f64 / (a.total_duration_ms as f64 / 1000.0);
        assert!(
            (tps_a - 60.0).abs() < 0.01,
            "日次TPS計算: expected 60.0, got {tps_a}"
        );
    }

    /// T017 [US4]: get_today_stats_all が当日のTPS関連データを正しく返すことを検証
    #[tokio::test]
    async fn test_get_today_stats_all() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2026-02-24";

        // (1) endpoint_daily_stats にデータ挿入
        // total_output_tokens=100, total_duration_ms=2000 になるようにupsert
        upsert_daily_stats(&pool, endpoint_id, "test-model", date, true, 100, 2000)
            .await
            .unwrap();

        // (2) get_today_stats_all 呼び出し
        let entries = get_today_stats_all(&pool, date).await.unwrap();

        // (3) 返却された TpsSeedEntry が正しいことを確認
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].endpoint_id, endpoint_id);
        assert_eq!(entries[0].model_id, "test-model");
        assert_eq!(entries[0].api_kind, "chat_completions");
        assert_eq!(entries[0].total_output_tokens, 100);
        assert_eq!(entries[0].total_duration_ms, 2000);
        assert_eq!(entries[0].successful_requests, 1);
    }

    // --- Additional tests ---

    #[tokio::test]
    async fn test_upsert_with_api_kind() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2025-02-01";

        // Insert with different api_kinds for the same model
        upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            "llama3:8b",
            date,
            "chat_completions",
            true,
            100,
            1000,
        )
        .await
        .unwrap();

        upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            "llama3:8b",
            date,
            "completions",
            true,
            50,
            500,
        )
        .await
        .unwrap();

        // Model stats should show aggregated values across api_kinds
        let stats = get_model_stats(&pool, endpoint_id).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_requests, 2);
        assert_eq!(stats[0].total_output_tokens, 150);
        assert_eq!(stats[0].total_duration_ms, 1500);
    }

    #[tokio::test]
    async fn test_upsert_same_api_kind_increments() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2025-02-02";

        // Same endpoint/model/date/api_kind should increment
        upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            "model-a",
            date,
            "responses",
            true,
            10,
            100,
        )
        .await
        .unwrap();

        upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            "model-a",
            date,
            "responses",
            false,
            20,
            200,
        )
        .await
        .unwrap();

        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.failed_requests, 1);
    }

    #[tokio::test]
    async fn test_get_today_stats_empty_returns_zeros() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let stats = get_today_stats(&pool, endpoint_id, "2099-01-01")
            .await
            .unwrap();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 0);
        assert_eq!(stats.date, "2099-01-01");
    }

    #[tokio::test]
    async fn test_get_daily_stats_empty_for_other_endpoint() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep1 = Uuid::new_v4();
        let ep2 = Uuid::new_v4();

        upsert_daily_stats(&pool, ep1, "model", "2025-01-01", true, 0, 0)
            .await
            .unwrap();

        let stats = get_daily_stats(&pool, ep2, 36500).await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_get_model_stats_empty_for_other_endpoint() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep1 = Uuid::new_v4();
        let ep2 = Uuid::new_v4();

        upsert_daily_stats(&pool, ep1, "model", "2025-01-01", true, 10, 100)
            .await
            .unwrap();

        let stats = get_model_stats(&pool, ep2).await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_get_today_stats_all_excludes_zero_tokens() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2026-03-01";

        // Record with zero tokens should be excluded from TPS seeding
        upsert_daily_stats(&pool, endpoint_id, "model-zero", date, true, 0, 0)
            .await
            .unwrap();

        // Record with tokens should be included
        upsert_daily_stats(
            &pool,
            endpoint_id,
            "model-with-tokens",
            date,
            true,
            100,
            2000,
        )
        .await
        .unwrap();

        let entries = get_today_stats_all(&pool, date).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model_id, "model-with-tokens");
    }

    #[tokio::test]
    async fn test_get_today_stats_all_empty_date() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let entries = get_today_stats_all(&pool, "2099-12-31").await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_endpoints_daily_stats() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let ep1 = Uuid::new_v4();
        let ep2 = Uuid::new_v4();
        let date = "2025-03-01";

        upsert_daily_stats(&pool, ep1, "model-a", date, true, 50, 500)
            .await
            .unwrap();
        upsert_daily_stats(&pool, ep2, "model-a", date, true, 100, 1000)
            .await
            .unwrap();

        // Each endpoint should have independent stats
        let stats1 = get_today_stats(&pool, ep1, date).await.unwrap();
        assert_eq!(stats1.total_requests, 1);

        let stats2 = get_today_stats(&pool, ep2, date).await.unwrap();
        assert_eq!(stats2.total_requests, 1);

        // Model stats are per-endpoint
        let model_stats1 = get_model_stats(&pool, ep1).await.unwrap();
        assert_eq!(model_stats1.len(), 1);
        assert_eq!(model_stats1[0].total_output_tokens, 50);

        let model_stats2 = get_model_stats(&pool, ep2).await.unwrap();
        assert_eq!(model_stats2.len(), 1);
        assert_eq!(model_stats2[0].total_output_tokens, 100);
    }

    #[tokio::test]
    async fn test_daily_stat_entry_serialization() {
        let entry = DailyStatEntry {
            date: "2025-01-15".to_string(),
            total_requests: 100,
            successful_requests: 90,
            failed_requests: 10,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["date"], "2025-01-15");
        assert_eq!(json["total_requests"], 100);
        assert_eq!(json["successful_requests"], 90);
        assert_eq!(json["failed_requests"], 10);
    }

    #[tokio::test]
    async fn test_model_stat_entry_serialization() {
        let entry = ModelStatEntry {
            model_id: "llama3:8b".to_string(),
            total_requests: 50,
            successful_requests: 45,
            failed_requests: 5,
            total_output_tokens: 10000,
            total_duration_ms: 50000,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["model_id"], "llama3:8b");
        assert_eq!(json["total_output_tokens"], 10000);
        assert_eq!(json["total_duration_ms"], 50000);
    }

    #[tokio::test]
    async fn test_upsert_failure_increments_correctly() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2025-04-01";

        // All failures
        for _ in 0..5 {
            upsert_daily_stats(&pool, endpoint_id, "model-f", date, false, 0, 0)
                .await
                .unwrap();
        }

        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 5);
    }

    #[tokio::test]
    async fn test_get_today_stats_all_multiple_models() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2026-04-01";

        upsert_daily_stats(&pool, endpoint_id, "model-x", date, true, 100, 1000)
            .await
            .unwrap();
        upsert_daily_stats(&pool, endpoint_id, "model-y", date, true, 200, 2000)
            .await
            .unwrap();

        let entries = get_today_stats_all(&pool, date).await.unwrap();
        assert_eq!(entries.len(), 2);

        let model_ids: Vec<&str> = entries.iter().map(|e| e.model_id.as_str()).collect();
        assert!(model_ids.contains(&"model-x"));
        assert!(model_ids.contains(&"model-y"));
    }

    #[tokio::test]
    async fn test_daily_stat_row_into_daily_stat_entry() {
        // Verify From<DailyStatRow> for DailyStatEntry via the public API
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2025-06-01";

        upsert_daily_stats(&pool, endpoint_id, "m1", date, true, 0, 0)
            .await
            .unwrap();

        let stats = get_today_stats(&pool, endpoint_id, date).await.unwrap();
        assert_eq!(stats.date, "2025-06-01");
        assert_eq!(stats.total_requests, 1);
    }

    #[tokio::test]
    async fn test_model_stat_row_into_model_stat_entry() {
        // Verify From<ModelStatRow> for ModelStatEntry via the public API
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();

        upsert_daily_stats(&pool, endpoint_id, "m-conv", "2025-06-01", true, 500, 10000)
            .await
            .unwrap();

        let stats = get_model_stats(&pool, endpoint_id).await.unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model_id, "m-conv");
        assert_eq!(stats[0].total_output_tokens, 500);
        assert_eq!(stats[0].total_duration_ms, 10000);
    }

    #[tokio::test]
    async fn test_tps_seed_entry_fields() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2026-06-01";

        upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            "tps-model",
            date,
            "responses",
            true,
            999,
            5000,
        )
        .await
        .unwrap();

        let entries = get_today_stats_all(&pool, date).await.unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.endpoint_id, endpoint_id);
        assert_eq!(entry.model_id, "tps-model");
        assert_eq!(entry.api_kind, "responses");
        assert_eq!(entry.total_output_tokens, 999);
        assert_eq!(entry.total_duration_ms, 5000);
        assert_eq!(entry.successful_requests, 1);
    }

    #[tokio::test]
    async fn test_get_all_model_stats_empty() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let stats = get_all_model_stats(&pool).await.unwrap();
        assert!(stats.is_empty());
    }

    #[tokio::test]
    async fn test_upsert_large_token_counts() {
        let _lock = TEST_LOCK.lock().await;
        let pool = setup_test_db().await;

        let endpoint_id = Uuid::new_v4();
        let date = "2025-07-01";

        // Large values to test for overflow
        upsert_daily_stats(
            &pool,
            endpoint_id,
            "big-model",
            date,
            true,
            1_000_000,
            60_000,
        )
        .await
        .unwrap();
        upsert_daily_stats(
            &pool,
            endpoint_id,
            "big-model",
            date,
            true,
            2_000_000,
            120_000,
        )
        .await
        .unwrap();

        let stats = get_model_stats(&pool, endpoint_id).await.unwrap();
        assert_eq!(stats[0].total_output_tokens, 3_000_000);
        assert_eq!(stats[0].total_duration_ms, 180_000);
    }
}
