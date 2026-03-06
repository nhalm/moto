//! WebSocket endpoint for event streaming.
//!
//! Provides:
//! - `WS /ws/v1/events` - Stream garage events (TTL warnings, status changes, errors)

use axum::{
    Json, Router,
    extract::{Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    routing::get,
};

use crate::{ApiError, AppState};
use moto_club_ws::events::EventStreamQuery;

/// Extract owner from Authorization header (same as `logs_ws` module).
fn extract_owner(headers: &HeaderMap) -> Result<String, (StatusCode, Json<ApiError>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    "UNAUTHORIZED",
                    "Missing Authorization header",
                )),
            )
        })?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    "UNAUTHORIZED",
                    "Invalid Authorization header format, expected 'Bearer <token>'",
                )),
            )
        })?;

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("UNAUTHORIZED", "Empty Bearer token")),
        ));
    }

    Ok(token.to_string())
}

/// WebSocket upgrade handler for event streaming.
///
/// WS /ws/v1/events?garages=bold-mongoose,quiet-falcon
async fn events_websocket(
    State(state): State<AppState>,
    Query(query): Query<EventStreamQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let owner = extract_owner(&headers)?;

    tracing::info!(owner = %owner, garages = ?query.garages, "event WebSocket upgrade requested");

    Ok(ws.on_upgrade(move |socket| moto_club_ws::handle_event_socket(socket, owner, query, state)))
}

/// Creates the event streaming WebSocket router.
pub fn router() -> Router<AppState> {
    Router::new().route("/ws/v1/events", get(events_websocket))
}
