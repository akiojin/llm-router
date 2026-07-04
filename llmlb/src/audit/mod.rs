//! 監査ログシステム (SPEC-8301d106)
//!
//! 全HTTP操作のメタデータを自動記録し、改ざん防止チェーンで保護する

/// 監査ログの型定義
pub mod types;

/// 非同期バッファライター
pub mod writer;

/// 監査ログミドルウェア
pub mod middleware;

/// SHA-256バッチハッシュチェーン（改ざん検知）
pub mod hash_chain;

use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// 監査タスクの実行間隔（24時間）
const AUDIT_TASK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// 24時間ごとの定期ハッシュチェーン検証タスクを起動する（SPEC-8301d106）。
///
/// 起動時検証は呼び出し側で実施済みのため、最初の tick はスキップする。
/// 以前は bootstrap::initialize_inner に inline されていたものを audit ドメインへ集約。
pub fn start_periodic_verification_task(storage: Arc<crate::db::audit_log::AuditLogStorage>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(AUDIT_TASK_INTERVAL);
        interval.tick().await; // 最初のtickはスキップ（起動時検証は実施済み）
        loop {
            interval.tick().await;
            match hash_chain::verify_chain(&storage).await {
                Ok(result) => {
                    if result.valid {
                        info!(
                            batches_checked = result.batches_checked,
                            "Periodic audit log hash chain verification passed"
                        );
                    } else {
                        warn!(
                            tampered_batch = ?result.tampered_batch,
                            message = ?result.message,
                            "Periodic audit log hash chain verification FAILED"
                        );
                    }
                }
                Err(e) => {
                    warn!("Periodic audit log hash chain verification error: {}", e);
                }
            }
        }
    });
}

/// 24時間ごとの定期アーカイブタスクを起動する（SPEC-8301d106）。
pub fn start_archive_task(
    storage: Arc<crate::db::audit_log::AuditLogStorage>,
    archive_pool: sqlx::SqlitePool,
    retention_days: i64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(AUDIT_TASK_INTERVAL);
        interval.tick().await; // 最初のtickをスキップ
        loop {
            interval.tick().await;
            match storage
                .archive_old_entries(retention_days, &archive_pool)
                .await
            {
                Ok(count) => {
                    if count > 0 {
                        info!(count, retention_days, "Archived old audit log entries");
                    }
                }
                Err(e) => {
                    warn!("Audit log archive task error: {}", e);
                }
            }
        }
    });
}
