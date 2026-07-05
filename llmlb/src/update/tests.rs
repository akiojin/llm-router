use super::*;

fn available_state_with_payload(payload: PayloadState) -> UpdateState {
    UpdateState::Available {
        current: "4.5.0".to_string(),
        latest: "4.5.1".to_string(),
        release_url: "https://example.com/release".to_string(),
        portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
        installer_asset_url: None,
        payload,
        checked_at: Utc::now(),
    }
}

#[test]
fn test_platform_asset_names() {
    let p = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(
        p.portable_asset_name(),
        Some("llmlb-linux-x86_64.tar.gz".to_string())
    );
    assert_eq!(p.installer_asset_name(), None);

    let p = Platform {
        os: "windows".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(
        p.portable_asset_name(),
        Some("llmlb-windows-x86_64.zip".to_string())
    );
    assert_eq!(
        p.installer_asset_name(),
        Some((
            "llmlb-windows-x86_64-setup.exe".to_string(),
            InstallerKind::WindowsSetup
        ))
    );
}

#[test]
fn test_parse_tag_to_version() {
    assert_eq!(
        parse_tag_to_version("v3.1.0").unwrap(),
        Version::parse("3.1.0").unwrap()
    );
    assert_eq!(
        parse_tag_to_version("3.1.0").unwrap(),
        Version::parse("3.1.0").unwrap()
    );
}

#[tokio::test]
async fn record_check_failure_preserves_available_payload() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    let ready_payload = PayloadState::Ready {
        kind: PayloadKind::Portable {
            binary_path: "/tmp/llmlb-new".to_string(),
        },
    };

    {
        *manager.inner.state.write().await = available_state_with_payload(ready_payload.clone());
    }

    manager
        .record_check_failure("temporary network outage".to_string())
        .await;

    match manager.state().await {
        UpdateState::Available {
            latest, payload, ..
        } => {
            assert_eq!(latest, "4.5.1");
            assert_eq!(payload, ready_payload);
        }
        other => panic!("expected available state, got {other:?}"),
    }
}

#[tokio::test]
async fn record_check_failure_transitions_non_available_to_failed() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    {
        *manager.inner.state.write().await = UpdateState::UpToDate { checked_at: None };
    }

    manager
        .record_check_failure("check failed".to_string())
        .await;

    match manager.state().await {
        UpdateState::Failed {
            latest,
            release_url,
            message,
            ..
        } => {
            assert_eq!(latest, None);
            assert_eq!(release_url, None);
            assert_eq!(message, "check failed");
        }
        other => panic!("expected failed state, got {other:?}"),
    }
}

#[tokio::test]
async fn request_apply_normal_reports_not_queued_when_ready_and_idle() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        });
    }

    let queued = manager.request_apply_normal().await;
    assert!(!queued);
    assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Normal);
}

#[tokio::test]
async fn request_apply_normal_reports_queued_when_payload_not_ready() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    let queued = manager.request_apply_normal().await;
    assert!(queued);
    assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Normal);
}

#[tokio::test]
async fn request_apply_force_requires_ready_payload() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    let err = manager
        .request_apply_force()
        .await
        .expect_err("force apply should fail when payload is not ready");
    assert!(err.to_string().contains("not ready"));
}

#[tokio::test]
async fn request_apply_force_promotes_pending_normal_request() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        });
    }

    manager.request_apply();
    let dropped = manager
        .request_apply_force()
        .await
        .expect("force apply request should be accepted");
    assert_eq!(dropped, 0);
    assert_eq!(manager.take_apply_request_mode(), ApplyRequestMode::Force);
}

#[test]
fn applying_state_serializes_phase_metadata() {
    let state = UpdateState::Applying {
        latest: "4.5.1".to_string(),
        method: ApplyMethod::WindowsSetup,
        phase: ApplyPhase::RunningInstaller,
        phase_message: "Installer is running".to_string(),
        started_at: Utc::now(),
        timeout_at: None,
    };

    let json = serde_json::to_value(state).expect("serialize applying state");
    assert_eq!(json["state"], "applying");
    assert_eq!(json["phase"], "running_installer");
    assert!(json.get("phase_message").is_some());
    assert!(json.get("started_at").is_some());
    assert!(json.get("timeout_at").is_none());
}

// =======================================================================
// T210: check_only — GitHub APIチェックのみ同期、DLは行わない
// =======================================================================
#[tokio::test]
async fn check_only_does_not_download_payload() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v99.0.0",
                "html_url": "https://github.com/test-owner/test-repo/releases/tag/v99.0.0",
                "assets": [{
                    "name": format!("llmlb-{}.tar.gz", Platform::detect().unwrap().artifact().unwrap_or("linux-x86_64")),
                    "browser_download_url": format!("{}/download/portable.tar.gz", mock_server.uri()),
                }]
            })))
            .mount(&mock_server)
            .await;

    let manager = UpdateManager::new_with_config(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
        "test-owner".to_string(),
        "test-repo".to_string(),
        Some(mock_server.uri()),
    )
    .expect("create update manager");

    let state = manager.check_only(true).await.expect("check_only");

    // Should discover the update.
    match &state {
        UpdateState::Available {
            latest, payload, ..
        } => {
            assert_eq!(latest, "99.0.0");
            // check_only must NOT start downloading.
            assert_eq!(*payload, PayloadState::NotReady);
        }
        other => panic!("expected available, got {other:?}"),
    }
}

// =======================================================================
// T211: download_background — バックグラウンドDL開始、進捗更新
// =======================================================================
#[tokio::test]
async fn download_background_transitions_to_downloading() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // Serve a tiny payload so download completes.
    Mock::given(method("GET"))
        .and(path("/download/portable.tar.gz"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0u8; 100])
                .insert_header("content-length", "100"),
        )
        .mount(&mock_server)
        .await;

    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    // Pre-seed available state with a portable asset URL pointing to mock.
    {
        let mut st = manager.inner.state.write().await;
        *st = UpdateState::Available {
            current: "4.5.0".to_string(),
            latest: "4.5.1".to_string(),
            release_url: "https://example.com/release".to_string(),
            portable_asset_url: Some(format!("{}/download/portable.tar.gz", mock_server.uri())),
            installer_asset_url: None,
            payload: PayloadState::NotReady,
            checked_at: Utc::now(),
        };
    }

    // Start background download.
    manager.download_background();

    // Give some time for async task to start and update state.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let state = manager.state().await;
    match &state {
        UpdateState::Available { payload, .. } => {
            // Should be Downloading or Ready (if completed quickly).
            assert!(
                matches!(
                    payload,
                    PayloadState::Downloading { .. } | PayloadState::Ready { .. }
                ),
                "expected Downloading or Ready, got {payload:?}"
            );
        }
        other => panic!("expected available, got {other:?}"),
    }
}

