//! Health check endpoints for moto-ai-proxy.
//!
//! Provides Kubernetes-style health check functionality per the Engine Contract (moto-bike.md):
//! - `/health/live` - Process is alive (not deadlocked), always 200 if reachable
//! - `/health/ready` - Ready for traffic (keybox reachable, at least one provider key cached)
//! - `/health/startup` - Initial startup complete (SVID loaded, initial key fetch complete)
//!
//! These endpoints are served on a separate port (8081) from the main API (8080).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;

use crate::keys::{KeyStore, has_cached_keys};

/// Global startup flag — set to true once initial startup is complete.
static STARTUP_COMPLETE: AtomicBool = AtomicBool::new(false);

/// Marks the startup as complete. Call this after SVID is loaded and initial key fetch is done.
pub fn mark_startup_complete() {
    STARTUP_COMPLETE.store(true, Ordering::SeqCst);
}

/// Returns whether startup is complete.
#[must_use]
pub fn is_startup_complete() -> bool {
    STARTUP_COMPLETE.load(Ordering::SeqCst)
}

/// Health check response body.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    /// Status string: `ok`, `not_ready`, or `starting`.
    pub status: &'static str,
}

/// Liveness probe handler — returns 200 if the process is alive.
async fn live_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

/// Readiness probe handler — returns 200 if ready for traffic.
///
/// Per ai-proxy spec: keybox reachable, at least one provider key cached.
async fn ready_handler<K: KeyStore>(State(key_store): State<Arc<K>>) -> impl IntoResponse {
    if is_startup_complete() && has_cached_keys(key_store.as_ref()).await {
        (StatusCode::OK, Json(HealthResponse { status: "ok" }))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "not_ready",
            }),
        )
    }
}

/// Startup probe handler — returns 200 if initial startup is complete.
///
/// Per ai-proxy spec: SVID loaded, initial key fetch complete.
async fn startup_handler() -> impl IntoResponse {
    if is_startup_complete() {
        (StatusCode::OK, Json(HealthResponse { status: "ok" }))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse { status: "starting" }),
        )
    }
}

/// Creates the health server router for port 8081.
pub fn health_router<K: KeyStore + 'static>(key_store: Arc<K>) -> Router {
    Router::new()
        .route("/health/live", get(live_handler))
        .route("/health/ready", get(ready_handler::<K>))
        .route("/health/startup", get(startup_handler))
        .with_state(key_store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_serialization() {
        let response = HealthResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
    }

    #[test]
    fn startup_flag_defaults_to_false() {
        // Reset for test isolation — in practice the static is process-global.
        STARTUP_COMPLETE.store(false, Ordering::SeqCst);
        assert!(!is_startup_complete());
    }

    #[test]
    fn mark_startup_complete_sets_flag() {
        STARTUP_COMPLETE.store(false, Ordering::SeqCst);
        mark_startup_complete();
        assert!(is_startup_complete());
        // Reset after test.
        STARTUP_COMPLETE.store(false, Ordering::SeqCst);
    }
}
