//! REST API handlers for moto-club.
//!
//! This crate provides the HTTP API layer for moto-club, including:
//! - Health check endpoints (`/health`, `/api/v1/info`) on main API port 8080
//! - K8s health probes (`/health/live`, `/health/ready`, `/health/startup`) on port 8081
//! - Garage management endpoints (`/api/v1/garages/*`)
//! - `WireGuard` coordination endpoints (`/api/v1/wg/*`)
//! - Peer streaming WebSocket (`/internal/wg/garages/{id}/peers`)
//!
//! # Example
//!
//! ```ignore
//! use moto_club_api::{AppState, router, PostgresIpamStore, PostgresPeerStore, PostgresSessionStore};
//! use moto_club_db::DbPool;
//! use moto_club_wg::{PeerRegistry, Ipam, SessionManager, PeerBroadcaster, parse_derp_servers_env};
//! use std::sync::Arc;
//!
//! let pool = DbPool::connect("postgres://...").await?;
//! let ipam_store = PostgresIpamStore::new(pool.clone());
//! let peer_store = PostgresPeerStore::new(pool.clone());
//! let session_store = PostgresSessionStore::new(pool.clone());
//! let peer_registry = Arc::new(PeerRegistry::new(peer_store, Ipam::new(ipam_store)));
//! let session_manager = Arc::new(SessionManager::new(session_store));
//! let derp_map = parse_derp_servers_env().unwrap().to_derp_map();
//! let peer_broadcaster = Arc::new(PeerBroadcaster::new());
//! let state = AppState::new(pool, peer_registry, session_manager, derp_map, peer_broadcaster);
//! let app = router(state);
//!
//! // Run with axum
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//! axum::serve(listener, app).await?;
//! ```

pub mod audit;
pub mod events_ws;
pub mod garages;
pub mod health;
pub mod logs_ws;
pub mod metrics;
pub mod postgres_stores;
pub mod wg;

use std::sync::Arc;

use axum::{Router, middleware};
use moto_club_db::DbPool;
use moto_club_garage::GarageService;
use moto_club_k8s::GarageK8s;
use moto_club_wg::{PeerBroadcaster, PeerRegistry, SessionManager};
use moto_club_ws::{ConnectionTracker, EventBroadcaster};
use moto_k8s::K8sClient;
use moto_wgtunnel_types::derp::DerpMap;

pub use health::{health_server_router, is_startup_complete, mark_startup_complete};
pub use postgres_stores::{PostgresIpamStore, PostgresPeerStore, PostgresSessionStore};

/// Type alias for the peer registry used with `PostgreSQL` storage.
pub type WgPeerRegistry = PeerRegistry<PostgresPeerStore, PostgresIpamStore>;

/// Type alias for the session manager used with `PostgreSQL` storage.
pub type WgSessionManager = SessionManager<PostgresSessionStore>;

/// Shared application state using `PostgreSQL` storage backends.
///
/// Contains all dependencies needed by API handlers.
/// Uses PostgreSQL-backed stores for peer registry and session management.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub db_pool: DbPool,
    /// `WireGuard` peer registry for device and garage registration.
    pub peer_registry: Arc<WgPeerRegistry>,
    /// `WireGuard` session manager for tunnel sessions.
    pub session_manager: Arc<WgSessionManager>,
    /// DERP map for relay server configuration (static per deployment).
    pub derp_map: Arc<DerpMap>,
    /// Peer event broadcaster for garage WebSocket connections.
    pub peer_broadcaster: Arc<PeerBroadcaster>,
    /// Garage event broadcaster for event streaming WebSocket connections.
    pub event_broadcaster: Arc<EventBroadcaster>,
    /// Kubernetes client for `ServiceAccount` token validation.
    /// When `None`, token validation is skipped (for testing/local dev).
    pub k8s_client: Option<K8sClient>,
    /// Garage K8s operations for namespace and pod management.
    /// When `None`, K8s operations are skipped (for testing/local dev).
    pub garage_k8s: Option<GarageK8s>,
    /// Garage service for garage lifecycle management with full K8s integration.
    /// When `None`, garage create only writes to database (no K8s resources).
    pub garage_service: Option<GarageService>,
    /// Keybox health URL for health checks (e.g., `http://keybox:8081`).
    /// When `None`, keybox health check is skipped (for testing/local dev).
    pub keybox_health_url: Option<String>,
    /// Keybox API URL for audit log fan-out queries (e.g., `http://keybox:8080`).
    /// When `None`, audit fan-out to keybox is skipped.
    pub keybox_url: Option<String>,
    /// Keybox service token for authenticated API calls (audit fan-out).
    pub keybox_service_token: Option<String>,
    /// Connection tracker for log streaming WebSocket connections (per garage, max 5).
    pub log_connection_tracker: Arc<ConnectionTracker>,
    /// Connection tracker for event streaming WebSocket connections (per user, max 3).
    pub event_connection_tracker: Arc<ConnectionTracker>,
    /// Service token for admin authentication (static shared token).
    /// When `None`, service token validation is not available.
    pub service_token: Option<String>,
}