// =======================================================================
// T212: レートリミット判定
// =======================================================================
#[tokio::test]
async fn rate_limit_rejects_within_60_seconds() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    // First call should succeed (not rate-limited).
    assert!(
        !manager.is_manual_check_rate_limited(),
        "first call should not be rate-limited"
    );
    manager.record_manual_check();

    // Immediate second call should be rate-limited.
    assert!(
        manager.is_manual_check_rate_limited(),
        "second call within 60s should be rate-limited"
    );
}

#[tokio::test]
async fn rate_limit_allows_after_cooldown() {
    use tokio::time;

    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    manager.record_manual_check();
    assert!(manager.is_manual_check_rate_limited());

    // Advance time past 60 seconds.
    time::pause();
    time::advance(Duration::from_secs(61)).await;

    assert!(
        !manager.is_manual_check_rate_limited(),
        "should allow check after 60s cooldown"
    );
}

// =======================================================================
// T250: ドレインタイムアウト — タイムアウト超過でキャンセル＋ゲート再開＋failed遷移
// =======================================================================
#[tokio::test]
async fn drain_timeout_cancels_and_transitions_to_failed() {
    use tokio::time;

    time::pause();

    let gate = InferenceGate::default();
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        gate.clone(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    // Set up available state with ready payload.
    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        });
    }

    // Simulate an in-flight request that never completes.
    let _guard = gate.begin_for_test();

    // Start apply_flow in a task — it will try to drain.
    let mgr = manager.clone();
    let apply_task = tokio::spawn(async move { mgr.apply_flow(ApplyRequestMode::Normal).await });

    // Let the drain start.
    time::advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;

    // Verify we're in Draining state.
    let state = manager.state().await;
    assert!(
        matches!(state, UpdateState::Draining { .. }),
        "expected draining, got {state:?}"
    );

    // Advance time past the drain timeout (300s).
    time::advance(Duration::from_secs(301)).await;
    tokio::task::yield_now().await;

    // apply_flow should return an error.
    let result = apply_task.await.expect("task should complete");
    assert!(result.is_err(), "apply_flow should fail on drain timeout");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("timed out"),
        "error should mention timeout: {err_msg}"
    );

    // State should be Failed.
    let state = manager.state().await;
    match &state {
        UpdateState::Failed { message, .. } => {
            assert!(
                message.contains("timed out"),
                "failed message should mention timeout: {message}"
            );
        }
        other => panic!("expected failed state, got {other:?}"),
    }

    // Gate should no longer be rejecting.
    assert!(
        !gate.is_rejecting(),
        "gate should stop rejecting after drain timeout"
    );
}

// T250 supplemental: drain that completes before timeout succeeds.
#[tokio::test]
async fn drain_completes_before_timeout() {
    use tokio::time;

    time::pause();

    let gate = InferenceGate::default();
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        gate.clone(),
        ShutdownController::default(),
    )
    .expect("create update manager");

    // Set up available state with ready payload.
    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::Ready {
            kind: PayloadKind::Portable {
                binary_path: "/tmp/llmlb-new".to_string(),
            },
        });
    }

    // Simulate an in-flight request.
    let guard = gate.begin_for_test();

    let mgr = manager.clone();
    let apply_task = tokio::spawn(async move { mgr.apply_flow(ApplyRequestMode::Normal).await });

    // Let drain start.
    time::advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;

    // Complete the request before timeout.
    drop(guard);
    time::advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;

    // apply_flow will fail because it tries to spawn a real binary,
    // but it should NOT fail due to timeout.
    let result = apply_task.await.expect("task should complete");
    // The error (if any) should be about spawning, not timeout.
    if let Err(e) = &result {
        assert!(
            !e.to_string().contains("timed out"),
            "should not time out: {e}"
        );
    }

    // State should NOT be Failed due to timeout.
    let state = manager.state().await;
    assert!(
        !matches!(
            &state,
            UpdateState::Failed { message, .. } if message.contains("timed out")
        ),
        "should not be in timeout-failed state: {state:?}"
    );
}

/// Helper to create an UpdateManager with an isolated temp data dir for testing.
///
/// Uses a unique env var approach with per-test isolation.
fn test_manager_with_gate(gate: InferenceGate) -> (UpdateManager, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("create temp dir");
    std::fs::create_dir_all(tmp.path()).expect("create data dir");
    let manager = UpdateManager::new_with_data_dir(
        reqwest::Client::new(),
        gate,
        ShutdownController::default(),
        tmp.path(),
    )
    .expect("create update manager");
    (manager, tmp)
}

// =======================================================================
// T232: アイドル時適用トリガー — in_flight=0でスケジュール起動
// =======================================================================
#[tokio::test]
async fn idle_schedule_triggers_when_in_flight_zero() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate.clone());

    // Set up available state.
    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    // Create idle schedule.
    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Idle,
        scheduled_at: None,
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager
        .create_schedule(sched)
        .expect("schedule should be created");

    // No in-flight requests → in_flight == 0.
    assert_eq!(gate.in_flight(), 0);

    // Start schedule loop.
    manager.start_schedule_loop();

    // Give the loop time to detect idle and trigger.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Schedule should have been removed (triggered).
    assert!(
        manager.get_schedule().unwrap().is_none(),
        "schedule should be consumed after idle trigger"
    );

    // Apply request should have been triggered.
    let mode = manager.take_apply_request_mode();
    assert_eq!(
        mode,
        ApplyRequestMode::Normal,
        "idle schedule should trigger normal apply"
    );
}

#[tokio::test]
async fn idle_schedule_does_not_trigger_while_busy() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate.clone());

    // Set up available state.
    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    // Simulate in-flight request.
    let _guard = gate.begin_for_test();

    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Idle,
        scheduled_at: None,
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager
        .create_schedule(sched)
        .expect("schedule should be created");

    manager.start_schedule_loop();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Schedule should still exist (not triggered).
    assert!(
        manager.get_schedule().unwrap().is_some(),
        "schedule should remain while requests are in-flight"
    );

    // No apply request should be pending.
    let mode = manager.take_apply_request_mode();
    assert_eq!(
        mode,
        ApplyRequestMode::None,
        "should not trigger while busy"
    );
}

#[test]
fn scheduled_mode_requires_scheduled_at() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Scheduled,
        scheduled_at: None,
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };

    let err = manager
        .create_schedule(sched)
        .expect_err("scheduled mode without scheduled_at must be rejected");
    assert!(
        err.to_string()
            .contains("scheduled_at is required when mode is scheduled"),
        "unexpected error: {err}"
    );
}

