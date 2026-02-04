//! Health check endpoints for moto-keybox.
//!
//! Provides Kubernetes-style health check functionality per the Engine Contract (moto-bike.md):
//! - `/health/live` - Process is alive (not deadlocked), always 200 if reachable
//! - `/health/ready` - Ready for traffic (master key loaded, SVID signing key loaded)
//! - `/health/startup` - Initial startup complete
//!
//! These endpoints are served on a separate port (8081) from the main API (8080).

use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
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
///
/// Since keys are loaded at startup and we can't proceed without them,
/// readiness depends on startup completion.
async fn ready_handler() -> impl IntoResponse {
    // If startup is complete, we're ready (keys are loaded)
    // If startup failed, the process would have exited
    if is_startup_complete() {
        let response = ReadyResponse { status: "ok" };
        (StatusCode::OK, Json(response))
    } else {
        let response = ReadyResponse {
            status: "not_ready",
        };
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
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
/// - `GET /health/ready` - Readiness probe (keys loaded, ready for traffic)
/// - `GET /health/startup` - Startup probe (initial startup complete)
pub fn health_router() -> Router {
    Router::new()
        .route("/health/live", get(live_handler))
        .route("/health/ready", get(ready_handler))
        .route("/health/startup", get(startup_handler))
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