impl AppState {
    /// Creates a new `AppState` with the given dependencies.
    #[must_use]
    pub fn new(
        db_pool: DbPool,
        peer_registry: Arc<WgPeerRegistry>,
        session_manager: Arc<WgSessionManager>,
        derp_map: DerpMap,
        peer_broadcaster: Arc<PeerBroadcaster>,
    ) -> Self {
        Self {
            db_pool,
            peer_registry,
            session_manager,
            derp_map: Arc::new(derp_map),
            peer_broadcaster,
            event_broadcaster: Arc::new(EventBroadcaster::new()),
            k8s_client: None,
            garage_k8s: None,
            garage_service: None,
            keybox_health_url: None,
            keybox_url: None,
            keybox_service_token: None,
            log_connection_tracker: Arc::new(ConnectionTracker::new()),
            event_connection_tracker: Arc::new(ConnectionTracker::new()),
            service_token: None,
        }
    }

    /// Sets the event broadcaster (shared with the reconciler for TTL warnings).
    #[must_use]
    pub fn with_event_broadcaster(mut self, event_broadcaster: Arc<EventBroadcaster>) -> Self {
        self.event_broadcaster = event_broadcaster;
        self
    }

    /// Creates a new `AppState` with a K8s client for token validation.
    #[must_use]
    pub fn with_k8s_client(mut self, k8s_client: K8sClient) -> Self {
        self.k8s_client = Some(k8s_client);
        self
    }

    /// Creates a new `AppState` with a garage K8s client for namespace operations.
    #[must_use]
    pub fn with_garage_k8s(mut self, garage_k8s: GarageK8s) -> Self {
        self.garage_k8s = Some(garage_k8s);
        self
    }

    /// Creates a new `AppState` with a garage service for full K8s integration.
    #[must_use]
    pub fn with_garage_service(mut self, garage_service: GarageService) -> Self {
        self.garage_service = Some(garage_service);
        self
    }

    /// Creates a new `AppState` with a keybox health URL for health checks.
    #[must_use]
    pub fn with_keybox_health_url(mut self, keybox_health_url: String) -> Self {
        self.keybox_health_url = Some(keybox_health_url);
        self
    }

    /// Sets the keybox API URL for audit log fan-out queries.
    #[must_use]
    pub fn with_keybox_url(mut self, url: String) -> Self {
        self.keybox_url = Some(url);
        self
    }

    /// Sets the keybox service token for authenticated audit fan-out.
    #[must_use]
    pub fn with_keybox_service_token(mut self, token: String) -> Self {
        self.keybox_service_token = Some(token);
        self
    }

    /// Sets the service token for admin authentication.
    #[must_use]
    pub fn with_service_token(mut self, token: impl Into<String>) -> Self {
        self.service_token = Some(token.into());
        self
    }

    /// Get the DERP map.
    ///
    /// DERP config is static per deployment (loaded from env var at startup).
    #[must_use]
    pub fn get_derp_map(&self) -> DerpMap {
        (*self.derp_map).clone()
    }
}

use moto_club_wg::{RegisteredDevice, Session};
use moto_club_ws::logs::{GarageInfo, LogStreamError};
use moto_club_ws::{EventStreamingContext, LogStreamingContext, PeerStreamingContext};
use moto_k8s::{LogStream, PodLogOptions, PodOps};
use moto_wgtunnel_types::WgPublicKey;

