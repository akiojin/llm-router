// Regression tests for dashboard update banner behavior (FR-006/FR-007/FR-010)
//
// This verifies source-level guarantees for the update banner:
// - The "Check for updates" CTA remains present.
// - The banner no longer disappears when update state is temporarily unavailable.

fn get_dashboard_source() -> String {
    include_str!("../../src/web/dashboard/src/pages/Dashboard.tsx").to_string()
}

#[test]
fn check_for_updates_button_is_present() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Check for updates"),
        "Dashboard should expose the manual update-check CTA"
    );
}

#[test]
fn update_banner_does_not_early_return_when_update_missing() {
    let source = get_dashboard_source();
    assert!(
        !source.contains("if (!update) return null"),
        "Update banner should stay visible even when update state is temporarily unavailable"
    );
    assert!(
        source.contains("Update status unavailable"),
        "Dashboard should show a fallback description while update state is unavailable"
    );
}

#[test]
fn restart_button_visibility_depends_on_update_state() {
    let source = get_dashboard_source();
    assert!(
        source.contains("updateState === 'failed' && Boolean(update?.latest)"),
        "Failed state should expose restart only when an actionable update candidate exists"
    );
    assert!(
        source.contains("updateState === 'available' || failedHasUpdateCandidate || applying"),
        "Restart button visibility should be tied to actionable update states"
    );
}

// FR-011: draining状態でボタンテキストが「Waiting to update...」に変化すること
#[test]
fn draining_state_shows_waiting_to_update_button_text() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Waiting to update..."),
        "Dashboard should show 'Waiting to update...' button text during draining state"
    );
}

// FR-011: draining状態でin_flight数がボタンに反映されること
#[test]
fn draining_state_shows_in_flight_count_in_button() {
    let source = get_dashboard_source();
    // The button text must include in_flight from the update state
    assert!(
        source.contains("update.in_flight"),
        "Dashboard button should reflect in_flight count during draining state"
    );
}

// FR-012: applying状態でボタンテキストが「Applying update...」に変化すること
#[test]
fn applying_state_shows_applying_update_button_text() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Applying update..."),
        "Dashboard should show 'Applying update...' button text during applying state"
    );
}

// FR-013: draining状態でもCheck for updatesがdisabledになること
#[test]
fn check_for_updates_disabled_during_draining() {
    let source = get_dashboard_source();
    // `applying` variable already covers draining state in the code:
    // const applying = updateState === 'draining' || updateState === 'applying'
    // canCheck = isAdmin && !applying → disabled during both draining and applying
    assert!(
        source
            .contains("const applying = updateState === 'draining' || updateState === 'applying'"),
        "applying variable should cover both draining and applying states"
    );
    assert!(
        source.contains("!applying"),
        "Check for updates should be disabled when applying (includes draining)"
    );
}

// FR-016: 強制更新ボタンが表示されること
#[test]
fn force_update_button_is_present() {
    let source = get_dashboard_source();
    assert!(
        source.contains("Force update now"),
        "Dashboard should expose a dedicated force update button"
    );
}

// FR-019: 強制更新ボタンはpayload ready時のみ有効になること
#[test]
fn force_update_requires_ready_payload() {
    let source = get_dashboard_source();
    assert!(
        source.contains("update?.payload?.payload === 'ready'"),
        "Force update should require payload=ready state"
    );
    assert!(
        source.contains("Update payload is still preparing"),
        "Dashboard should explain why force update is disabled when payload is not ready"
    );
}

// FR-018: 強制更新ボタンはアップデートが利用可能な場合のみ表示する
#[test]
fn force_update_button_shown_only_when_update_available() {
    let source = get_dashboard_source();
    assert!(
        source.contains("const showForceButton = hasAvailableUpdate"),
        "Force update button should only be visible when an update is available"
    );
}

// FR-031: ロールバックボタンは.bakが存在する場合のみ表示する
#[test]
fn rollback_button_shown_only_when_available() {
    let source = get_dashboard_source();
    assert!(
        source.contains("isAdmin && rollbackAvailable && ("),
        "Rollback button should only be visible when rollback is available"
    );
}

// FR-014: queued=false時に即時適用分岐を持つこと
#[test]
fn apply_update_handles_non_queued_response() {
    let source = get_dashboard_source();
    assert!(
        source.contains("if (result.queued)"),
        "Dashboard should branch on applyUpdate queued response"
    );
    assert!(
        source.contains("Applying update"),
        "Dashboard should announce immediate apply when queued=false"
    );
}
