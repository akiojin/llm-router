//! 自己更新状態のトレイ通知（GUI 連携）
//!
//! arch-review [H2]: update/mod.rs が純ドメイン・IO・OS プロセス・UI を混載して
//! いたため、トレイ(UI)への通知を submodule へ分離した。モジュール全体が
//! windows/macos 限定のため `mod tray;` ごと cfg でゲートする。

use super::schedule;
use tokio::sync::RwLock;

pub(super) fn schedule_to_tray_info(
    schedule: &schedule::UpdateSchedule,
) -> crate::gui::tray::ScheduleInfo {
    let mode = match schedule.mode {
        schedule::ScheduleMode::Immediate => "Immediate",
        schedule::ScheduleMode::Idle => "Idle",
        schedule::ScheduleMode::Scheduled => "Scheduled",
    }
    .to_string();

    crate::gui::tray::ScheduleInfo {
        mode,
        scheduled_at: schedule.scheduled_at.as_ref().cloned(),
    }
}

pub(super) async fn notify_tray_available(
    tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>,
    latest: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_available(latest);
    }
}
pub(super) async fn notify_tray_ready(tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_ready();
    }
}

pub(super) async fn notify_tray_failed(
    tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>,
    message: String,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_failed(message);
    }
}

pub(super) async fn notify_tray_up_to_date(
    tray: &RwLock<Option<crate::gui::tray::TrayEventProxy>>,
) {
    if let Some(proxy) = tray.read().await.clone() {
        proxy.notify_update_up_to_date();
    }
}
