//! Health check endpoints.
//!
//! Provides health check functionality that reports the status of dependent services.
//! The health endpoint always returns 200 and reports degraded status rather than failing.

use std::collections::HashMap;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;

use crate::AppState;

/// Overall health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// All services are healthy.
    Healthy,
    /// Some services are degraded but the system is operational.
    Degraded,
}

/// Individual service check result.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CheckResult {
    /// Service is healthy.
    Ok(OkResult),
    /// Service has an error.
    Error(ErrorResult),
}

/// Successful check result.
#[derive(Debug, Clone, Serialize)]
pub struct OkResult {
    status: &'static str,
}

/// Failed check result.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResult {
    status: &'static str,
    error: String,
}

impl CheckResult {
    /// Creates a successful check result.
    #[must_use]
    pub const fn ok() -> Self {
        Self::Ok(OkResult { status: "ok" })
    }

    /// Creates a failed check result with an error message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error(ErrorResult {
            status: "error",
            error: message.into(),
        })
    }

    /// Returns true if the check was successful.
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }
}

/// Health check response body.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    /// Overall health status.
    pub status: HealthStatus,
    /// Individual service check results.
    pub checks: HashMap<String, CheckResult>,
}

/// Check database connectivity.
async fn check_database(pool: &moto_club_db::DbPool) -> CheckResult {
    match sqlx::query("SELECT 1").execute(pool).await {
        Ok(_) => CheckResult::ok(),
        Err(e) => CheckResult::error(e.to_string()),
    }
}

/// Health check handler.
///
/// Returns 200 with health status. Individual check failures result in
/// degraded status rather than a hard failure.
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut checks = HashMap::new();

    // Check database
    let db_check = check_database(&state.db_pool).await;
    checks.insert("database".to_string(), db_check);

    // TODO: Add K8s health check when moto-club-k8s is implemented
    // TODO: Add Keybox health check when keybox integration is implemented

    // Determine overall status
    let status = if checks.values().all(CheckResult::is_ok) {
        HealthStatus::Healthy
    } else {
        HealthStatus::Degraded
    };

    let response = HealthResponse { status, checks };

    (StatusCode::OK, Json(response))
}

/// Server info response.
#[derive(Debug, Clone, Serialize)]
pub struct InfoResponse {
    /// Server name.
    pub name: &'static str,
    /// Server version.
    pub version: &'static str,
    /// API version.
    pub api_version: &'static str,
    /// Build git commit (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<&'static str>,
    /// Feature flags.
    pub features: InfoFeatures,
}

/// Feature flags for the server info response.
#[derive(Debug, Clone, Serialize)]
pub struct InfoFeatures {
    /// Whether WebSocket streaming is enabled.
    pub websocket: bool,
    /// Number of DERP regions available.
    pub derp_regions: u32,
}

/// Server info handler.
#[allow(clippy::cast_possible_truncation)] // DERP region count won't exceed u32::MAX
async fn info_handler(State(state): State<AppState>) -> impl IntoResponse {
    // Count DERP regions from the DERP manager
    let derp_regions = state
        .derp_manager
        .region_count()
        .map(|c| c as u32)
        .unwrap_or(0);

    let response = InfoResponse {
        name: "moto-club",
        version: env!("CARGO_PKG_VERSION"),
        api_version: "v1",
        git_sha: option_env!("GIT_SHA"),
        features: InfoFeatures {
            websocket: false, // WebSocket streaming deferred to future version
            derp_regions,
        },
    };

    (StatusCode::OK, Json(response))
}

/// Creates the health router.
///
/// Includes:
/// - `GET /health` - Health check endpoint
/// - `GET /api/v1/info` - Server info endpoint
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/v1/info", get(info_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_result_ok() {
        let result = CheckResult::ok();
        assert!(result.is_ok());

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""status":"ok""#));
    }

    #[test]
    fn check_result_error() {
        let result = CheckResult::error("connection refused");
        assert!(!result.is_ok());

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""status":"error""#));
        assert!(json.contains(r#""error":"connection refused""#));
    }

    #[test]
    fn health_status_serialization() {
        assert_eq!(
            serde_json::to_string(&HealthStatus::Healthy).unwrap(),
            r#""healthy""#
        );
        assert_eq!(
            serde_json::to_string(&HealthStatus::Degraded).unwrap(),
            r#""degraded""#
        );
    }

    #[test]
    fn health_response_serialization() {
        let mut checks = HashMap::new();
        checks.insert("database".to_string(), CheckResult::ok());
        checks.insert(
            "keybox".to_string(),
            CheckResult::error("connection refused"),
        );

        let response = HealthResponse {
            status: HealthStatus::Degraded,
            checks,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""status":"degraded""#));
        assert!(json.contains(r#""database""#));
        assert!(json.contains(r#""keybox""#));
    }

    #[test]
    fn info_response_serialization() {
        let response = InfoResponse {
            name: "moto-club",
            version: "0.1.0",
            api_version: "v1",
            git_sha: Some("abc1234"),
            features: InfoFeatures {
                websocket: false,
                derp_regions: 1,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""name":"moto-club""#));
        assert!(json.contains(r#""version":"0.1.0""#));
        assert!(json.contains(r#""api_version":"v1""#));
        assert!(json.contains(r#""git_sha":"abc1234""#));
        assert!(json.contains(r#""websocket":false"#));
        assert!(json.contains(r#""derp_regions":1"#));
    }

    #[test]
    fn info_response_without_git_sha() {
        let response = InfoResponse {
            name: "moto-club",
            version: "0.1.0",
            api_version: "v1",
            git_sha: None,
            features: InfoFeatures {
                websocket: false,
                derp_regions: 0,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""name":"moto-club""#));
        assert!(!json.contains("git_sha")); // Should be omitted when None
    }
}
