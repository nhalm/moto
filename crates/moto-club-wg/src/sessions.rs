//! Tunnel session management for `WireGuard` coordination.
//!
//! This module handles tunnel sessions that connect devices to garages:
//!
//! - **Create session:** Client requests access to a garage
//! - **List sessions:** Show active sessions for a user
//! - **Close session:** End a session explicitly
//! - **Session TTL:** Sessions expire automatically
//!
//! # Architecture
//!
//! Sessions are created by the CLI when entering a garage. The device is identified
//! by its WireGuard public key (Cloudflare WARP model):
//!
//! ```text
//! Session Creation:
//!   POST /api/v1/wg/sessions { garage_id, device_pubkey, ttl_seconds }
//!   → { session_id, garage { public_key, overlay_ip, endpoints }, client_ip, derp_map, expires_at }
//!
//! List Sessions:
//!   GET /api/v1/wg/sessions
//!   → { sessions: [...] }
//!
//! Close Session:
//!   DELETE /api/v1/wg/sessions/{session_id}
//!   → 204 No Content
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::sessions::{SessionManager, InMemorySessionStore, CreateSessionRequest};
//! use moto_club_wg::peers::{PeerRegistry, InMemoryPeerStore, DeviceRegistration, GarageRegistration};
//! use moto_club_wg::ipam::{Ipam, InMemoryStore};
//! use moto_wgtunnel_types::keys::WgPrivateKey;
//! use moto_wgtunnel_types::derp::{DerpMap, DerpRegion, DerpNode};
//!
//! # tokio_test::block_on(async {
//! // Create stores and registry
//! let ipam_store = InMemoryStore::new();
//! let peer_store = InMemoryPeerStore::new();
//! let session_store = InMemorySessionStore::new();
//!
//! let ipam = Ipam::new(ipam_store);
//! let registry = PeerRegistry::new(peer_store, ipam);
//! let manager = SessionManager::new(session_store);
//!
//! // Register a device and garage
//! let device_key = WgPrivateKey::generate();
//! let device = registry.register_device(DeviceRegistration {
//!     public_key: device_key.public_key(),
//!     device_name: Some("laptop".to_string()),
//! }).await.unwrap();
//!
//! let garage_key = WgPrivateKey::generate();
//! let garage = registry.register_garage(GarageRegistration {
//!     garage_id: "feature-foo".to_string(),
//!     public_key: garage_key.public_key(),
//!     endpoints: vec![],
//! }).await.unwrap();
//!
//! // Create a session
//! let derp_map = DerpMap::new()
//!     .with_region(DerpRegion::new(1, "primary")
//!         .with_node(DerpNode::with_defaults("derp.example.com")));
//!
//! let request = CreateSessionRequest {
//!     garage_id: "feature-foo".to_string(),
//!     device_pubkey: device_key.public_key(),
//!     ttl_seconds: Some(3600),
//! };
//!
//! let session = manager.create_session(
//!     request,
//!     &device,
//!     &garage,
//!     &derp_map,
//! ).await.unwrap();
//!
//! assert!(session.session_id.starts_with("sess_"));
//! # });
//! ```

use chrono::{DateTime, Duration, Utc};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use uuid::Uuid;

use crate::peers::{RegisteredDevice, RegisteredGarage};

// Note: WgPublicKey is re-exported for use in Session type

/// Default session TTL in seconds (4 hours).
pub const DEFAULT_SESSION_TTL_SECS: u32 = 14400;

/// Grace period in seconds for disconnected clients (5 minutes).
pub const DISCONNECT_GRACE_PERIOD_SECS: u32 = 300;

/// Error type for session operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Session not found.
    #[error("session not found: {0}")]
    NotFound(String),

    /// Device not registered (identified by WireGuard public key).
    #[error("device not registered: {0}")]
    DeviceNotRegistered(String),

    /// Garage not registered.
    #[error("garage not registered: {0}")]
    GarageNotRegistered(String),
}

/// Result type for session operations.
pub type Result<T> = std::result::Result<T, SessionError>;

/// Request to create a tunnel session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    /// Garage to connect to.
    pub garage_id: String,

    /// Device requesting the connection (WireGuard public key IS the device identity).
    pub device_pubkey: WgPublicKey,

    /// Optional session TTL in seconds. Defaults to garage TTL or 4 hours.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u32>,
}