impl LogStreamingContext for AppState {
    async fn resolve_garage(&self, name: &str, owner: &str) -> Result<GarageInfo, LogStreamError> {
        use moto_club_db::garage_repo;

        let garage = garage_repo::get_by_name(&self.db_pool, name)
            .await
            .map_err(|e| match e {
                moto_club_db::DbError::NotFound { .. } => {
                    LogStreamError::NotFound(format!("garage '{name}' not found"))
                }
                _ => LogStreamError::Internal(format!("database error: {e}")),
            })?;

        if garage.owner != owner {
            return Err(LogStreamError::NotOwned(format!(
                "garage '{name}' is owned by another user"
            )));
        }

        Ok(GarageInfo {
            namespace: garage.namespace,
            status: garage.status.to_string(),
        })
    }

    async fn stream_pod_logs(
        &self,
        namespace: &str,
        options: &PodLogOptions,
    ) -> Result<LogStream, LogStreamError> {
        let Some(ref garage_k8s) = self.garage_k8s else {
            return Err(LogStreamError::Internal(
                "K8s client not configured".to_string(),
            ));
        };

        garage_k8s
            .client()
            .stream_pod_logs(namespace, None, options)
            .await
            .map_err(|e| LogStreamError::Kubernetes(e.to_string()))
    }
}

impl PeerStreamingContext for AppState {
    fn list_sessions_for_garage(&self, garage_id: &str) -> Result<Vec<Session>, String> {
        self.session_manager
            .list_sessions_for_garage(garage_id)
            .map_err(|e| e.to_string())
    }

    fn get_device(&self, pubkey: &WgPublicKey) -> Result<Option<RegisteredDevice>, String> {
        self.peer_registry
            .get_device(pubkey)
            .map_err(|e| e.to_string())
    }

    fn peer_broadcaster(&self) -> Arc<PeerBroadcaster> {
        Arc::clone(&self.peer_broadcaster)
    }
}

impl EventStreamingContext for AppState {
    async fn list_owned_garage_names(&self, owner: &str) -> Result<Vec<String>, String> {
        use moto_club_db::garage_repo;

        let garages = garage_repo::list_by_owner(&self.db_pool, owner, false)
            .await
            .map_err(|e| format!("database error: {e}"))?;

        Ok(garages.into_iter().map(|g| g.name).collect())
    }

    fn event_broadcaster(&self) -> Arc<EventBroadcaster> {
        Arc::clone(&self.event_broadcaster)
    }
}

/// Creates the main API router with all routes.
///
/// The router includes:
/// - Health endpoints from [`health::router()`]
/// - Garage endpoints from [`garages::router()`]
/// - `WireGuard` endpoints from [`wg::router()`]
/// - HTTP metrics middleware (records `http_requests_total` and `http_request_duration_seconds`)
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(garages::router())
        .merge(audit::router())
        .merge(logs_ws::router())
        .merge(events_ws::router())
        .merge(wg::router())
        .layer(middleware::from_fn(metrics::record_http_metrics))
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
    /// Unknown status value in filter.
    pub const INVALID_STATUS: &str = "INVALID_STATUS";
    /// `WireGuard` device not found.
    pub const DEVICE_NOT_FOUND: &str = "DEVICE_NOT_FOUND";
    /// Device (public key) belongs to different user.
    pub const DEVICE_NOT_OWNED: &str = "DEVICE_NOT_OWNED";
    /// `WireGuard` session not found.
    pub const SESSION_NOT_FOUND: &str = "SESSION_NOT_FOUND";
    /// Session belongs to different user.
    pub const SESSION_NOT_OWNED: &str = "SESSION_NOT_OWNED";
    /// Garage hasn't registered its `WireGuard` endpoint yet.
    pub const GARAGE_NOT_REGISTERED: &str = "GARAGE_NOT_REGISTERED";
    /// Internal server error.
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    /// Kubernetes API error.
    pub const K8S_ERROR: &str = "K8S_ERROR";
    /// Database connection error.
    pub const DATABASE_ERROR: &str = "DATABASE_ERROR";
    /// K8s `ServiceAccount` token is invalid or expired.
    pub const INVALID_TOKEN: &str = "INVALID_TOKEN";
    /// Pod not running in expected garage namespace.
    pub const NAMESPACE_MISMATCH: &str = "NAMESPACE_MISMATCH";
    /// Missing or invalid Authorization header.
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    /// Operation requires service token.
    pub const FORBIDDEN: &str = "FORBIDDEN";
    /// Service token not configured on server.
    pub const SERVICE_TOKEN_NOT_CONFIGURED: &str = "SERVICE_TOKEN_NOT_CONFIGURED";
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
