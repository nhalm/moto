//! Health check endpoints.
//!
//! Provides Kubernetes-style health check functionality per the Engine Contract (moto-bike.md):
//! - `/health/live` - Process is alive (not deadlocked), always 200 if reachable
//! - `/health/ready` - Ready for traffic (deps connected), checks database and keybox
//! - `/health/startup` - Initial startup complete
//!
//! These endpoints are served on a separate port (8081) from the main API (8080).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};

use crate::AppState;

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

/// Keybox health response (from `/health/ready` endpoint).
#[derive(Debug, Clone, Deserialize)]
struct KeyboxHealthResponse {
    /// Status: `"ok"` or `"not_ready"`.
    status: String,
}

/// Timeout for keybox health check requests.
const KEYBOX_HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

/// Check keybox health by calling its `/health/ready` endpoint.
///
/// Returns `CheckResult::ok()` if keybox responds with 200 and status "ok".
/// Returns `CheckResult::error()` with details if:
/// - Connection fails (unreachable)
/// - Response is not 200
/// - Response status is not "ok"
async fn check_keybox(keybox_url: &str) -> CheckResult {
    let client = match reqwest::Client::builder()
        .timeout(KEYBOX_HEALTH_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => return CheckResult::error(format!("failed to create HTTP client: {e}")),
    };

    // Keybox health endpoint is on port 8081 per moto-bike.md Engine Contract
    // The keybox_url is the base URL (e.g., http://keybox:8080), but health is on 8081
    // Parse the URL to construct the proper health endpoint
    // e.g., http://keybox:8080 -> http://keybox:8081/health/ready
    #[allow(clippy::option_if_let_else)]
    let health_url = match url::Url::parse(keybox_url) {
        Ok(mut url) => {
            url.set_port(Some(8081)).ok();
            url.set_path("/health/ready");
            url.to_string()
        }
        Err(_) => {
            // Fallback: just append port and path
            format!("{keybox_url}:8081/health/ready")
        }
    };

    let response = match client.get(&health_url).send().await {
        Ok(r) => r,
        Err(e) => {
            if e.is_connect() || e.is_timeout() {
                return CheckResult::error(format!("connection failed: {e}"));
            }
            return CheckResult::error(format!("request failed: {e}"));
        }
    };

    if !response.status().is_success() {
        return CheckResult::error(format!("HTTP {}", response.status()));
    }

    match response.json::<KeyboxHealthResponse>().await {
        Ok(body) => {
            if body.status == "ok" {
                CheckResult::ok()
            } else {
                CheckResult::error(format!("status: {}", body.status))
            }
        }
        Err(e) => CheckResult::error(format!("invalid response: {e}")),
    }
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
    /// "ok" if all dependencies are connected, "`not_ready`" otherwise.
    pub status: &'static str,
    /// Individual dependency check results (only included if not ready).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checks: Option<HashMap<String, CheckResult>>,
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
/// Returns 200 if the service is ready to accept traffic (all dependencies connected).
/// Returns 503 if dependencies are not ready.
///
/// Per moto-club.md v1.5, checks:
/// - Database connectivity
/// - Keybox `/health/ready` endpoint (returns degraded if unreachable)
async fn ready_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut checks = HashMap::new();

    // Check database
    let db_check = check_database(&state.db_pool).await;
    let db_ready = db_check.is_ok();
    checks.insert("database".to_string(), db_check);

    // Check keybox if URL is configured
    let keybox_ready = if let Some(ref keybox_url) = state.keybox_url {
        let keybox_check = check_keybox(keybox_url).await;
        let is_ok = keybox_check.is_ok();
        checks.insert("keybox".to_string(), keybox_check);
        is_ok
    } else {
        // Keybox not configured (local dev mode), consider it ready
        checks.insert("keybox".to_string(), CheckResult::ok());
        true
    };

    // Overall ready status: database must be ready, keybox failure degrades but doesn't fail
    // Per spec: "return degraded status if keybox unreachable"
    if db_ready {
        if keybox_ready {
            let response = ReadyResponse {
                status: "ok",
                checks: None,
            };
            (StatusCode::OK, Json(response))
        } else {
            // Database is ready but keybox is not - degraded state
            // Still return 200 since we can serve requests, but include checks
            let response = ReadyResponse {
                status: "degraded",
                checks: Some(checks),
            };
            (StatusCode::OK, Json(response))
        }
    } else {
        let response = ReadyResponse {
            status: "not_ready",
            checks: Some(checks),
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

/// Check Kubernetes API connectivity.
async fn check_k8s(client: &moto_k8s::K8sClient) -> CheckResult {
    use moto_k8s::NamespaceOps;
    // Try to list namespaces with a label selector that won't match anything.
    // This validates API connectivity without loading large result sets.
    match client
        .list_namespaces(Some("moto.dev/health-check=true"))
        .await
    {
        Ok(_) => CheckResult::ok(),
        Err(e) => CheckResult::error(e.to_string()),
    }
}

/// Health check handler (legacy - combines all checks).
///
/// Returns 200 with health status. Individual check failures result in
/// degraded status rather than a hard failure.
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut checks = HashMap::new();

    // Check database
    let db_check = check_database(&state.db_pool).await;
    checks.insert("database".to_string(), db_check);

    // Check K8s if client is available
    if let Some(ref k8s_client) = state.k8s_client {
        let k8s_check = check_k8s(k8s_client).await;
        checks.insert("k8s".to_string(), k8s_check);
    } else {
        // K8s client not configured (local dev mode), report as ok
        checks.insert("k8s".to_string(), CheckResult::ok());
    }

    // Check keybox health (per moto-club.md v1.5)
    if let Some(ref keybox_url) = state.keybox_url {
        let keybox_check = check_keybox(keybox_url).await;
        checks.insert("keybox".to_string(), keybox_check);
    } else {
        // Keybox not configured (local dev mode), report as ok
        checks.insert("keybox".to_string(), CheckResult::ok());
    }

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
    // Count DERP regions from the static DERP map
    let derp_regions = state.derp_map.len() as u32;

    let response = InfoResponse {
        name: "moto-club",
        version: env!("CARGO_PKG_VERSION"),
        api_version: "v1",
        git_sha: option_env!("GIT_SHA"),
        features: InfoFeatures {
            websocket: true, // WebSocket peer streaming implemented (WS /internal/wg/garages/{id}/peers)
            derp_regions,
        },
    };

    (StatusCode::OK, Json(response))
}

/// Creates the health router for the main API server (port 8080).
///
/// Includes:
/// - `GET /health` - Legacy health check endpoint (for backwards compatibility)
/// - `GET /api/v1/info` - Server info endpoint
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/v1/info", get(info_handler))
}

/// Creates the health server router for port 8081.
///
/// Per the Engine Contract (moto-bike.md), health endpoints are served on a separate port:
/// - `GET /health/live` - Liveness probe (process is alive)
/// - `GET /health/ready` - Readiness probe (dependencies connected)
/// - `GET /health/startup` - Startup probe (initial startup complete)
pub fn health_server_router() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(live_handler))
        .route("/health/ready", get(ready_handler))
        .route("/health/startup", get(startup_handler))
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
                websocket: true,
                derp_regions: 1,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""name":"moto-club""#));
        assert!(json.contains(r#""version":"0.1.0""#));
        assert!(json.contains(r#""api_version":"v1""#));
        assert!(json.contains(r#""git_sha":"abc1234""#));
        assert!(json.contains(r#""websocket":true"#));
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
                websocket: true,
                derp_regions: 0,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""name":"moto-club""#));
        assert!(!json.contains("git_sha")); // Should be omitted when None
    }

    #[test]
    fn live_response_serialization() {
        let response = LiveResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
    }

    #[test]
    fn ready_response_ok_serialization() {
        let response = ReadyResponse {
            status: "ok",
            checks: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);
        assert!(!json.contains("checks")); // Should be omitted when None
    }

    #[test]
    fn ready_response_not_ready_serialization() {
        let mut checks = HashMap::new();
        checks.insert(
            "database".to_string(),
            CheckResult::error("connection refused"),
        );

        let response = ReadyResponse {
            status: "not_ready",
            checks: Some(checks),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""status":"not_ready""#));
        assert!(json.contains(r#""checks""#));
        assert!(json.contains(r#""database""#));
    }

    #[test]
    fn startup_response_serialization() {
        let response = StartupResponse { status: "ok" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"ok"}"#);

        let response = StartupResponse { status: "starting" };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"status":"starting"}"#);
    }

    #[test]
    fn health_response_with_all_checks() {
        // Per spec (moto-club.md), health response should include database, k8s, and keybox checks
        let mut checks = HashMap::new();
        checks.insert("database".to_string(), CheckResult::ok());
        checks.insert("k8s".to_string(), CheckResult::ok());
        checks.insert("keybox".to_string(), CheckResult::ok());

        let response = HealthResponse {
            status: HealthStatus::Healthy,
            checks,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""status":"healthy""#));
        assert!(json.contains(r#""database""#));
        assert!(json.contains(r#""k8s""#));
        assert!(json.contains(r#""keybox""#));
    }

    #[test]
    fn ready_response_degraded_serialization() {
        // Per spec v1.5: ready returns degraded if keybox unreachable but DB is ok
        let mut checks = HashMap::new();
        checks.insert("database".to_string(), CheckResult::ok());
        checks.insert(
            "keybox".to_string(),
            CheckResult::error("connection failed"),
        );

        let response = ReadyResponse {
            status: "degraded",
            checks: Some(checks),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""status":"degraded""#));
        assert!(json.contains(r#""database""#));
        assert!(json.contains(r#""keybox""#));
    }

    #[test]
    fn keybox_url_parsing() {
        // Test URL construction for keybox health endpoint
        let keybox_url = "http://keybox:8080";
        let health_url = url::Url::parse(keybox_url).map_or_else(
            |_| format!("{keybox_url}:8081/health/ready"),
            |mut url| {
                url.set_port(Some(8081)).ok();
                url.set_path("/health/ready");
                url.to_string()
            },
        );

        assert_eq!(health_url, "http://keybox:8081/health/ready");
    }

    #[test]
    fn keybox_url_parsing_with_port() {
        // Test URL construction when keybox URL already has a port
        let keybox_url = "http://keybox:8080";
        let health_url = url::Url::parse(keybox_url).map_or_else(
            |_| format!("{keybox_url}:8081/health/ready"),
            |mut url| {
                url.set_port(Some(8081)).ok();
                url.set_path("/health/ready");
                url.to_string()
            },
        );

        // The port 8080 should be replaced with 8081
        assert_eq!(health_url, "http://keybox:8081/health/ready");
    }

    #[test]
    fn keybox_health_response_deserialization() {
        let json = r#"{"status":"ok"}"#;
        let response: KeyboxHealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status, "ok");

        let json = r#"{"status":"not_ready"}"#;
        let response: KeyboxHealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.status, "not_ready");
    }
}