// =======================================================================
// T233: 時刻指定適用トリガー — 指定時刻到達でドレイン開始
// =======================================================================
#[tokio::test]
async fn scheduled_time_triggers_when_past_due() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    // Schedule for 1 second ago (already past due).
    let scheduled_at = Utc::now() - chrono::Duration::seconds(1);
    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Scheduled,
        scheduled_at: Some(scheduled_at),
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager
        .create_schedule(sched)
        .expect("schedule should be created");

    manager.start_schedule_loop();

    // Give the loop time to detect and trigger.
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        manager.get_schedule().unwrap().is_none(),
        "schedule should be consumed after scheduled_at"
    );

    let mode = manager.take_apply_request_mode();
    assert_eq!(
        mode,
        ApplyRequestMode::Normal,
        "scheduled trigger should request normal apply"
    );
}

#[tokio::test]
async fn scheduled_time_does_not_trigger_when_target_version_mismatch() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    {
        let mut state = available_state_with_payload(PayloadState::NotReady);
        if let UpdateState::Available { latest, .. } = &mut state {
            *latest = "4.5.2".to_string();
        }
        *manager.inner.state.write().await = state;
    }

    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Scheduled,
        scheduled_at: Some(Utc::now() - chrono::Duration::seconds(1)),
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager
        .create_schedule(sched)
        .expect("schedule should be created");

    manager.start_schedule_loop();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        manager.get_schedule().unwrap().is_some(),
        "schedule should remain when target version no longer matches latest"
    );
    assert_eq!(
        manager.take_apply_request_mode(),
        ApplyRequestMode::None,
        "target version mismatch must not trigger apply"
    );
}

#[tokio::test]
async fn malformed_scheduled_without_time_does_not_trigger() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    // Simulate malformed persisted data from an older version.
    let malformed = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Scheduled,
        scheduled_at: None,
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager.inner.schedule_store.save(&malformed).unwrap();

    manager.start_schedule_loop();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        manager.get_schedule().unwrap().is_some(),
        "malformed scheduled entry should not be consumed automatically"
    );
    assert_eq!(
        manager.take_apply_request_mode(),
        ApplyRequestMode::None,
        "malformed scheduled entry must never trigger apply"
    );
}

// =======================================================================
// T260: ヘルパー起動監視 — .bakから復元ロジックのテスト
// =======================================================================
#[test]
fn internal_rollback_restores_backup() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("llmlb");
    let backup = dir.path().join("llmlb.bak");
    let args_file = dir.path().join("restart_args.json");

    // Simulate a freshly updated target that rollback must replace.
    fs::write(&target, b"new-binary-content").unwrap();
    // Create a fake "old" binary.
    fs::write(&backup, b"old-binary-content").unwrap();
    // Create a fake args file (needed for restart_from_args_file).
    let args = RestartArgsFile {
        args: vec![],
        cwd: dir.path().to_string_lossy().to_string(),
    };
    fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();

    // internal_rollback expects the old process to have exited.
    // Using PID 0 or a non-existent PID: use current PID which is alive.
    // Instead, use PID 1 which is always running on Unix — let's use a non-existent PID.
    // PID u32::MAX is unlikely to exist.
    let result = internal_rollback(u32::MAX, target.clone(), backup.clone(), args_file);

    // The rollback should have restored the backup to the target path.
    assert!(target.exists(), "target should be restored from backup");
    assert!(!backup.exists(), "backup should be consumed (renamed)");
    let content = fs::read(&target).unwrap();
    assert_eq!(content, b"old-binary-content");

    // The restart_from_args_file call will fail because the target is not
    // executable, but the backup restoration should have succeeded.
    // We check if the result is Err (from failed spawn) but not from rollback.
    if let Err(e) = result {
        // Expected: spawn failure because we wrote fake content, not a real binary.
        assert!(
            !e.to_string().contains("Backup file does not exist"),
            "should not fail due to missing backup: {e}"
        );
    }
}

#[test]
fn internal_rollback_fails_without_backup() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("llmlb");
    let backup = dir.path().join("llmlb.bak");
    let args_file = dir.path().join("restart_args.json");

    let result = internal_rollback(u32::MAX, target, backup, args_file);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Backup file does not exist"));
}

// =======================================================================
// T262: ロールバック結果の update-history.json 記録
// =======================================================================
#[test]
fn record_auto_rollback_history_writes_entry() {
    let dir = tempfile::tempdir().unwrap();
    // Create directory structure: data_dir/updates/rollback-X.Y.Z/restart_args.json
    let updates_dir = dir.path().join("updates").join("rollback-test");
    fs::create_dir_all(&updates_dir).unwrap();
    let args_file = updates_dir.join("restart_args.json");
    fs::write(&args_file, "{}").unwrap();

    super::record_auto_rollback_history(&args_file, "health check failed");

    let store = history::HistoryStore::new(dir.path());
    let entries = store.load().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, history::HistoryEventKind::Rollback);
    assert!(entries[0]
        .message
        .as_ref()
        .unwrap()
        .contains("health check failed"));
}

#[test]
fn detect_server_port_reads_restart_args_file() {
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("restart_args.json");
    let args = RestartArgsFile {
        args: vec![
            "serve".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            "40123".to_string(),
        ],
        cwd: dir.path().to_string_lossy().to_string(),
    };
    fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();

    assert_eq!(detect_server_port(&args_file), 40123);
}

#[test]
fn parse_port_from_args_supports_equals_style() {
    let args = vec!["serve".to_string(), "--port=40124".to_string()];
    assert_eq!(parse_port_from_args(&args), Some(40124));
}

#[tokio::test]
async fn scheduled_time_does_not_trigger_before_time() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    // Schedule for 60 seconds from now (far future, won't trigger in test).
    let scheduled_at = Utc::now() + chrono::Duration::seconds(60);
    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Scheduled,
        scheduled_at: Some(scheduled_at),
        scheduled_by: "admin".to_string(),
        target_version: "4.5.1".to_string(),
        created_at: Utc::now(),
    };
    manager
        .create_schedule(sched)
        .expect("schedule should be created");

    manager.start_schedule_loop();

    // Wait a bit — should NOT trigger (still 60s away).
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        manager.get_schedule().unwrap().is_some(),
        "schedule should not trigger before scheduled_at"
    );
    assert_eq!(
        manager.take_apply_request_mode(),
        ApplyRequestMode::None,
        "should not trigger before time"
    );
}

// =======================================================================
// check_only: GitHub API失敗時にキャッシュフォールバック
// (SPEC-a6e55b37 ユーザーストーリー10シナリオ4)
// =======================================================================
// =======================================================================
// parse_tag_to_version: edge cases
// =======================================================================
#[test]
fn parse_tag_to_version_with_v_prefix() {
    let v = parse_tag_to_version("v1.2.3").unwrap();
    assert_eq!(v, Version::new(1, 2, 3));
}

