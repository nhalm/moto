//! Health check endpoints for moto-keybox.
//!
//! Provides Kubernetes-style health check functionality per the Engine Contract (moto-bike.md):
//! - `/health/live` - Process is alive (not deadlocked), always 200 if reachable
//! - `/health/ready` - Ready for traffic (master key loaded, DB connected)
//! - `/health/startup` - Initial startup complete
//!
//! These endpoints are served on a separate port (8081) from the main API (8080).

use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use moto_keybox_db::DbPool;
use serde::Serialize;

/// Global startup flag - set to true once initial startup is complete.
static STARTUP_COMPLETE: AtomicBool = AtomicBool::new(false);

/// Marks the startup as complete. Call this after all initialization is done.
pub fn mark_startup_complete() {
    STARTUP_COMPLETE.store(true, Ordering::SeqCst);
}

/// Returns whether startup is complete.
#[must_use]
pub fn is_startup_complete() -> bool {
    STARTUP_COMPLETE.load(Ordering::SeqCst)
}

/// Optional state for health endpoints, holding a DB pool when using `PostgreSQL`.
#[derive(Clone)]
pub struct HealthState {
    pool: Option<DbPool>,
}

/// Liveness response body.
#[derive(Debug, Clone, Serialize)]
pub struct LiveResponse {
    /// Always "ok" if the process is reachable.
    pub status: &'static str,
}

/// Ready response body.
#[derive(Debug, Clone, Serialize)]
pub struct ReadyResponse {
    /// "ok" if all dependencies are ready, `not_ready` otherwise.
    pub status: &'static str,
}

/// Startup response body.
#[derive(Debug, Clone, Serialize)]
pub struct StartupResponse {
    /// "ok" if startup is complete, "starting" otherwise.
    pub status: &'static str,
}

/// Liveness probe handler.
///
/// Returns 200 if the process is alive and not deadlocked.
/// This is the simplest check - if the handler runs, the process is alive.
async fn live_handler() -> impl IntoResponse {
    let response = LiveResponse { status: "ok" };
    (StatusCode::OK, Json(response))
}

/// Readiness probe handler.
///
/// Returns 200 if the service is ready to accept traffic.
/// Per keybox.md spec, readiness criteria are:
/// - Master key successfully loaded
/// - SVID signing key successfully loaded
/// - Database connection established (when using `PostgreSQL` backend)
async fn ready_handler(State(state): State<HealthState>) -> impl IntoResponse {
    if !is_startup_complete() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "not_ready",
            }),
        );
    }

    // Check DB connectivity when using PostgreSQL backend
    if let Some(ref pool) = state.pool
        && sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "not_ready",
            }),
        );
    }

    (StatusCode::OK, Json(ReadyResponse { status: "ok" }))
}

/// Startup probe handler.
///
/// Returns 200 if initial startup is complete.
/// Returns 503 if still starting up.
///
/// K8s uses this to know when to start liveness/readiness checks.
async fn startup_handler() -> impl IntoResponse {
    if is_startup_complete() {
        let response = StartupResponse { status: "ok" };
        (StatusCode::OK, Json(response))
    } else {
        let response = StartupResponse { status: "starting" };
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
}

/// Creates the health server router for port 8081.
///
/// Per the Engine Contract (moto-bike.md), health endpoints are served on a separate port:
/// - `GET /health/live` - Liveness probe (process is alive)
/// - `GET /health/ready` - Readiness probe (keys loaded, DB connected, ready for traffic)
/// - `GET /health/startup` - Startup probe (initial startup complete)
///
/// Pass a `DbPool` to enable runtime database connectivity checks in the readiness probe.
/// When `None`, readiness only checks startup completion (in-memory mode).
pub fn health_router(pool: Option<DbPool>) -> Router {
    let state = HealthState { pool };
    Router::new()
        .route("/health/live", get(live_handler))
        .route("/health/ready", get(ready_handler))
        .route("/health/startup", get(startup_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_response_serialization() {
        let response = LiveResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
    }

    #[test]
    fn ready_response_ok_serialization() {
        let response = ReadyResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
    }

    #[test]
    fn ready_response_not_ready_serialization() {
        let response = ReadyResponse {
            status: "not_ready",
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"not_ready"}"#);
    }

    #[test]
    fn startup_response_ok_serialization() {
        let response = StartupResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
    }

    #[test]
    fn startup_response_starting_serialization() {
        let response = StartupResponse { status: "starting" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"starting"}"#);
    }
}
