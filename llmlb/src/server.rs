//! axumサーバー起動・シャットダウンハンドリング

use crate::shutdown::ShutdownController;
use crate::AppState;
use std::{net::SocketAddr, time::Duration};
use tracing::info;

const BIND_RETRY_TIMEOUT: Duration = Duration::from_secs(5);
const BIND_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// axumサーバーを起動し、シャットダウンシグナルを待機する
pub async fn run(state: AppState, bind_addr: &str) {
    let shutdown = state.shutdown.clone();

    let app = crate::api::create_app(state);

    let listener = bind_listener_with_retry(bind_addr, BIND_RETRY_TIMEOUT, BIND_RETRY_INTERVAL)
        .await
        .expect("Failed to bind to address");

    info!("LLM Load Balancer server listening on {}", bind_addr);

    let shutdown_signal = shutdown_signal(shutdown);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await
    .expect("Server error");

    info!("Server shutdown complete");
}

/// シャットダウンシグナルを待機
async fn shutdown_signal(shutdown: ShutdownController) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        }
        _ = terminate => {
            info!("Received SIGTERM, shutting down...");
        }
        _ = shutdown.wait() => {
            info!("Shutdown requested, shutting down...");
        }
    }
}

async fn bind_listener_with_retry(
    bind_addr: &str,
    retry_timeout: Duration,
    retry_interval: Duration,
) -> std::io::Result<tokio::net::TcpListener> {
    let deadline = tokio::time::Instant::now() + retry_timeout;
    let mut waited_for_handoff = false;

    loop {
        match tokio::net::TcpListener::bind(bind_addr).await {
            Ok(listener) => {
                if waited_for_handoff {
                    info!("Recovered bind to {} after restart handoff wait", bind_addr);
                }
                return Ok(listener);
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::AddrInUse
                    && tokio::time::Instant::now() < deadline =>
            {
                waited_for_handoff = true;
                tokio::time::sleep(retry_interval).await;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn shutdown_signal_completes_when_controller_requests_shutdown() {
        let shutdown = ShutdownController::default();
        let wait_task = tokio::spawn(shutdown_signal(shutdown.clone()));

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        shutdown.request_shutdown();

        tokio::time::timeout(std::time::Duration::from_secs(2), wait_task)
            .await
            .expect("shutdown signal task timed out")
            .expect("shutdown signal task panicked");
    }

    #[tokio::test]
    async fn bind_listener_with_retry_waits_for_port_handoff() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("bind occupied port");
        let addr = occupied.local_addr().expect("read occupied addr");
        let bind_addr = addr.to_string();

        let release_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            drop(occupied);
        });

        let rebound = bind_listener_with_retry(
            &bind_addr,
            Duration::from_millis(500),
            Duration::from_millis(25),
        )
        .await
        .expect("listener should bind after previous process releases the port");

        drop(rebound);
        release_task.await.expect("release task should finish");
    }
}
