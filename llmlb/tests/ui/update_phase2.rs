// Phase 2 UI tests for dashboard update features (FR-021/FR-022/FR-031/FR-034/FR-035/FR-036/FR-038)
//
// Source-level guarantees for Phase 2 update banner enhancements:
// - DL progress bar with byte counts
// - Viewer role hides update banner
// - Drain timeout countdown display
// - Rollback button with confirmation dialog
// - Update settings modal (gear icon)
// - Schedule state display in banner
// - UI throttling on manual check

fn get_dashboard_source() -> String {
    include_str!("../../src/web/dashboard/src/pages/Dashboard.tsx").to_string()
}

fn get_system_ts_source() -> String {
    include_str!("../../src/web/dashboard/src/lib/api/system.ts").to_string()
}

// T280: FR-022 — DL progress bar shows when PayloadState is downloading
#[test]
fn download_progress_bar_displayed_for_downloading_state() {
    let source = get_dashboard_source();
    assert!(
        source.contains("downloaded_bytes") && source.contains("total_bytes"),
        "Dashboard should display download progress using downloaded_bytes and total_bytes"
    );
}

#[test]
fn download_progress_shows_percentage() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Progress"),
        "Dashboard should use a Progress component for download progress"
    );
}

// T281: FR-035 — viewer role hides update banner and operation buttons
#[test]
fn viewer_role_hides_update_banner() {
    let source = get_dashboard_source();
    // The updateBanner is already conditionally rendered with {!isViewer && updateBanner}
    assert!(
        source.contains("!isViewer && updateBanner"),
        "Update banner should be hidden for viewer role"
    );
}

// T283: FR-034 — drain timeout countdown display
#[test]
fn drain_timeout_countdown_displayed() {
    let source = get_dashboard_source();
    assert!(
        source.contains("timeout_at"),
        "Dashboard should reference timeout_at for drain timeout countdown"
    );
    assert!(
        source.contains("Drain timeout in"),
        "Dashboard should show drain timeout countdown text"
    );
}

#[test]
fn applying_phase_message_and_timeout_are_displayed() {
    let source = get_dashboard_source();
    assert!(
        source.contains("phase_message"),
        "Dashboard should display applying phase_message when provided"
    );
    assert!(
        source.contains("Apply timeout in"),
        "Dashboard should show applying timeout countdown text"
    );
}

// T284: FR-031/FR-032 — rollback button with confirmation
#[test]
fn rollback_button_present_with_confirmation() {
    let source = get_dashboard_source();
    assert!(
        source.contains("rollback_available"),
        "Dashboard should check rollback_available to show rollback button"
    );
    assert!(
        source.contains("Rollback to previous version"),
        "Dashboard should show 'Rollback to previous version' button"
    );
}

#[test]
fn rollback_button_has_confirmation_dialog() {
    let source = get_dashboard_source();
    assert!(
        source.contains("rollback") && source.contains("AlertDialog"),
        "Rollback action should have a confirmation dialog"
    );
}

// T285: FR-038 — update settings modal exists
#[test]
fn update_settings_modal_exists() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Settings") || source.contains("settings"),
        "Dashboard should have an update settings button"
    );
}

#[test]
fn update_settings_modal_has_schedule_tab() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Schedule"),
        "Update settings modal should have a Schedule tab"
    );
}

#[test]
fn update_settings_modal_has_history_tab() {
    let source = get_dashboard_source();
    assert!(
        source.contains("History"),
        "Update settings modal should have a History tab"
    );
}

// T290: Type definitions include Phase 2 fields
#[test]
fn system_ts_has_downloading_progress_fields() {
    let source = get_system_ts_source();
    assert!(
        source.contains("downloaded_bytes"),
        "system.ts should define downloaded_bytes for downloading state"
    );
    assert!(
        source.contains("total_bytes"),
        "system.ts should define total_bytes for downloading state"
    );
}

#[test]
fn system_ts_has_drain_timeout_at() {
    let source = get_system_ts_source();
    assert!(
        source.contains("timeout_at"),
        "system.ts should define timeout_at for draining state"
    );
}

#[test]
fn system_ts_has_applying_phase_fields() {
    let source = get_system_ts_source();
    assert!(
        source.contains("running_installer"),
        "system.ts should include applying phase values"
    );
    assert!(
        source.contains("phase_message"),
        "system.ts should define phase_message for applying state"
    );
}

#[test]
fn system_ts_has_schedule_info() {
    let source = get_system_ts_source();
    assert!(
        source.contains("ScheduleInfo"),
        "system.ts should define ScheduleInfo type"
    );
    assert!(
        source.contains("schedule"),
        "system.ts should include schedule in SystemInfo"
    );
}

#[test]
fn system_ts_has_rollback_available() {
    let source = get_system_ts_source();
    assert!(
        source.contains("rollback_available"),
        "system.ts should define rollback_available in SystemInfo"
    );
}

#[test]
fn system_ts_has_rollback_api() {
    let source = get_system_ts_source();
    assert!(
        source.contains("rollback"),
        "system.ts should provide a rollback API method"
    );
}

#[test]
fn system_ts_has_schedule_api_methods() {
    let source = get_system_ts_source();
    assert!(
        source.contains("createSchedule") || source.contains("schedule"),
        "system.ts should provide schedule API methods"
    );
}

// FR-036: UI throttling for manual check (30-second cooldown)
#[test]
fn manual_check_has_ui_throttling() {
    let source = get_dashboard_source();
    assert!(
        source.contains("lastCheck") || source.contains("cooldown") || source.contains("throttle"),
        "Dashboard should implement UI throttling for manual check button"
    );
}

// FR-039: Schedule state displayed in update banner
#[test]
fn schedule_state_displayed_in_banner() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Scheduled") || source.contains("scheduled"),
        "Dashboard should display schedule state in the update banner"
    );
}
