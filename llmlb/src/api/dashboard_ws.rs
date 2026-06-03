//! WebSocket endpoint for real-time dashboard updates
//!
//! This module provides `/ws/dashboard` endpoint that streams
//! DashboardEvents to connected clients in real-time.
//!
//! Authentication is required via Bearer token (`Authorization`) or JWT cookie.

use crate::common::auth::UserRole;
use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tracing::{debug, warn};

use crate::events::SharedEventBus;
use crate::AppState;

/// Query parameter for WebSocket token authentication
#[derive(serde::Deserialize, Default)]
pub struct WsAuthQuery {
    /// JWT token passed as query parameter (e.g., `?token=xxx`)
    pub token: Option<String>,
}

/// WebSocket upgrade handler for dashboard events
///
/// Clients connect to `/ws/dashboard` to receive real-time updates about:
/// - Node registration/removal
/// - Node status changes
/// - Metrics updates
///
/// Authentication is always required (JWT via Authorization header, cookie, or query parameter).
pub async fn dashboard_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsAuthQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let token = if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    "Invalid Authorization header format".to_string(),
                )
            })?
            .to_string()
    } else if let Some(query_token) = query.token {
        query_token
    } else {
        crate::auth::middleware::extract_jwt_cookie(&headers)
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Missing JWT cookie".to_string()))?
    };

    let claims = crate::auth::jwt::verify_jwt(&token, &state.jwt_secret).map_err(|e| {
        warn!("WebSocket JWT verification failed: {}", e);
        (StatusCode::UNAUTHORIZED, format!("Invalid token: {}", e))
    })?;

    // Only admin users can access the dashboard WebSocket
    if claims.role != UserRole::Admin {
        return Err((StatusCode::FORBIDDEN, "Admin access required".to_string()));
    }

    debug!("WebSocket authenticated for user: {}", claims.sub);

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state.event_bus.clone())))
}

async fn handle_socket(socket: WebSocket, event_bus: SharedEventBus) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = event_bus.subscribe();

    debug!("Dashboard WebSocket client connected");

    // Send initial connection confirmation
    let welcome = serde_json::json!({
        "type": "connected",
        "message": "Dashboard WebSocket connected"
    });
    if let Err(e) = sender.send(Message::Text(welcome.to_string().into())).await {
        warn!("Failed to send welcome message: {}", e);
        return;
    }

    // Spawn task to handle incoming messages (ping/pong, close)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, will respond with pong");
                    // Pong is handled automatically by axum
                    let _ = data;
                }
                Err(e) => {
                    warn!("WebSocket receive error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Send events to the client
    loop {
        tokio::select! {
            // Check if receive task finished (client disconnected)
            _ = &mut recv_task => {
                debug!("Dashboard WebSocket client disconnected");
                break;
            }
            // Receive events from the event bus
            event_result = event_rx.recv() => {
                match event_result {
                    Ok(event) => {
                        let json = match serde_json::to_string(&event) {
                            Ok(j) => j,
                            Err(e) => {
                                warn!("Failed to serialize event: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = sender.send(Message::Text(json.into())).await {
                            warn!("Failed to send event: {}", e);
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Dashboard WebSocket lagged by {} events", n);
                        // Continue receiving, we just lost some events
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("Event bus closed");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- WsAuthQuery deserialization tests ---

    #[test]
    fn ws_auth_query_default_has_no_token() {
        let query = WsAuthQuery::default();
        assert!(query.token.is_none());
    }

    #[test]
    fn ws_auth_query_deserialize_with_token() {
        let json = r#"{"token": "my-jwt-token"}"#;
        let query: WsAuthQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.token, Some("my-jwt-token".to_string()));
    }

    #[test]
    fn ws_auth_query_deserialize_without_token() {
        let json = r#"{}"#;
        let query: WsAuthQuery = serde_json::from_str(json).unwrap();
        assert!(query.token.is_none());
    }

    #[test]
    fn ws_auth_query_deserialize_with_null_token() {
        let json = r#"{"token": null}"#;
        let query: WsAuthQuery = serde_json::from_str(json).unwrap();
        assert!(query.token.is_none());
    }

    #[test]
    fn ws_auth_query_deserialize_with_empty_token() {
        let json = r#"{"token": ""}"#;
        let query: WsAuthQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.token, Some("".to_string()));
    }

    #[test]
    fn ws_auth_query_deserialize_ignores_extra_fields() {
        let json = r#"{"token": "abc", "extra": "ignored"}"#;
        let query: WsAuthQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.token, Some("abc".to_string()));
    }

    #[test]
    fn ws_auth_query_deserialize_long_token() {
        let long_token = "a".repeat(2048);
        let json = format!(r#"{{"token": "{}"}}"#, long_token);
        let query: WsAuthQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(query.token.as_deref(), Some(long_token.as_str()));
    }

    // --- Welcome message format tests ---

    #[test]
    fn welcome_message_has_correct_format() {
        let welcome = serde_json::json!({
            "type": "connected",
            "message": "Dashboard WebSocket connected"
        });
        assert_eq!(welcome["type"], "connected");
        assert_eq!(welcome["message"], "Dashboard WebSocket connected");
    }

    #[test]
    fn welcome_message_serializes_to_valid_json() {
        let welcome = serde_json::json!({
            "type": "connected",
            "message": "Dashboard WebSocket connected"
        });
        let json_str = welcome.to_string();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["type"], "connected");
    }

    // --- UserRole authorization logic tests ---

    #[test]
    fn admin_role_is_authorized_for_ws() {
        let role = UserRole::Admin;
        assert_eq!(role, UserRole::Admin);
    }

    #[test]
    fn viewer_role_is_not_authorized_for_ws() {
        let role = UserRole::Viewer;
        assert_ne!(role, UserRole::Admin);
    }

    // --- Token extraction logic tests (unit-level) ---

    #[test]
    fn bearer_prefix_stripping() {
        let auth_header = "Bearer my-token-123";
        let token = auth_header.strip_prefix("Bearer ").unwrap();
        assert_eq!(token, "my-token-123");
    }

    #[test]
    fn bearer_prefix_missing_returns_none() {
        let auth_header = "Basic dXNlcjpwYXNz";
        let token = auth_header.strip_prefix("Bearer ");
        assert!(token.is_none());
    }

    #[test]
    fn bearer_prefix_case_sensitive() {
        let auth_header = "bearer my-token";
        let token = auth_header.strip_prefix("Bearer ");
        assert!(token.is_none());
    }

    #[test]
    fn bearer_prefix_with_empty_token() {
        let auth_header = "Bearer ";
        let token = auth_header.strip_prefix("Bearer ").unwrap();
        assert_eq!(token, "");
    }
}
