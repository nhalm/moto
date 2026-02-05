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
//! by its `WireGuard` public key (Cloudflare WARP model):
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
//! # Storage
//!
//! The [`SessionStore`] trait defines the storage interface. For production,
//! use `PostgresSessionStore` from `moto-club-api`.

use chrono::{DateTime, Duration, Utc};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
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

    /// Device not registered (identified by `WireGuard` public key).
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

    /// Device requesting the connection (`WireGuard` public key IS the device identity).
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

    /// Device that created this session (`WireGuard` public key IS the device identity).
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
    /// The `WireGuard` public key IS the device identity.
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
    /// The `WireGuard` public key IS the device identity.
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

// Unit tests for pure functions and serde (no database needed)
#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
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

// Integration tests that require PostgreSQL
// Run with: cargo test --features integration
// Note: These tests require PostgresSessionStore from moto-club-api.
// See moto-club-api/src/wg_test.rs for integration tests with PostgreSQL storage.
//
// Tests to implement:
// - create_session: Create session with device/garage and verify response
// - session_id_format: Verify "sess_" prefix + 32 hex chars
// - get_session: Lookup session by ID
// - close_session: Close session and verify removal
// - list_sessions: List sessions for a device
// - list_sessions_for_garage: List sessions for a garage
// - on_garage_terminated: Remove all sessions when garage terminates
// - cleanup_expired_sessions: Remove expired sessions, keep active ones