/// Response for session creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    /// Unique session identifier (prefixed with "sess_").
    pub session_id: String,

    /// Garage connection info.
    pub garage: GarageConnectionInfo,

    /// Client's overlay IP address.
    pub client_ip: OverlayIp,

    /// DERP relay map for fallback connections.
    pub derp_map: DerpMap,

    /// When this session expires.
    pub expires_at: DateTime<Utc>,
}

/// Garage connection information for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarageConnectionInfo {
    /// Garage's `WireGuard` public key.
    pub public_key: WgPublicKey,

    /// Garage's overlay IP address.
    pub overlay_ip: OverlayIp,

    /// Direct UDP endpoints for P2P connections.
    pub endpoints: Vec<SocketAddr>,
}

/// A tunnel session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (prefixed with "sess_").
    pub session_id: String,

    /// Garage this session connects to.
    pub garage_id: String,

    /// Human-readable garage name (same as `garage_id` for now).
    pub garage_name: String,

    /// Device that created this session (WireGuard public key IS the device identity).
    pub device_pubkey: WgPublicKey,

    /// When this session was created.
    pub created_at: DateTime<Utc>,

    /// When this session expires.
    pub expires_at: DateTime<Utc>,
}

impl Session {
    /// Check if this session has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Get the remaining TTL in seconds.
    ///
    /// Returns 0 if the session has expired.
    #[must_use]
    pub fn remaining_ttl_secs(&self) -> u32 {
        let remaining = self.expires_at - Utc::now();
        let secs = remaining.num_seconds();
        if secs > 0 {
            // Safe: secs is positive and we cap at u32::MAX
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let result = secs.min(i64::from(u32::MAX)) as u32;
            result
        } else {
            0
        }
    }
}

/// Storage backend for session manager.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait SessionStore: Send + Sync {
    /// Get a session by ID.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_session(&self, session_id: &str) -> Result<Option<Session>>;

    /// Store a session.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_session(&self, session: Session) -> Result<()>;

    /// Remove a session.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_session(&self, session_id: &str) -> Result<Option<Session>>;

    /// List all sessions for a device (by public key).
    ///
    /// The WireGuard public key IS the device identity.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn list_sessions_by_device(&self, device_pubkey: &WgPublicKey) -> Result<Vec<Session>>;

    /// List all sessions for a garage.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn list_sessions_by_garage(&self, garage_id: &str) -> Result<Vec<Session>>;

    /// Remove all sessions for a garage.
    ///
    /// Called when a garage terminates.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_sessions_by_garage(&self, garage_id: &str) -> Result<Vec<Session>>;

    /// Remove all expired sessions.
    ///
    /// Called by cleanup background job.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_expired_sessions(&self) -> Result<Vec<Session>>;
}

/// Session manager for creating and managing tunnel sessions.
pub struct SessionManager<S> {
    store: S,
}

impl<S: SessionStore> SessionManager<S> {
    /// Create a new session manager.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Create a new tunnel session.
    ///
    /// # Arguments
    ///
    /// - `request`: Session creation request
    /// - `device`: Registered device information
    /// - `garage`: Registered garage information
    /// - `derp_map`: DERP relay map for fallback
    ///
    /// # Errors
    ///
    /// Returns error if storage operations fail.
    #[allow(clippy::unused_async)] // Async for future database operations
    pub async fn create_session(
        &self,
        request: CreateSessionRequest,
        device: &RegisteredDevice,
        garage: &RegisteredGarage,
        derp_map: &DerpMap,
    ) -> Result<CreateSessionResponse> {
        let now = Utc::now();
        let ttl_secs = request.ttl_seconds.unwrap_or(DEFAULT_SESSION_TTL_SECS);
        let ttl = Duration::seconds(i64::from(ttl_secs));
        let expires_at = now + ttl;

        // Generate session ID
        let session_id = generate_session_id();

        let session = Session {
            session_id: session_id.clone(),
            garage_id: garage.garage_id.clone(),
            garage_name: garage.garage_id.clone(), // Same for now
            device_pubkey: device.public_key.clone(),
            created_at: now,
            expires_at,
        };

        self.store.set_session(session)?;

        Ok(CreateSessionResponse {
            session_id,
            garage: GarageConnectionInfo {
                public_key: garage.public_key.clone(),
                overlay_ip: garage.overlay_ip,
                endpoints: garage.endpoints.clone(),
            },
            client_ip: device.overlay_ip,
            derp_map: derp_map.clone(),
            expires_at,
        })
    }

