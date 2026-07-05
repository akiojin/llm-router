//! 定期ヘルスチェックの起動とファンアウト。
//!
//! バックグラウンド監視ループの起動と、全エンドポイントの逐次/並列チェックを担う。

use crate::db::endpoints as db;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info};

impl super::EndpointHealthChecker {
    /// バックグラウンドで監視を開始
    pub fn start(self) {
        tokio::spawn(async move {
            // Run an initial parallel check to converge quickly without delaying server startup.
            if let Err(e) = self.check_all_endpoints_parallel().await {
                error!("Startup health check error: {}", e);
            }
            self.monitor_loop().await;
        });
    }

    /// 監視ループ
    async fn monitor_loop(&self) {
        let mut timer = interval(Duration::from_secs(self.check_interval_secs));

        info!(
            interval_secs = self.check_interval_secs,
            "Endpoint health checker started"
        );

        // `interval()` ticks immediately on the first call. Since we already performed an initial
        // startup check, wait a full interval before the next periodic check.
        timer.tick().await;

        loop {
            timer.tick().await;

            if let Err(e) = self.check_all_endpoints().await {
                error!("Health check error: {}", e);
            }

            // 古いヘルスチェック履歴をクリーンアップ
            if let Err(e) = db::cleanup_old_health_checks(self.registry.pool()).await {
                error!("Failed to cleanup old health checks: {}", e);
            }
        }
    }

    /// 全エンドポイントのヘルスチェック
    pub async fn check_all_endpoints(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let endpoints = self.registry.list().await;

        for endpoint in endpoints {
            if let Err(e) = self.check_endpoint(&endpoint).await {
                debug!(
                    endpoint_id = %endpoint.id,
                    endpoint_name = %endpoint.name,
                    error = %e,
                    "Health check failed"
                );
            }
        }

        Ok(())
    }

    /// 全エンドポイントを並列チェック（起動時用）
    pub async fn check_all_endpoints_parallel(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let endpoints = self.registry.list().await;

        if endpoints.is_empty() {
            info!("No endpoints to check");
            return Ok(());
        }

        info!(
            count = endpoints.len(),
            "Starting parallel health check for all endpoints"
        );

        let mut handles = Vec::with_capacity(endpoints.len());

        for endpoint in endpoints {
            let checker = self.clone();
            handles.push(tokio::spawn(async move {
                let result = checker.check_endpoint(&endpoint).await;
                (endpoint.id, endpoint.name.clone(), result)
            }));
        }

        let mut success_count = 0;
        let mut failure_count = 0;

        for handle in handles {
            match handle.await {
                Ok((id, name, result)) => {
                    if result.is_ok() {
                        success_count += 1;
                    } else {
                        failure_count += 1;
                        debug!(
                            endpoint_id = %id,
                            endpoint_name = %name,
                            "Parallel health check failed"
                        );
                    }
                }
                Err(e) => {
                    error!("Task join error: {}", e);
                    failure_count += 1;
                }
            }
        }

        info!(
            success = success_count,
            failure = failure_count,
            "Parallel health check completed"
        );

        Ok(())
    }
}
