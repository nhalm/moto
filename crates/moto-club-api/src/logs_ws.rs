//! WebSocket endpoint for log streaming.
//!
//! Provides:
//! - `WS /ws/v1/garages/{name}/logs` - Stream garage container logs

use axum::{
    Json, Router,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    routing::get,
};

use crate::{ApiError, AppState};
use moto_club_ws::logs::LogStreamQuery;

/// Extract owner from Authorization header (same as garages module).
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

/// WebSocket upgrade handler for log streaming.
///
/// WS /ws/v1/garages/{name}/logs?tail=100&follow=false&since=5m
async fn logs_websocket(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<LogStreamQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let owner = extract_owner(&headers)?;

    tracing::info!(garage = %name, owner = %owner, "log WebSocket upgrade requested");

    Ok(ws.on_upgrade(move |socket| {
        moto_club_ws::handle_log_socket(socket, name, owner, query, state)
    }))
}

/// Creates the log streaming WebSocket router.
pub fn router() -> Router<AppState> {
    Router::new().route("/ws/v1/garages/{name}/logs", get(logs_websocket))
}
