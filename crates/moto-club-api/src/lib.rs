//! REST API handlers for moto-club.
//!
//! This crate provides the HTTP API layer for moto-club, including:
//! - Health check endpoints (`/health`, `/api/v1/info`)
//! - Garage management endpoints (`/api/v1/garages/*`)
//! - `WireGuard` coordination endpoints (`/api/v1/wg/*`)
//! - Peer streaming WebSocket (`/internal/wg/garages/{id}/peers`)
//!
//! # Example
//!
//! ```ignore
//! use moto_club_api::{AppState, router};
//! use moto_club_db::DbPool;
//! use moto_club_wg::{PeerRegistry, InMemoryPeerStore, Ipam, InMemoryStore, SessionManager,
//!     InMemorySessionStore, DerpMapManager, InMemoryDerpStore, SshKeyManager, InMemorySshKeyStore,
//!     PeerBroadcaster};
//! use std::sync::Arc;
//!
//! let pool = DbPool::connect("postgres://...").await?;
//! let peer_registry = Arc::new(PeerRegistry::new(
//!     InMemoryPeerStore::new(),
//!     Ipam::new(InMemoryStore::new()),
//! ));
//! let session_manager = Arc::new(SessionManager::new(InMemorySessionStore::new()));
//! let derp_manager = Arc::new(DerpMapManager::new(InMemoryDerpStore::with_default_map()));
//! let ssh_key_manager = Arc::new(SshKeyManager::new(InMemorySshKeyStore::new()));
//! let peer_broadcaster = Arc::new(PeerBroadcaster::new());
//! let state = AppState::new(pool, peer_registry, session_manager, derp_manager, ssh_key_manager, peer_broadcaster);
//! let app = router(state);
//!
//! // Run with axum
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//! axum::serve(listener, app).await?;
//! ```

pub mod garages;
pub mod health;
pub mod postgres_stores;
pub mod wg;

use std::sync::Arc;

use axum::Router;
use moto_club_db::DbPool;
use moto_club_wg::{
    DerpMapManager, InMemoryDerpStore, InMemoryPeerStore, InMemorySessionStore,
    InMemorySshKeyStore, InMemoryStore, PeerBroadcaster, PeerRegistry, SessionManager,
    SshKeyManager, ipam::Ipam,
};
use moto_k8s::K8sClient;

pub use postgres_stores::{PostgresPeerStore, PostgresSessionStore, PostgresSshKeyStore};

/// Type alias for the peer registry used with in-memory storage (for testing).
pub type InMemoryWgPeerRegistry = PeerRegistry<InMemoryPeerStore, InMemoryStore>;

/// Type alias for the session manager used with in-memory storage (for testing).
pub type InMemoryWgSessionManager = SessionManager<InMemorySessionStore>;

/// Type alias for the SSH key manager used with in-memory storage (for testing).
pub type InMemoryWgSshKeyManager = SshKeyManager<InMemorySshKeyStore>;

/// Type alias for the peer registry used with PostgreSQL storage (for production).
pub type PostgresWgPeerRegistry = PeerRegistry<PostgresPeerStore, InMemoryStore>;

/// Type alias for the session manager used with PostgreSQL storage (for production).
pub type PostgresWgSessionManager = SessionManager<PostgresSessionStore>;

/// Type alias for the SSH key manager used with PostgreSQL storage (for production).
pub type PostgresWgSshKeyManager = SshKeyManager<PostgresSshKeyStore>;

/// Type alias for the peer registry used in production.
/// Currently defaults to in-memory; will be switched to PostgreSQL when fully wired.
pub type WgPeerRegistry = PeerRegistry<InMemoryPeerStore, InMemoryStore>;

/// Type alias for the session manager used in production.
/// Currently defaults to in-memory; will be switched to PostgreSQL when fully wired.
pub type WgSessionManager = SessionManager<InMemorySessionStore>;

/// Type alias for the DERP map manager used in production.
pub type WgDerpMapManager = DerpMapManager<InMemoryDerpStore>;

/// Type alias for the SSH key manager used in production.
/// Currently defaults to in-memory; will be switched to PostgreSQL when fully wired.
pub type WgSshKeyManager = SshKeyManager<InMemorySshKeyStore>;

/// Shared application state.
///
/// Contains all dependencies needed by API handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub db_pool: DbPool,
    /// `WireGuard` peer registry for device and garage registration.
    pub peer_registry: Arc<WgPeerRegistry>,
    /// `WireGuard` session manager for tunnel sessions.
    pub session_manager: Arc<WgSessionManager>,
    /// DERP map manager for relay server configuration.
    pub derp_manager: Arc<WgDerpMapManager>,
    /// SSH key manager for user key registration.
    pub ssh_key_manager: Arc<WgSshKeyManager>,
    /// Peer event broadcaster for garage WebSocket connections.
    pub peer_broadcaster: Arc<PeerBroadcaster>,
    /// Kubernetes client for ServiceAccount token validation.
    /// When `None`, token validation is skipped (for testing/local dev).
    pub k8s_client: Option<K8sClient>,
}

impl AppState {
    /// Creates a new `AppState` with the given dependencies.
    #[must_use]
    pub const fn new(
        db_pool: DbPool,
        peer_registry: Arc<WgPeerRegistry>,
        session_manager: Arc<WgSessionManager>,
        derp_manager: Arc<WgDerpMapManager>,
        ssh_key_manager: Arc<WgSshKeyManager>,
        peer_broadcaster: Arc<PeerBroadcaster>,
    ) -> Self {
        Self {
            db_pool,
            peer_registry,
            session_manager,
            derp_manager,
            ssh_key_manager,
            peer_broadcaster,
            k8s_client: None,
        }
    }

    /// Creates a new `AppState` with a K8s client for token validation.
    #[must_use]
    pub fn with_k8s_client(mut self, k8s_client: K8sClient) -> Self {
        self.k8s_client = Some(k8s_client);
        self
    }

    /// Creates a new `AppState` with in-memory storage (for testing).
    ///
    /// This uses in-memory stores that don't persist data across restarts.
    /// K8s token validation is disabled (k8s_client is None).
    #[must_use]
    pub fn with_in_memory_storage(db_pool: DbPool) -> Self {
        let ipam_store = InMemoryStore::new();
        let peer_store = InMemoryPeerStore::new();
        let session_store = InMemorySessionStore::new();
        let derp_store = InMemoryDerpStore::with_default_map();
        let ssh_key_store = InMemorySshKeyStore::new();

        let ipam = Ipam::new(ipam_store);
        let peer_registry = Arc::new(PeerRegistry::new(peer_store, ipam));
        let session_manager = Arc::new(SessionManager::new(session_store));
        let derp_manager = Arc::new(DerpMapManager::new(derp_store));
        let ssh_key_manager = Arc::new(SshKeyManager::new(ssh_key_store));
        let peer_broadcaster = Arc::new(PeerBroadcaster::new());

        Self {
            db_pool,
            peer_registry,
            session_manager,
            derp_manager,
            ssh_key_manager,
            peer_broadcaster,
            k8s_client: None,
        }
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
    /// Invalid SSH public key.
    pub const INVALID_SSH_KEY: &str = "INVALID_SSH_KEY";
    /// K8s ServiceAccount token is invalid or expired.
    pub const INVALID_TOKEN: &str = "INVALID_TOKEN";
    /// Pod not running in expected garage namespace.
    pub const NAMESPACE_MISMATCH: &str = "NAMESPACE_MISMATCH";
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
