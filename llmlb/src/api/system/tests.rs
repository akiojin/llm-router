use super::parse_scheduled_at;
use chrono::{Local, LocalResult, NaiveDateTime, TimeZone, Utc};

#[test]
fn parse_scheduled_at_accepts_rfc3339() {
    let parsed = parse_scheduled_at("2026-02-23T12:34:56Z").expect("must parse RFC3339");
    let expected = Utc
        .with_ymd_and_hms(2026, 2, 23, 12, 34, 56)
        .single()
        .expect("valid datetime");
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_accepts_datetime_local_without_timezone() {
    let input = "2026-02-23T12:34";
    let parsed = parse_scheduled_at(input).expect("must parse datetime-local");

    let naive =
        NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M").expect("valid naive datetime");
    let expected = match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        LocalResult::None => panic!("datetime must be representable in local timezone"),
    };
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_rejects_invalid_input() {
    assert!(parse_scheduled_at("not-a-date").is_none());
}

// --- parse_scheduled_at additional coverage ---

#[test]
fn parse_scheduled_at_accepts_rfc3339_with_positive_offset() {
    let parsed = parse_scheduled_at("2026-02-23T21:34:56+09:00").expect("must parse");
    let expected = Utc
        .with_ymd_and_hms(2026, 2, 23, 12, 34, 56)
        .single()
        .expect("valid datetime");
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_accepts_rfc3339_with_negative_offset() {
    let parsed = parse_scheduled_at("2026-02-23T07:34:56-05:00").expect("must parse");
    let expected = Utc
        .with_ymd_and_hms(2026, 2, 23, 12, 34, 56)
        .single()
        .expect("valid datetime");
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_accepts_datetime_local_with_seconds() {
    let input = "2026-02-23T12:34:56";
    let parsed = parse_scheduled_at(input).expect("must parse datetime-local with seconds");

    let naive =
        NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S").expect("valid naive datetime");
    let expected = match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        LocalResult::None => panic!("datetime must be representable"),
    };
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_accepts_datetime_local_with_fractional_seconds() {
    let input = "2026-02-23T12:34:56.789";
    let parsed = parse_scheduled_at(input).expect("must parse fractional seconds");

    let naive =
        NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S%.f").expect("valid naive datetime");
    let expected = match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        LocalResult::None => panic!("datetime must be representable"),
    };
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_rejects_empty_string() {
    assert!(parse_scheduled_at("").is_none());
}

#[test]
fn parse_scheduled_at_rejects_date_only() {
    assert!(parse_scheduled_at("2026-02-23").is_none());
}

#[test]
fn parse_scheduled_at_rejects_time_only() {
    assert!(parse_scheduled_at("12:34:56").is_none());
}

#[test]
fn parse_scheduled_at_rejects_unix_timestamp() {
    assert!(parse_scheduled_at("1740000000").is_none());
}

#[test]
fn parse_scheduled_at_rejects_partial_datetime() {
    assert!(parse_scheduled_at("2026-02-23T").is_none());
}

#[test]
fn parse_scheduled_at_midnight_utc() {
    let parsed = parse_scheduled_at("2026-01-01T00:00:00Z").expect("must parse midnight");
    let expected = Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .expect("valid datetime");
    assert_eq!(parsed, expected);
}

#[test]
fn parse_scheduled_at_end_of_day_utc() {
    let parsed = parse_scheduled_at("2026-12-31T23:59:59Z").expect("must parse end of day");
    let expected = Utc
        .with_ymd_and_hms(2026, 12, 31, 23, 59, 59)
        .single()
        .expect("valid datetime");
    assert_eq!(parsed, expected);
}

// --- CreateScheduleRequest deserialization tests ---

#[test]
fn create_schedule_request_deserialize_immediate() {
    let json = r#"{"mode": "immediate"}"#;
    let req: super::CreateScheduleRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.mode, "immediate");
    assert!(req.scheduled_at.is_none());
}

#[test]
fn create_schedule_request_deserialize_scheduled_with_time() {
    let json = r#"{"mode": "scheduled", "scheduled_at": "2026-03-01T10:00:00Z"}"#;
    let req: super::CreateScheduleRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.mode, "scheduled");
    assert_eq!(req.scheduled_at.as_deref(), Some("2026-03-01T10:00:00Z"));
}

#[test]
fn create_schedule_request_deserialize_idle() {
    let json = r#"{"mode": "idle"}"#;
    let req: super::CreateScheduleRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.mode, "idle");
}

#[test]
fn create_schedule_request_scheduled_at_defaults_to_none() {
    let json = r#"{"mode": "immediate"}"#;
    let req: super::CreateScheduleRequest = serde_json::from_str(json).unwrap();
    assert!(req.scheduled_at.is_none());
}

#[test]
fn create_schedule_request_rejects_missing_mode() {
    let json = r#"{}"#;
    let result = serde_json::from_str::<super::CreateScheduleRequest>(json);
    assert!(result.is_err());
}

#[test]
fn create_schedule_request_rejects_non_string_mode() {
    let json = r#"{"mode": 123}"#;
    let result = serde_json::from_str::<super::CreateScheduleRequest>(json);
    assert!(result.is_err());
}

// --- Response struct serialization tests ---

#[test]
fn system_info_response_serializes_all_fields() {
    let resp = super::SystemInfoResponse {
        version: "1.0.0".to_string(),
        pid: 12345,
        in_flight: 3,
        update: crate::update::UpdateState::UpToDate { checked_at: None },
        schedule: None,
        rollback_available: false,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["version"], "1.0.0");
    assert_eq!(json["pid"], 12345);
    assert_eq!(json["in_flight"], 3);
    assert_eq!(json["rollback_available"], false);
}

#[test]
fn check_update_response_serializes() {
    let resp = super::CheckUpdateResponse {
        update: crate::update::UpdateState::UpToDate { checked_at: None },
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("update").is_some());
}

#[test]
fn apply_update_response_serializes() {
    let resp = super::ApplyUpdateResponse {
        queued: true,
        mode: "normal",
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["queued"], true);
    assert_eq!(json["mode"], "normal");
}

#[test]
fn force_apply_update_response_serializes() {
    let resp = super::ForceApplyUpdateResponse {
        queued: false,
        mode: "force",
        dropped_in_flight: 5,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["queued"], false);
    assert_eq!(json["mode"], "force");
    assert_eq!(json["dropped_in_flight"], 5);
}

#[test]
fn apply_update_response_mode_field_is_static_str() {
    let resp = super::ApplyUpdateResponse {
        queued: false,
        mode: "normal",
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"mode\":\"normal\""));
}

#[test]
fn force_apply_update_response_dropped_zero() {
    let resp = super::ForceApplyUpdateResponse {
        queued: true,
        mode: "force",
        dropped_in_flight: 0,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["dropped_in_flight"], 0);
}

// --- parse_scheduled_at consistency tests ---

#[test]
fn parse_scheduled_at_rfc3339_utc_equivalent() {
    let z_parsed = parse_scheduled_at("2026-06-15T08:30:00Z").unwrap();
    let offset_parsed = parse_scheduled_at("2026-06-15T08:30:00+00:00").unwrap();
    assert_eq!(z_parsed, offset_parsed);
}

#[test]
fn parse_scheduled_at_rejects_whitespace() {
    assert!(parse_scheduled_at("  ").is_none());
}

#[test]
fn parse_scheduled_at_rejects_random_chars() {
    assert!(parse_scheduled_at("abc123xyz").is_none());
}