    /// Get a session by ID.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.store.get_session(session_id)
    }

    /// Close a session.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails or session not found.
    pub fn close_session(&self, session_id: &str) -> Result<Session> {
        self.store
            .remove_session(session_id)?
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))
    }

    /// List sessions for a device (by public key).
    ///
    /// The WireGuard public key IS the device identity.
    /// Excludes expired sessions.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn list_sessions(&self, device_pubkey: &WgPublicKey) -> Result<Vec<Session>> {
        let sessions = self.store.list_sessions_by_device(device_pubkey)?;
        Ok(sessions.into_iter().filter(|s| !s.is_expired()).collect())
    }

    /// List sessions for a garage.
    ///
    /// Excludes expired sessions.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn list_sessions_for_garage(&self, garage_id: &str) -> Result<Vec<Session>> {
        let sessions = self.store.list_sessions_by_garage(garage_id)?;
        Ok(sessions.into_iter().filter(|s| !s.is_expired()).collect())
    }

    /// Handle garage termination by closing all its sessions.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn on_garage_terminated(&self, garage_id: &str) -> Result<Vec<Session>> {
        self.store.remove_sessions_by_garage(garage_id)
    }

    /// Clean up expired sessions.
    ///
    /// Should be called periodically by a background job.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn cleanup_expired(&self) -> Result<Vec<Session>> {
        self.store.remove_expired_sessions()
    }
}

/// Generate a unique session ID with "sess_" prefix.
fn generate_session_id() -> String {
    let uuid = Uuid::now_v7();
    // Use simple hex encoding of UUID for compactness
    format!("sess_{}", uuid.simple())
}

/// In-memory session store for testing.
///
/// Sessions are lost when the store is dropped.
pub struct InMemorySessionStore {
    inner: Mutex<InMemorySessionStoreInner>,
}

struct InMemorySessionStoreInner {
    sessions: HashMap<String, Session>,
}

