//! System API (self-update status / apply / schedule).

use crate::common::auth::{Claims, UserRole};
use crate::common::error::LbError;
use crate::UpdateApiState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::error::AppError;

#[derive(Debug, Serialize)]
struct SystemInfoResponse {
    version: String,
    pid: u32,
    in_flight: usize,
    update: crate::update::UpdateState,
    schedule: Option<crate::update::schedule::UpdateSchedule>,
    rollback_available: bool,
}

#[derive(Debug, Serialize)]
struct CheckUpdateResponse {
    update: crate::update::UpdateState,
}

#[derive(Debug, Serialize)]
struct ApplyUpdateResponse {
    queued: bool,
    mode: &'static str,
}

#[derive(Debug, Serialize)]
struct ForceApplyUpdateResponse {
    queued: bool,
    mode: &'static str,
    dropped_in_flight: usize,
}

/// GET /api/version
///
/// 認証不要。ビルド時のバージョン文字列を返す。
pub async fn get_version() -> Response {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") })).into_response()
}

/// GET /api/system
pub async fn get_system(State(state): State<UpdateApiState>) -> Response {
    let update = state.update_manager.state().await;
    let in_flight = state.inference_gate.in_flight();
    let schedule = state.update_manager.get_schedule().ok().flatten();
    let rollback_available = state.update_manager.rollback_available();
    Json(SystemInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        pid: std::process::id(),
        in_flight,
        update,
        schedule,
        rollback_available,
    })
    .into_response()
}

/// POST /api/system/update/check
///
/// Check for updates (GitHub API only, no download).
/// Rate-limited to once per 60 seconds.
///
/// Admin only (JWT middleware applied in create_app).
pub async fn check_update(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    // Rate limit: reject if checked within the last 60 seconds.
    if state.update_manager.is_manual_check_rate_limited() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": {
                    "message": "Rate limited: please wait before checking again",
                    "type": "rate_limit",
                    "code": 429
                }
            })),
        )
            .into_response();
    }

    state.update_manager.record_manual_check();

    match state.update_manager.check_only(true).await {
        Ok(update) => {
            // If an update is available, start background download.
            if matches!(&update, crate::update::UpdateState::Available { .. }) {
                state.update_manager.download_background();
            }
            state
                .event_bus
                .publish(crate::events::DashboardEvent::UpdateStateChanged);
            (StatusCode::OK, Json(CheckUpdateResponse { update })).into_response()
        }
        Err(err) => {
            state
                .update_manager
                .record_check_failure(err.to_string())
                .await;
            state
                .event_bus
                .publish(crate::events::DashboardEvent::UpdateStateChanged);
            AppError(LbError::Http(err.to_string())).into_response()
        }
    }
}

/// POST /api/system/update/apply
///
/// Admin only (JWT middleware applied in create_app).
pub async fn apply_update(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    let queued = state.update_manager.request_apply_normal().await;
    (
        StatusCode::ACCEPTED,
        Json(ApplyUpdateResponse {
            queued,
            mode: "normal",
        }),
    )
        .into_response()
}

/// POST /api/system/update/apply/force
///
/// Admin only (JWT middleware applied in create_app).
pub async fn apply_force_update(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    match state.update_manager.request_apply_force().await {
        Ok(dropped_in_flight) => (
            StatusCode::ACCEPTED,
            Json(ForceApplyUpdateResponse {
                queued: false,
                mode: "force",
                dropped_in_flight,
            }),
        )
            .into_response(),
        Err(err) => AppError(LbError::Conflict(err.to_string())).into_response(),
    }
}

/// Request body for `POST /api/system/update/schedule`.
#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    mode: String,
    #[serde(default)]
    scheduled_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ScheduleResponse {
    schedule: crate::update::schedule::UpdateSchedule,
}

fn parse_scheduled_at(value: &str) -> Option<DateTime<Utc>> {
    // Accept RFC3339 values with timezone (e.g. "...Z", "...+09:00").
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&Utc));
    }

    // Also accept `datetime-local` values from the dashboard (no timezone).
    // They are interpreted in the server's local timezone.
    for format in [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        let Ok(naive) = NaiveDateTime::parse_from_str(value, format) else {
            continue;
        };
        let local = match Local.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt,
            LocalResult::Ambiguous(dt, _) => dt,
            LocalResult::None => return None,
        };
        return Some(local.with_timezone(&Utc));
    }

    None
}

/// POST /api/system/update/schedule
///
/// Admin only.
pub async fn create_schedule(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateScheduleRequest>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    let mode = match body.mode.as_str() {
        "immediate" => crate::update::schedule::ScheduleMode::Immediate,
        "idle" => crate::update::schedule::ScheduleMode::Idle,
        "scheduled" => crate::update::schedule::ScheduleMode::Scheduled,
        _ => {
            return AppError(LbError::Http(format!("Invalid mode: {}", body.mode))).into_response();
        }
    };

    let scheduled_at = if let Some(at) = body.scheduled_at {
        match parse_scheduled_at(&at) {
            Some(dt) => Some(dt),
            None => {
                return AppError(LbError::Http("Invalid scheduled_at datetime".to_string()))
                    .into_response();
            }
        }
    } else {
        None
    };

    // Determine the target version from current update state.
    let target_version = match state.update_manager.state().await {
        crate::update::UpdateState::Available { latest, .. } => latest,
        _ => {
            return AppError(LbError::Conflict(
                "No update is available to schedule".to_string(),
            ))
            .into_response();
        }
    };

    let schedule = crate::update::schedule::UpdateSchedule {
        mode,
        scheduled_at,
        scheduled_by: claims.sub,
        target_version,
        created_at: Utc::now(),
    };

    match state.update_manager.create_schedule(schedule) {
        Ok(sched) => {
            state
                .event_bus
                .publish(crate::events::DashboardEvent::UpdateStateChanged);
            (
                StatusCode::CREATED,
                Json(ScheduleResponse { schedule: sched }),
            )
                .into_response()
        }
        Err(err) => AppError(LbError::Conflict(err.to_string())).into_response(),
    }
}

/// GET /api/system/update/schedule
///
/// Admin only.
pub async fn get_schedule(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    match state.update_manager.get_schedule() {
        Ok(Some(schedule)) => Json(json!({ "schedule": schedule })).into_response(),
        Ok(None) => Json(json!({ "schedule": null })).into_response(),
        Err(err) => AppError(LbError::Http(err.to_string())).into_response(),
    }
}

/// DELETE /api/system/update/schedule
///
/// Admin only.
pub async fn cancel_schedule(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    match state.update_manager.cancel_schedule() {
        Ok(()) => {
            state
                .event_bus
                .publish(crate::events::DashboardEvent::UpdateStateChanged);
            Json(json!({ "cancelled": true })).into_response()
        }
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "message": err.to_string() } })),
        )
            .into_response(),
    }
}

/// POST /api/system/update/rollback
///
/// Admin only. Restores the previous version from `.bak` if available.
pub async fn rollback(
    State(state): State<UpdateApiState>,
    Extension(claims): Extension<Claims>,
) -> Response {
    if claims.role != UserRole::Admin {
        return AppError(LbError::Authorization("Admin access required".to_string()))
            .into_response();
    }

    match state.update_manager.request_rollback() {
        Ok(()) => {
            state
                .event_bus
                .publish(crate::events::DashboardEvent::UpdateStateChanged);
            (StatusCode::ACCEPTED, Json(json!({ "rolling_back": true }))).into_response()
        }
        Err(err) => AppError(LbError::Conflict(err.to_string())).into_response(),
    }
}

#[cfg(test)]
mod tests;