#[test]
fn parse_tag_to_version_without_prefix() {
    let v = parse_tag_to_version("1.2.3").unwrap();
    assert_eq!(v, Version::new(1, 2, 3));
}

#[test]
fn parse_tag_to_version_prerelease() {
    let v = parse_tag_to_version("v2.0.0-beta.1").unwrap();
    assert_eq!(v.major, 2);
    assert!(!v.pre.is_empty());
}

#[test]
fn parse_tag_to_version_invalid() {
    assert!(parse_tag_to_version("not-a-version").is_err());
}

#[test]
fn parse_tag_to_version_empty() {
    assert!(parse_tag_to_version("").is_err());
}

#[test]
fn parse_tag_to_version_v_only() {
    assert!(parse_tag_to_version("v").is_err());
}

#[test]
fn parse_tag_to_version_partial() {
    // semver requires major.minor.patch
    assert!(parse_tag_to_version("v1.2").is_err());
}

// =======================================================================
// Platform tests
// =======================================================================
#[test]
fn platform_detect_returns_current_os() {
    let p = Platform::detect().unwrap();
    assert_eq!(p.os, std::env::consts::OS);
    assert_eq!(p.arch, std::env::consts::ARCH);
}

#[test]
fn platform_artifact_linux_x86_64() {
    let p = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(p.artifact(), Some("linux-x86_64"));
}

#[test]
fn platform_artifact_linux_arm64() {
    let p = Platform {
        os: "linux".to_string(),
        arch: "aarch64".to_string(),
    };
    assert_eq!(p.artifact(), Some("linux-arm64"));
}

#[test]
fn platform_artifact_macos_arm64() {
    let p = Platform {
        os: "macos".to_string(),
        arch: "aarch64".to_string(),
    };
    assert_eq!(p.artifact(), Some("macos-arm64"));
    assert_eq!(
        p.portable_asset_name(),
        Some("llmlb-macos-arm64.tar.gz".to_string())
    );
    assert_eq!(
        p.installer_asset_name(),
        Some(("llmlb-macos-arm64.pkg".to_string(), InstallerKind::MacPkg))
    );
}

#[test]
fn platform_artifact_macos_x86_64() {
    let p = Platform {
        os: "macos".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(p.artifact(), Some("macos-x86_64"));
    assert_eq!(
        p.installer_asset_name(),
        Some(("llmlb-macos-x86_64.pkg".to_string(), InstallerKind::MacPkg))
    );
}

#[test]
fn platform_artifact_unknown() {
    let p = Platform {
        os: "freebsd".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(p.artifact(), None);
    assert_eq!(p.portable_asset_name(), None);
    assert_eq!(p.installer_asset_name(), None);
}

#[test]
fn platform_binary_name_unix() {
    let p = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(p.binary_name(), "llmlb");
}

#[test]
fn platform_binary_name_windows() {
    let p = Platform {
        os: "windows".to_string(),
        arch: "x86_64".to_string(),
    };
    assert_eq!(p.binary_name(), "llmlb.exe");
}

// =======================================================================
// asset_name_from_url
// =======================================================================
#[test]
fn asset_name_from_url_extracts_filename() {
    assert_eq!(
        asset_name_from_url("https://example.com/downloads/llmlb-linux-x86_64.tar.gz"),
        Some("llmlb-linux-x86_64.tar.gz".to_string())
    );
}

#[test]
fn asset_name_from_url_single_segment() {
    assert_eq!(
        asset_name_from_url("llmlb.tar.gz"),
        Some("llmlb.tar.gz".to_string())
    );
}

#[test]
fn asset_name_from_url_empty() {
    assert_eq!(asset_name_from_url(""), Some("".to_string()));
}

// =======================================================================
// select_assets
// =======================================================================
#[test]
fn select_assets_finds_matching_portable() {
    let release = GitHubRelease {
        tag_name: "v5.0.0".to_string(),
        html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
        assets: vec![
            GitHubAsset {
                name: "llmlb-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://dl.example.com/llmlb-linux-x86_64.tar.gz"
                    .to_string(),
            },
            GitHubAsset {
                name: "llmlb-windows-x86_64.zip".to_string(),
                browser_download_url: "https://dl.example.com/llmlb-windows-x86_64.zip".to_string(),
            },
        ],
    };

    let platform = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    let (portable, installer) = select_assets(&release, &platform);
    assert!(portable.is_some());
    assert_eq!(portable.unwrap().name, "llmlb-linux-x86_64.tar.gz");
    assert!(installer.is_none()); // linux has no installer
}

#[test]
fn select_assets_finds_both_on_windows() {
    let release = GitHubRelease {
        tag_name: "v5.0.0".to_string(),
        html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
        assets: vec![
            GitHubAsset {
                name: "llmlb-windows-x86_64.zip".to_string(),
                browser_download_url: "https://dl.example.com/llmlb-windows-x86_64.zip".to_string(),
            },
            GitHubAsset {
                name: "llmlb-windows-x86_64-setup.exe".to_string(),
                browser_download_url: "https://dl.example.com/llmlb-windows-x86_64-setup.exe"
                    .to_string(),
            },
        ],
    };

    let platform = Platform {
        os: "windows".to_string(),
        arch: "x86_64".to_string(),
    };
    let (portable, installer) = select_assets(&release, &platform);
    assert!(portable.is_some());
    assert!(installer.is_some());
    assert_eq!(portable.unwrap().name, "llmlb-windows-x86_64.zip");
    assert_eq!(installer.unwrap().name, "llmlb-windows-x86_64-setup.exe");
}

#[test]
fn select_assets_returns_none_when_no_match() {
    let release = GitHubRelease {
        tag_name: "v5.0.0".to_string(),
        html_url: "https://github.com/test/test/releases/v5.0.0".to_string(),
        assets: vec![GitHubAsset {
            name: "llmlb-linux-x86_64.tar.gz".to_string(),
            browser_download_url: "https://dl.example.com/llmlb-linux-x86_64.tar.gz".to_string(),
        }],
    };

    let platform = Platform {
        os: "freebsd".to_string(),
        arch: "x86_64".to_string(),
    };
    let (portable, installer) = select_assets(&release, &platform);
    assert!(portable.is_none());
    assert!(installer.is_none());
}

// =======================================================================
// choose_apply_plan
// =======================================================================
#[test]
fn choose_apply_plan_prefers_portable_when_writable() {
    let dir = tempfile::tempdir().unwrap();
    let exe_path = dir.path().join("llmlb");
    fs::write(&exe_path, b"dummy").unwrap();

    let platform = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    let plan = choose_apply_plan(
        &platform,
        &exe_path,
        Some("https://example.com/portable.tar.gz"),
        None,
    );
    assert_eq!(
        plan,
        Some(ApplyPlan::Portable {
            url: "https://example.com/portable.tar.gz".to_string()
        })
    );
}

#[test]
fn choose_apply_plan_returns_none_when_no_urls() {
    let dir = tempfile::tempdir().unwrap();
    let exe_path = dir.path().join("llmlb");
    fs::write(&exe_path, b"dummy").unwrap();

    let platform = Platform {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
    };
    let plan = choose_apply_plan(&platform, &exe_path, None, None);
    assert!(plan.is_none());
}

#[test]
fn choose_apply_plan_falls_back_to_installer_when_writable() {
    let dir = tempfile::tempdir().unwrap();
    let exe_path = dir.path().join("llmlb");
    fs::write(&exe_path, b"dummy").unwrap();

    let platform = Platform {
        os: "macos".to_string(),
        arch: "aarch64".to_string(),
    };
    let plan = choose_apply_plan(
        &platform,
        &exe_path,
        None,
        Some("https://example.com/installer.pkg"),
    );
    assert!(matches!(plan, Some(ApplyPlan::Installer { .. })));
}

// =======================================================================
// is_dir_writable
// =======================================================================
#[test]
fn is_dir_writable_temp_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(is_dir_writable(dir.path()).unwrap());
}

// =======================================================================
// ApplyRequestMode
// =======================================================================
#[test]
fn apply_request_mode_from_u8_round_trip() {
    assert_eq!(ApplyRequestMode::from_u8(0), ApplyRequestMode::None);
    assert_eq!(ApplyRequestMode::from_u8(1), ApplyRequestMode::Normal);
    assert_eq!(ApplyRequestMode::from_u8(2), ApplyRequestMode::Force);
}

#[test]
fn apply_request_mode_from_u8_unknown_defaults_to_none() {
    assert_eq!(ApplyRequestMode::from_u8(3), ApplyRequestMode::None);
    assert_eq!(ApplyRequestMode::from_u8(255), ApplyRequestMode::None);
}

#[test]
fn apply_request_mode_ordering() {
    assert!(ApplyRequestMode::None < ApplyRequestMode::Normal);
    assert!(ApplyRequestMode::Normal < ApplyRequestMode::Force);
}

// =======================================================================
// ApplyPhase::message
// =======================================================================
#[test]
fn apply_phase_messages_are_non_empty() {
    let phases = [
        ApplyPhase::Starting,
        ApplyPhase::WaitingOldProcessExit,
        ApplyPhase::RunningInstaller,
        ApplyPhase::Restarting,
    ];
    for phase in &phases {
        assert!(
            !phase.message().is_empty(),
            "phase {:?} has empty message",
            phase
        );
    }
}

#[test]
fn apply_phase_starting_message() {
    assert_eq!(ApplyPhase::Starting.message(), "Preparing update apply");
}

#[test]
fn apply_phase_restarting_message() {
    assert_eq!(ApplyPhase::Restarting.message(), "Restarting service");
}

// =======================================================================
// UpdateState serialization
// =======================================================================
#[test]
fn update_state_up_to_date_serialization() {
    let state = UpdateState::UpToDate {
        checked_at: Some(Utc::now()),
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "up_to_date");
    assert!(json.get("checked_at").is_some());
}

#[test]
fn update_state_up_to_date_none_checked_at() {
    let state = UpdateState::UpToDate { checked_at: None };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "up_to_date");
}