impl InMemorySessionStore {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemorySessionStoreInner {
                sessions: HashMap::new(),
            }),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for InMemorySessionStore {
    fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.sessions.get(session_id).cloned())
    }

    fn set_session(&self, session: Session) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .insert(session.session_id.clone(), session);
        Ok(())
    }

    fn remove_session(&self, session_id: &str) -> Result<Option<Session>> {
        Ok(self.inner.lock().unwrap().sessions.remove(session_id))
    }

    fn list_sessions_by_device(&self, device_pubkey: &WgPublicKey) -> Result<Vec<Session>> {
        let sessions = self
            .inner
            .lock()
            .unwrap()
            .sessions
            .values()
            .filter(|s| s.device_pubkey == *device_pubkey)
            .cloned()
            .collect();
        Ok(sessions)
    }

    fn list_sessions_by_garage(&self, garage_id: &str) -> Result<Vec<Session>> {
        let sessions = self
            .inner
            .lock()
            .unwrap()
            .sessions
            .values()
            .filter(|s| s.garage_id == garage_id)
            .cloned()
            .collect();
        Ok(sessions)
    }

    fn remove_sessions_by_garage(&self, garage_id: &str) -> Result<Vec<Session>> {
        let mut inner = self.inner.lock().unwrap();
        let session_ids: Vec<_> = inner
            .sessions
            .values()
            .filter(|s| s.garage_id == garage_id)
            .map(|s| s.session_id.clone())
            .collect();

        let mut removed = Vec::new();
        for session_id in session_ids {
            if let Some(session) = inner.sessions.remove(&session_id) {
                removed.push(session);
            }
        }
        Ok(removed)
    }

    fn remove_expired_sessions(&self) -> Result<Vec<Session>> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let session_ids: Vec<_> = inner
            .sessions
            .values()
            .filter(|s| s.expires_at <= now)
            .map(|s| s.session_id.clone())
            .collect();

        let mut removed = Vec::new();
        for session_id in session_ids {
            if let Some(session) = inner.sessions.remove(&session_id) {
                removed.push(session);
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipam::{InMemoryStore, Ipam};
    use crate::peers::{DeviceRegistration, GarageRegistration, InMemoryPeerStore, PeerRegistry};
    use moto_wgtunnel_types::{DerpNode, DerpRegion, WgPrivateKey};

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    fn create_registry() -> PeerRegistry<InMemoryPeerStore, InMemoryStore> {
        let ipam_store = InMemoryStore::new();
        let peer_store = InMemoryPeerStore::new();
        let ipam = Ipam::new(ipam_store);
        PeerRegistry::new(peer_store, ipam)
    }

    fn create_manager() -> SessionManager<InMemorySessionStore> {
        SessionManager::new(InMemorySessionStore::new())
    }

    fn create_derp_map() -> DerpMap {
        DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.example.com")),
        )
    }

    #[tokio::test]
    async fn create_session() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        // Register device and garage
        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
            })
            .await
            .unwrap();

        // Create session
        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: device_key,
            ttl_seconds: Some(3600),
        };

        let response = manager
            .create_session(request, &device, &garage, &derp_map)
            .await
            .unwrap();

        assert!(response.session_id.starts_with("sess_"));
        assert_eq!(response.garage.public_key, garage.public_key);
        assert_eq!(response.garage.overlay_ip, garage.overlay_ip);
        assert_eq!(response.client_ip, device.overlay_ip);
        assert!(!response.derp_map.is_empty());
    }

    #[tokio::test]
    async fn session_id_format() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: device_key,
            ttl_seconds: None,
        };

        let response = manager
            .create_session(request, &device, &garage, &derp_map)
            .await
            .unwrap();

        // Should be "sess_" followed by a UUID in simple format (32 hex chars)
        assert!(response.session_id.starts_with("sess_"));
        let suffix = &response.session_id[5..];
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn get_session() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        // No session yet
        assert!(manager.get_session("nonexistent").unwrap().is_none());

        // Create session
        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: device_key.clone(),
            ttl_seconds: None,
        };

        let response = manager
            .create_session(request, &device, &garage, &derp_map)
            .await
            .unwrap();

        // Now found
        let session = manager.get_session(&response.session_id).unwrap().unwrap();
        assert_eq!(session.session_id, response.session_id);
        assert_eq!(session.garage_id, "test-garage");
        assert_eq!(session.device_pubkey, device_key);
    }

    #[tokio::test]
    async fn close_session() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: device_key,
            ttl_seconds: None,
        };

        let response = manager
            .create_session(request, &device, &garage, &derp_map)
            .await
            .unwrap();

        // Close session
        let closed = manager.close_session(&response.session_id).unwrap();
        assert_eq!(closed.session_id, response.session_id);

        // No longer found
        assert!(manager.get_session(&response.session_id).unwrap().is_none());

        // Closing again fails
        let err = manager.close_session(&response.session_id).unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_sessions() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        // Create multiple garages and sessions
        for i in 0..3 {
            let garage = registry
                .register_garage(GarageRegistration {
                    garage_id: format!("garage-{i}"),
                    public_key: generate_public_key(),
                    endpoints: vec![],
                })
                .await
                .unwrap();

            let request = CreateSessionRequest {
                garage_id: format!("garage-{i}"),
                device_pubkey: device_key.clone(),
                ttl_seconds: Some(3600),
            };

            manager
                .create_session(request, &device, &garage, &derp_map)
                .await
                .unwrap();
        }

        let sessions = manager.list_sessions(&device_key).unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[tokio::test]
    async fn list_sessions_for_garage() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        // Create sessions from multiple devices
        for _ in 0..3 {
            let device_key = generate_public_key();
            let device = registry
                .register_device(DeviceRegistration {
                    public_key: device_key.clone(),
                    device_name: None,
                })
                .await
                .unwrap();

            let request = CreateSessionRequest {
                garage_id: "test-garage".to_string(),
                device_pubkey: device_key,
                ttl_seconds: Some(3600),
            };

            manager
                .create_session(request, &device, &garage, &derp_map)
                .await
                .unwrap();
        }

        let sessions = manager.list_sessions_for_garage("test-garage").unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[tokio::test]
    async fn on_garage_terminated() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        // Create sessions
        for _ in 0..3 {
            let device_key = generate_public_key();
            let device = registry
                .register_device(DeviceRegistration {
                    public_key: device_key.clone(),
                    device_name: None,
                })
                .await
                .unwrap();

            let request = CreateSessionRequest {
                garage_id: "test-garage".to_string(),
                device_pubkey: device_key,
                ttl_seconds: Some(3600),
            };

            manager
                .create_session(request, &device, &garage, &derp_map)
                .await
                .unwrap();
        }

        // Terminate garage
        let removed = manager.on_garage_terminated("test-garage").unwrap();
        assert_eq!(removed.len(), 3);

        // No sessions left
        let sessions = manager.list_sessions_for_garage("test-garage").unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn session_expiration() {
        let now = Utc::now();

        // Not expired
        let active_session = Session {
            session_id: "sess_test".to_string(),
            garage_id: "garage".to_string(),
            garage_name: "garage".to_string(),
            device_pubkey: generate_public_key(),
            created_at: now,
            expires_at: now + Duration::hours(1),
        };
        assert!(!active_session.is_expired());
        assert!(active_session.remaining_ttl_secs() > 0);

        // Expired
        let expired_session = Session {
            session_id: "sess_test".to_string(),
            garage_id: "garage".to_string(),
            garage_name: "garage".to_string(),
            device_pubkey: generate_public_key(),
            created_at: now - Duration::hours(2),
            expires_at: now - Duration::hours(1),
        };
        assert!(expired_session.is_expired());
        assert_eq!(expired_session.remaining_ttl_secs(), 0);
    }

    #[tokio::test]
    async fn cleanup_expired_sessions() {
        let registry = create_registry();
        let manager = create_manager();
        let derp_map = create_derp_map();

        let device_key = generate_public_key();
        let device = registry
            .register_device(DeviceRegistration {
                public_key: device_key.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage = registry
            .register_garage(GarageRegistration {
                garage_id: "test-garage".to_string(),
                public_key: generate_public_key(),
                endpoints: vec![],
            })
            .await
            .unwrap();

        // Create an active session
        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: device_key.clone(),
            ttl_seconds: Some(3600),
        };
        let active = manager
            .create_session(request, &device, &garage, &derp_map)
            .await
            .unwrap();

        // Manually create an expired session
        let expired = Session {
            session_id: "sess_expired".to_string(),
            garage_id: "test-garage".to_string(),
            garage_name: "test-garage".to_string(),
            device_pubkey: device_key,
            created_at: Utc::now() - Duration::hours(5),
            expires_at: Utc::now() - Duration::hours(1),
        };
        manager.store.set_session(expired).unwrap();

        // Cleanup
        let removed = manager.cleanup_expired().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].session_id, "sess_expired");

        // Active session still exists
        assert!(manager.get_session(&active.session_id).unwrap().is_some());
    }

    #[test]
    fn default_session_ttl() {
        // 4 hours = 14400 seconds
        assert_eq!(DEFAULT_SESSION_TTL_SECS, 14400);
    }

    #[test]
    fn create_session_request_serde() {
        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: generate_public_key(),
            ttl_seconds: Some(3600),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: CreateSessionRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.garage_id, parsed.garage_id);
        assert_eq!(request.device_pubkey, parsed.device_pubkey);
        assert_eq!(request.ttl_seconds, parsed.ttl_seconds);
    }

    #[test]
    fn create_session_request_serde_no_ttl() {
        // TTL should be omitted when None
        let request = CreateSessionRequest {
            garage_id: "test-garage".to_string(),
            device_pubkey: generate_public_key(),
            ttl_seconds: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("ttl_seconds"));
    }

    #[test]
    fn session_serde() {
        let session = Session {
            session_id: "sess_test123".to_string(),
            garage_id: "test-garage".to_string(),
            garage_name: "test-garage".to_string(),
            device_pubkey: generate_public_key(),
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
        };

        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(session.session_id, parsed.session_id);
        assert_eq!(session.garage_id, parsed.garage_id);
        assert_eq!(session.device_pubkey, parsed.device_pubkey);
    }
}
