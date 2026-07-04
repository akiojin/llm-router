//! 自己更新状態の通知（UI 連携）
//!
//! arch-review [H2]: update/mod.rs が純ドメイン・IO・OS プロセス・UI を混載して
//! いたため、通知経路を submodule へ分離した。モジュール全体が windows/macos
//! 限定のため `mod tray;` ごと cfg でゲートする。
//!
//! arch-review [M8]: 通知先は具象 `TrayEventProxy` ではなく [`super::UpdateNotifier`]
//! trait 経由とし、update ドメインから gui への直接依存を排した。

use super::UpdateNotifier;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(super) async fn notify_tray_available(
    tray: &RwLock<Option<Arc<dyn UpdateNotifier>>>,
    latest: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_available(latest);
    }
}

pub(super) async fn notify_tray_ready(tray: &RwLock<Option<Arc<dyn UpdateNotifier>>>) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_ready();
    }
}

pub(super) async fn notify_tray_failed(
    tray: &RwLock<Option<Arc<dyn UpdateNotifier>>>,
    message: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_failed(message);
    }
}

pub(super) async fn notify_tray_up_to_date(tray: &RwLock<Option<Arc<dyn UpdateNotifier>>>) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_up_to_date();
    }
}