#[test]
fn update_state_available_serialization() {
    let state = UpdateState::Available {
        current: "5.0.0".to_string(),
        latest: "5.1.0".to_string(),
        release_url: "https://example.com/release".to_string(),
        portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
        installer_asset_url: None,
        payload: PayloadState::NotReady,
        checked_at: Utc::now(),
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "available");
    assert_eq!(json["current"], "5.0.0");
    assert_eq!(json["latest"], "5.1.0");
    // PayloadState is internally tagged, nested as {"payload": "not_ready"}
    assert_eq!(json["payload"]["payload"], "not_ready");
}

#[test]
fn update_state_draining_serialization() {
    let state = UpdateState::Draining {
        latest: "5.1.0".to_string(),
        in_flight: 5,
        requested_at: Utc::now(),
        timeout_at: Utc::now() + chrono::Duration::seconds(300),
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "draining");
    assert_eq!(json["in_flight"], 5);
}

#[test]
fn update_state_failed_serialization() {
    let state = UpdateState::Failed {
        latest: Some("5.1.0".to_string()),
        release_url: Some("https://example.com/release".to_string()),
        message: "download failed".to_string(),
        failed_at: Utc::now(),
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "failed");
    assert_eq!(json["message"], "download failed");
}

#[test]
fn update_state_failed_with_none_fields() {
    let state = UpdateState::Failed {
        latest: None,
        release_url: None,
        message: "unknown error".to_string(),
        failed_at: Utc::now(),
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["state"], "failed");
    assert!(json["latest"].is_null());
    assert!(json["release_url"].is_null());
}

// =======================================================================
// PayloadState serialization
// =======================================================================
#[test]
fn payload_state_not_ready_serialization() {
    let ps = PayloadState::NotReady;
    let json = serde_json::to_value(&ps).unwrap();
    assert_eq!(json["payload"], "not_ready");
}

#[test]
fn payload_state_downloading_serialization() {
    let ps = PayloadState::Downloading {
        started_at: Utc::now(),
        downloaded_bytes: Some(1024),
        total_bytes: Some(2048),
    };
    let json = serde_json::to_value(&ps).unwrap();
    assert_eq!(json["payload"], "downloading");
    assert_eq!(json["downloaded_bytes"], 1024);
    assert_eq!(json["total_bytes"], 2048);
}

#[test]
fn payload_state_downloading_skips_none_bytes() {
    let ps = PayloadState::Downloading {
        started_at: Utc::now(),
        downloaded_bytes: None,
        total_bytes: None,
    };
    let json = serde_json::to_value(&ps).unwrap();
    assert_eq!(json["payload"], "downloading");
    // skip_serializing_if = "Option::is_none" means no key at all
    assert!(json.get("downloaded_bytes").is_none());
    assert!(json.get("total_bytes").is_none());
}

#[test]
fn payload_state_ready_portable_serialization() {
    let ps = PayloadState::Ready {
        kind: PayloadKind::Portable {
            binary_path: "/tmp/llmlb-new".to_string(),
        },
    };
    let json = serde_json::to_value(&ps).unwrap();
    assert_eq!(json["payload"], "ready");
}

#[test]
fn payload_state_error_serialization() {
    let ps = PayloadState::Error {
        message: "download failed".to_string(),
    };
    let json = serde_json::to_value(&ps).unwrap();
    assert_eq!(json["payload"], "error");
    assert_eq!(json["message"], "download failed");
}

