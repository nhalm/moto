//! REST API handlers for moto-club.
//!
//! This crate provides the HTTP API layer for moto-club, including:
//! - Health check endpoints (`/health`, `/api/v1/info`)
//! - Garage management endpoints (`/api/v1/garages/*`)
//! - `WireGuard` coordination endpoints (`/api/v1/wg/*`) - future
//!
//! # Example
//!
//! ```ignore
//! use moto_club_api::{AppState, router};
//! use moto_club_db::DbPool;
//!
//! let pool = DbPool::connect("postgres://...").await?;
//! let state = AppState::new(pool);
//! let app = router(state);
//!
//! // Run with axum
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//! axum::serve(listener, app).await?;
//! ```

pub mod garages;
pub mod health;
pub mod wg;

use axum::Router;
use moto_club_db::DbPool;

/// Shared application state.
///
/// Contains all dependencies needed by API handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub db_pool: DbPool,
}

impl AppState {
    /// Creates a new `AppState` with the given database pool.
    #[must_use]
    pub const fn new(db_pool: DbPool) -> Self {
        Self { db_pool }
    }
}

/// Creates the main API router with all routes.
///
/// The router includes:
/// - Health endpoints from [`health::router()`]
/// - Garage endpoints from [`garages::router()`]
/// - `WireGuard` endpoints from [`wg::router()`]
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(garages::router())
        .merge(wg::router())
        .with_state(state)
}

/// API error type.
///
/// Standard error format for all API responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiError {
    /// Error details.
    pub error: ApiErrorDetail,
}

/// API error detail.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiErrorDetail {
    /// Error code (e.g., `GARAGE_NOT_FOUND`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Additional details (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// Creates a new API error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }

    /// Creates a new API error with details.
    #[must_use]
    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
                details: Some(details),
            },
        }
    }
}

/// Error codes used by the API.
pub mod error_codes {
    /// Garage not found.
    pub const GARAGE_NOT_FOUND: &str = "GARAGE_NOT_FOUND";
    /// Garage not owned by the requesting user.
    pub const GARAGE_NOT_OWNED: &str = "GARAGE_NOT_OWNED";
    /// Garage name already taken.
    pub const GARAGE_ALREADY_EXISTS: &str = "GARAGE_ALREADY_EXISTS";
    /// Garage has been terminated.
    pub const GARAGE_TERMINATED: &str = "GARAGE_TERMINATED";
    /// Garage TTL has expired.
    pub const GARAGE_EXPIRED: &str = "GARAGE_EXPIRED";
    /// Invalid TTL value.
    pub const INVALID_TTL: &str = "INVALID_TTL";
    /// `WireGuard` device not found.
    pub const DEVICE_NOT_FOUND: &str = "DEVICE_NOT_FOUND";
    /// `WireGuard` session not found.
    pub const SESSION_NOT_FOUND: &str = "SESSION_NOT_FOUND";
    /// Session has expired.
    pub const SESSION_EXPIRED: &str = "SESSION_EXPIRED";
    /// Internal server error.
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    /// Kubernetes API error.
    pub const K8S_ERROR: &str = "K8S_ERROR";
    /// Database connection error.
    pub const DATABASE_ERROR: &str = "DATABASE_ERROR";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_serialization() {
        let error = ApiError::new("GARAGE_NOT_FOUND", "Garage 'bold-mongoose' not found");
        let json = serde_json::to_string(&error).unwrap();

        assert!(json.contains(r#""code":"GARAGE_NOT_FOUND""#));
        assert!(json.contains(r#""message":"Garage 'bold-mongoose' not found""#));
        // No details field when None
        assert!(!json.contains("details"));
    }

    #[test]
    fn api_error_with_details_serialization() {
        let details = serde_json::json!({"attempted_name": "bold-mongoose"});
        let error = ApiError::with_details("GARAGE_ALREADY_EXISTS", "Name already taken", details);
        let json = serde_json::to_string(&error).unwrap();

        assert!(json.contains(r#""code":"GARAGE_ALREADY_EXISTS""#));
        assert!(json.contains(r#""details""#));
        assert!(json.contains(r#""attempted_name""#));
    }
}
