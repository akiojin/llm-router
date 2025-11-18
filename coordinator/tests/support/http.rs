use std::{io, net::SocketAddr};

use axum::Router;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

/// 汎用的にテスト用のHTTPサーバーを起動するためのユーティリティ
#[allow(dead_code)]
pub struct TestServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    handle: JoinHandle<Result<(), io::Error>>,
}

#[allow(dead_code)]
impl TestServer {
    /// サーバーがバインドしているアドレスを返す
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// サーバーを停止し、バックグラウンドタスクの終了を待つ
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

/// 任意のRouterを実ポートにバインドして起動する
pub async fn spawn_router(router: Router) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = rx.await;
        })
        .await
    });

    TestServer {
        addr,
        shutdown: Some(tx),
        handle,
    }
}