// =======================================================================
// PayloadKind serialization
// =======================================================================
#[test]
fn payload_kind_portable_serialization() {
    let kind = PayloadKind::Portable {
        binary_path: "/usr/local/bin/llmlb".to_string(),
    };
    let json = serde_json::to_value(&kind).unwrap();
    // Externally tagged: {"portable": {"binary_path": "..."}}
    assert_eq!(json["portable"]["binary_path"], "/usr/local/bin/llmlb");
}

#[test]
fn payload_kind_installer_serialization() {
    let kind = PayloadKind::Installer {
        installer_path: "/tmp/llmlb-setup.exe".to_string(),
        kind: InstallerKind::WindowsSetup,
    };
    let json = serde_json::to_value(&kind).unwrap();
    // Externally tagged: {"installer": {"installer_path": "...", "kind": "..."}}
    assert_eq!(json["installer"]["installer_path"], "/tmp/llmlb-setup.exe");
    assert_eq!(json["installer"]["kind"], "windows_setup");
}

// =======================================================================
// InstallerKind serialization
// =======================================================================
#[test]
fn installer_kind_serialization() {
    let mac = InstallerKind::MacPkg;
    let win = InstallerKind::WindowsSetup;
    assert_eq!(
        serde_json::to_value(&mac).unwrap(),
        serde_json::json!("mac_pkg")
    );
    assert_eq!(
        serde_json::to_value(&win).unwrap(),
        serde_json::json!("windows_setup")
    );
}

// =======================================================================
// ApplyMethod serialization
// =======================================================================
#[test]
fn apply_method_serialization() {
    assert_eq!(
        serde_json::to_value(&ApplyMethod::PortableReplace).unwrap(),
        serde_json::json!("portable_replace")
    );
    assert_eq!(
        serde_json::to_value(&ApplyMethod::MacPkg).unwrap(),
        serde_json::json!("mac_pkg")
    );
    assert_eq!(
        serde_json::to_value(&ApplyMethod::WindowsSetup).unwrap(),
        serde_json::json!("windows_setup")
    );
}

// =======================================================================
// RestartArgsFile serialization
// =======================================================================
#[test]
fn restart_args_file_roundtrip() {
    let raf = RestartArgsFile {
        args: vec![
            "serve".to_string(),
            "--port".to_string(),
            "8080".to_string(),
        ],
        cwd: "/home/user".to_string(),
    };
    let json = serde_json::to_string(&raf).unwrap();
    let deserialized: RestartArgsFile = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.args, raf.args);
    assert_eq!(deserialized.cwd, raf.cwd);
}

#[test]
fn restart_args_file_empty_args() {
    let raf = RestartArgsFile {
        args: vec![],
        cwd: ".".to_string(),
    };
    let json = serde_json::to_string(&raf).unwrap();
    let deserialized: RestartArgsFile = serde_json::from_str(&json).unwrap();
    assert!(deserialized.args.is_empty());
}

// =======================================================================
// write_restart_args_file
// =======================================================================
#[test]
fn write_restart_args_file_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let update_dir = dir.path().join("updates").join("5.0.0");
    let result = write_restart_args_file(&update_dir);
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap(), "restart_args.json");

    // Verify content is valid JSON
    let content = fs::read_to_string(&path).unwrap();
    let parsed: RestartArgsFile = serde_json::from_str(&content).unwrap();
    assert!(!parsed.cwd.is_empty());
}

// =======================================================================
// parse_port_from_args
// =======================================================================
#[test]
fn parse_port_from_args_flag_style() {
    let args = vec![
        "serve".to_string(),
        "--port".to_string(),
        "9090".to_string(),
    ];
    assert_eq!(parse_port_from_args(&args), Some(9090));
}

#[test]
fn parse_port_from_args_short_flag() {
    let args = vec!["serve".to_string(), "-p".to_string(), "9090".to_string()];
    assert_eq!(parse_port_from_args(&args), Some(9090));
}

#[test]
fn parse_port_from_args_equals_style() {
    let args = vec!["serve".to_string(), "--port=12345".to_string()];
    assert_eq!(parse_port_from_args(&args), Some(12345));
}

#[test]
fn parse_port_from_args_no_port() {
    let args = vec![
        "serve".to_string(),
        "--host".to_string(),
        "0.0.0.0".to_string(),
    ];
    assert_eq!(parse_port_from_args(&args), None);
}

#[test]
fn parse_port_from_args_empty() {
    let args: Vec<String> = vec![];
    assert_eq!(parse_port_from_args(&args), None);
}

#[test]
fn parse_port_from_args_invalid_port_value() {
    let args = vec![
        "serve".to_string(),
        "--port".to_string(),
        "not_a_number".to_string(),
    ];
    assert_eq!(parse_port_from_args(&args), None);
}

#[test]
fn parse_port_from_args_port_at_end_without_value() {
    let args = vec!["serve".to_string(), "--port".to_string()];
    assert_eq!(parse_port_from_args(&args), None);
}

// =======================================================================
// detect_server_port
// =======================================================================
#[test]
fn detect_server_port_falls_back_to_default() {
    let dir = tempfile::tempdir().unwrap();
    let nonexistent = dir.path().join("nonexistent.json");
    let port = detect_server_port(&nonexistent);
    assert_eq!(port, DEFAULT_LISTEN_PORT);
}

#[test]
fn detect_server_port_from_args_file_reads_port() {
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("restart_args.json");
    let args = RestartArgsFile {
        args: vec!["serve".to_string(), "-p".to_string(), "55555".to_string()],
        cwd: dir.path().to_string_lossy().to_string(),
    };
    fs::write(&args_file, serde_json::to_vec(&args).unwrap()).unwrap();
    assert_eq!(detect_server_port(&args_file), 55555);
}

// =======================================================================
// find_extracted_binary
// =======================================================================
#[test]
fn find_extracted_binary_at_root() {
    let dir = tempfile::tempdir().unwrap();
    let binary_path = dir.path().join("llmlb");
    fs::write(&binary_path, b"binary content").unwrap();

    let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), binary_path);
}

#[test]
fn find_extracted_binary_in_subdir() {
    let dir = tempfile::tempdir().unwrap();
    let sub_dir = dir.path().join("llmlb-linux-x86_64");
    fs::create_dir_all(&sub_dir).unwrap();
    let binary_path = sub_dir.join("llmlb");
    fs::write(&binary_path, b"binary content").unwrap();

    let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), binary_path);
}

#[test]
fn find_extracted_binary_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
    assert!(result.is_none());
}

#[test]
fn find_extracted_binary_deep_nested() {
    let dir = tempfile::tempdir().unwrap();
    let deep_dir = dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep_dir).unwrap();
    let binary_path = deep_dir.join("llmlb");
    fs::write(&binary_path, b"binary content").unwrap();

    let result = find_extracted_binary(dir.path(), "llmlb").unwrap();
    assert!(result.is_some());
}

// =======================================================================
// extract_archive: unsupported format
// =======================================================================
#[test]
fn extract_archive_unsupported_format_fails() {
    let dir = tempfile::tempdir().unwrap();
    let archive_path = dir.path().join("archive.7z");
    fs::write(&archive_path, b"some content").unwrap();
    let dest = dir.path().join("extract");
    fs::create_dir_all(&dest).unwrap();

    let result = extract_archive(&archive_path, &dest);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("unsupported archive format"));
}

// =======================================================================
// UpdateManager: state transitions
// =======================================================================
#[tokio::test]
async fn update_manager_initial_state_is_up_to_date() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    match manager.state().await {
        UpdateState::UpToDate { checked_at } => {
            assert!(checked_at.is_none());
        }
        other => panic!("expected up_to_date, got {other:?}"),
    }
}

#[tokio::test]
async fn set_applying_state_updates_correctly() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let started = Utc::now();
    manager
        .set_applying_state(
            "5.0.0",
            ApplyMethod::PortableReplace,
            ApplyPhase::Starting,
            started,
            None,
        )
        .await;

    match manager.state().await {
        UpdateState::Applying {
            latest,
            method,
            phase,
            timeout_at,
            ..
        } => {
            assert_eq!(latest, "5.0.0");
            assert_eq!(method, ApplyMethod::PortableReplace);
            assert_eq!(phase, ApplyPhase::Starting);
            assert!(timeout_at.is_none());
        }
        other => panic!("expected applying, got {other:?}"),
    }
}

#[tokio::test]
async fn set_payload_error_sets_error_on_available_state() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    manager
        .set_payload_error("download timeout".to_string())
        .await;

    match manager.state().await {
        UpdateState::Available { payload, .. } => {
            assert_eq!(
                payload,
                PayloadState::Error {
                    message: "download timeout".to_string()
                }
            );
        }
        other => panic!("expected available with error payload, got {other:?}"),
    }
}

#[tokio::test]
async fn set_payload_error_noop_on_non_available_state() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    // State is UpToDate (default), set_payload_error should be a no-op
    manager.set_payload_error("some error".to_string()).await;

    match manager.state().await {
        UpdateState::UpToDate { .. } => {} // unchanged
        other => panic!("expected up_to_date unchanged, got {other:?}"),
    }
}

// =======================================================================
// UpdateManager: require_ready_payload
// =======================================================================
#[tokio::test]
async fn require_ready_payload_returns_kind_when_ready() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let expected_kind = PayloadKind::Portable {
        binary_path: "/tmp/new-llmlb".to_string(),
    };
    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::Ready {
            kind: expected_kind.clone(),
        });
    }

    let kind = manager.require_ready_payload().await.unwrap();
    assert_eq!(kind, expected_kind);
}

#[tokio::test]
async fn require_ready_payload_errors_when_not_ready() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    {
        *manager.inner.state.write().await = available_state_with_payload(PayloadState::NotReady);
    }

    let err = manager.require_ready_payload().await.unwrap_err();
    assert!(err.to_string().contains("not ready"));
}

#[tokio::test]
async fn require_ready_payload_errors_when_no_update() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let err = manager.require_ready_payload().await.unwrap_err();
    assert!(err.to_string().contains("No update is available"));
}

// =======================================================================
// UpdateManager: validate_force_apply_request
// =======================================================================
#[tokio::test]
async fn validate_force_apply_rejects_draining_state() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    {
        *manager.inner.state.write().await = UpdateState::Draining {
            latest: "5.0.0".to_string(),
            in_flight: 3,
            requested_at: Utc::now(),
            timeout_at: Utc::now() + chrono::Duration::seconds(300),
        };
    }

    let err = manager.validate_force_apply_request().await.unwrap_err();
    assert!(err.to_string().contains("already in progress"));
}

#[tokio::test]
async fn validate_force_apply_rejects_up_to_date() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let err = manager.validate_force_apply_request().await.unwrap_err();
    assert!(err.to_string().contains("No update is available"));
}

// =======================================================================
// UpdateManager: apply_cache
// =======================================================================
#[tokio::test]
async fn apply_cache_empty_version_stays_up_to_date() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let cache = UpdateCacheFile {
        last_checked_at: Utc::now(),
        latest_version: Some("".to_string()),
        release_url: None,
        portable_asset_url: None,
        installer_asset_url: None,
    };
    manager.apply_cache(cache).await.unwrap();

    match manager.state().await {
        UpdateState::UpToDate { checked_at } => {
            assert!(checked_at.is_some());
        }
        other => panic!("expected up_to_date, got {other:?}"),
    }
}

#[tokio::test]
async fn apply_cache_none_version_stays_up_to_date() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let cache = UpdateCacheFile {
        last_checked_at: Utc::now(),
        latest_version: None,
        release_url: None,
        portable_asset_url: None,
        installer_asset_url: None,
    };
    manager.apply_cache(cache).await.unwrap();

    assert!(matches!(
        manager.state().await,
        UpdateState::UpToDate { .. }
    ));
}

#[tokio::test]
async fn apply_cache_invalid_version_errors() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    let cache = UpdateCacheFile {
        last_checked_at: Utc::now(),
        latest_version: Some("not-semver".to_string()),
        release_url: None,
        portable_asset_url: None,
        installer_asset_url: None,
    };
    let err = manager.apply_cache(cache).await;
    assert!(err.is_err());
}

// =======================================================================
// UpdateManager: record_check_failure from Draining and Applying states
// =======================================================================
#[tokio::test]
async fn record_check_failure_from_draining_preserves_latest() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    {
        *manager.inner.state.write().await = UpdateState::Draining {
            latest: "5.0.0".to_string(),
            in_flight: 2,
            requested_at: Utc::now(),
            timeout_at: Utc::now() + chrono::Duration::seconds(300),
        };
    }

    manager
        .record_check_failure("error during drain".to_string())
        .await;

    match manager.state().await {
        UpdateState::Failed {
            latest, message, ..
        } => {
            assert_eq!(latest, Some("5.0.0".to_string()));
            assert_eq!(message, "error during drain");
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

#[tokio::test]
async fn record_check_failure_from_applying_preserves_latest() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    {
        *manager.inner.state.write().await = UpdateState::Applying {
            latest: "5.0.0".to_string(),
            method: ApplyMethod::PortableReplace,
            phase: ApplyPhase::Starting,
            phase_message: "test".to_string(),
            started_at: Utc::now(),
            timeout_at: None,
        };
    }

    manager
        .record_check_failure("error during apply".to_string())
        .await;

    match manager.state().await {
        UpdateState::Failed { latest, .. } => {
            assert_eq!(latest, Some("5.0.0".to_string()));
        }
        other => panic!("expected failed, got {other:?}"),
    }
}

// =======================================================================
// UpdateManager: start_background_tasks is idempotent
// =======================================================================
#[tokio::test]
async fn start_background_tasks_is_idempotent() {
    let manager = UpdateManager::new(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
    )
    .unwrap();

    // First call should not panic
    manager.start_background_tasks();
    // Second call should be a no-op (idempotent)
    manager.start_background_tasks();
    // Just verify it doesn't panic or deadlock
    assert!(manager.inner.started.load(Ordering::SeqCst));
}

// =======================================================================
// UpdateManager: cancel_schedule errors when no schedule exists
// =======================================================================
#[test]
fn cancel_schedule_errors_when_empty() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    let err = manager
        .cancel_schedule()
        .expect_err("should error when no schedule");
    assert!(err.to_string().contains("No schedule exists"));
}

// =======================================================================
// UpdateManager: create_schedule errors on duplicate
// =======================================================================
#[test]
fn create_schedule_errors_on_duplicate() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    let sched = schedule::UpdateSchedule {
        mode: schedule::ScheduleMode::Idle,
        scheduled_at: None,
        scheduled_by: "admin".to_string(),
        target_version: "5.0.0".to_string(),
        created_at: Utc::now(),
    };

    manager.create_schedule(sched.clone()).unwrap();

    let err = manager
        .create_schedule(sched)
        .expect_err("should error on duplicate schedule");
    assert!(err.to_string().contains("already exists"));
}

// =======================================================================
// UpdateManager: history roundtrip
// =======================================================================
#[test]
fn record_and_get_history() {
    let gate = InferenceGate::default();
    let (manager, _tmp) = test_manager_with_gate(gate);

    assert!(manager.get_history().is_empty());

    manager.record_history(history::HistoryEntry {
        kind: history::HistoryEventKind::Applied,
        version: "5.0.0".to_string(),
        message: Some("applied successfully".to_string()),
        timestamp: Utc::now(),
    });

    let h = manager.get_history();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].version, "5.0.0");
}

// =======================================================================
// UpdateManager: new_with_data_dir isolation
// =======================================================================
#[test]
fn new_with_data_dir_uses_temp_dir() {
    let dir = tempfile::tempdir().unwrap();
    let manager = UpdateManager::new_with_data_dir(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
        dir.path(),
    )
    .unwrap();

    assert!(manager.inner.cache_path.starts_with(dir.path()));
    assert!(manager.inner.updates_dir.starts_with(dir.path()));
}

#[tokio::test]
async fn check_only_github_error_cache_fallback() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // GitHub API が 429 を返すようモック
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/releases/latest"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&mock_server)
        .await;

    let tmp = tempfile::tempdir().expect("create temp dir");
    let manager = UpdateManager::new_with_data_dir_and_config(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
        tmp.path(),
        Some(mock_server.uri()),
    )
    .expect("create update manager");

    // --- ケース1: キャッシュなし → エラーが返る ---
    assert!(
        !manager.inner.cache_path.exists(),
        "cache should not exist in fresh temp dir"
    );
    let result = manager.check_only(true).await;
    assert!(
        result.is_err(),
        "check_only should fail when no cache and GitHub returns 429"
    );

    // --- ケース2: キャッシュあり → フォールバックで成功 ---
    save_cache(
        &manager.inner.cache_path,
        UpdateCacheFile {
            last_checked_at: Utc::now(),
            latest_version: Some("99.0.0".to_string()),
            release_url: Some(
                "https://github.com/test-owner/test-repo/releases/tag/v99.0.0".to_string(),
            ),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
        },
    )
    .expect("save cache");

    // force=true でもキャッシュフォールバックすべき
    let state = manager
        .check_only(true)
        .await
        .expect("check_only should succeed via cache fallback");

    match &state {
        UpdateState::Available { latest, .. } => {
            assert_eq!(latest, "99.0.0");
        }
        other => panic!("expected Available from cache fallback, got {other:?}"),
    }

    // --- ケース3: 既にAvailable(payload=Ready)なら状態を保持 ---
    {
        let mut st = manager.inner.state.write().await;
        *st = UpdateState::Available {
            current: "5.0.0".to_string(),
            latest: "99.0.0".to_string(),
            release_url: "https://example.com/release".to_string(),
            portable_asset_url: Some("https://example.com/portable.tar.gz".to_string()),
            installer_asset_url: None,
            payload: PayloadState::Ready {
                kind: PayloadKind::Portable {
                    binary_path: "/tmp/llmlb-new".to_string(),
                },
            },
            checked_at: Utc::now(),
        };
    }

    let state = manager
        .check_only(true)
        .await
        .expect("check_only should preserve existing Available state");

    match &state {
        UpdateState::Available { payload, .. } => {
            assert!(
                matches!(payload, PayloadState::Ready { .. }),
                "payload should remain Ready, got {payload:?}"
            );
        }
        other => panic!("expected Available with Ready payload, got {other:?}"),
    }
}

/// Regression test: `check_and_maybe_download` must not deadlock by holding
/// the state write guard across the `ensure_payload_ready().await` call.
///
/// Before the fix, the write guard in `check_and_maybe_download` was not
/// dropped before calling `ensure_payload_ready`, which tried to acquire a
/// read lock on the same `RwLock` — causing an irrecoverable deadlock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_and_maybe_download_does_not_deadlock() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // Return a release with a version higher than current.
    let release_json = serde_json::json!({
        "tag_name": "v99.0.0",
        "html_url": "https://github.com/test-owner/test-repo/releases/tag/v99.0.0",
        "assets": []
    });
    Mock::given(method("GET"))
        .and(path("/repos/akiojin/llmlb/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&release_json))
        .mount(&mock_server)
        .await;

    let tmp = tempfile::tempdir().expect("create temp dir");
    let manager = UpdateManager::new_with_data_dir_and_config(
        reqwest::Client::new(),
        InferenceGate::default(),
        ShutdownController::default(),
        tmp.path(),
        Some(mock_server.uri()),
    )
    .expect("create update manager");

    // check_and_maybe_download with force=true triggers the code path
    // that previously deadlocked. Use a timeout to detect deadlocks.
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        manager.check_and_maybe_download(true),
    )
    .await;

    assert!(
        result.is_ok(),
        "check_and_maybe_download should not deadlock (timed out)"
    );

    // Verify we can still read state (would hang if write lock is held).
    let state = tokio::time::timeout(Duration::from_secs(2), manager.state())
        .await
        .expect("state() should not deadlock");

    match &state {
        UpdateState::Available { latest, .. } => {
            assert_eq!(latest, "99.0.0");
        }
        other => panic!("expected Available state, got {other:?}"),
    }
}
