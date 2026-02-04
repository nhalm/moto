//! Tunnel lifecycle management.
//!
//! This module provides the [`TunnelManager`] for managing device identity
//! and tunnel sessions, and [`TunnelSession`] for individual garage connections.
//!
//! # Key Management
//!
//! Device keys are stored in `~/.config/moto/`:
//! - `wg-private.key`: `WireGuard` private key (0600 permissions)
//! - `wg-public.key`: `WireGuard` public key (0644 permissions)
//!
//! The `WireGuard` public key IS the device identity (Cloudflare WARP model).
//! There is no separate device ID - the public key serves as the identifier.
//!
//! Keys are generated on first use and reused across sessions.
//!
//! # Security
//!
//! - Key files MUST have correct permissions (0600 for private key)
//! - The manager refuses to start if permissions are wrong
//! - Keys are loaded into memory only when needed
//!
//! # Example
//!
//! ```ignore
//! use moto_cli_wgtunnel::tunnel::TunnelManager;
//!
//! // Create manager (loads or generates device identity)
//! let manager = TunnelManager::new().await?;
//!
//! // Get device public key for moto-club registration
//! let public_key = manager.public_key();
//! ```

// Allow similar names like garage_id and garage_ip which are distinct in context
#![allow(clippy::similar_names)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use moto_wgtunnel_conn::PathType;
use moto_wgtunnel_engine::tunnel::{
    Tunnel as WgTunnel, TunnelBuilder as WgTunnelBuilder, TunnelError as WgTunnelError,
    TunnelEvent as WgTunnelEvent, TunnelState as WgTunnelState,
};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPrivateKey, WgPublicKey};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Environment variable to override `WireGuard` key file location.
pub const ENV_WG_KEY_FILE: &str = "MOTO_WG_KEY_FILE";

/// Required permissions for key files (0600 = owner read/write only).
pub const KEY_FILE_PERMISSIONS: u32 = 0o600;

/// Required permissions for key directory (0700 = owner only).
pub const KEY_DIR_PERMISSIONS: u32 = 0o700;

/// Private key filename.
const PRIVATE_KEY_FILE: &str = "wg-private.key";

/// Public key filename.
const PUBLIC_KEY_FILE: &str = "wg-public.key";

/// Errors that can occur during tunnel operations.
#[derive(Debug, Error)]
pub enum TunnelError {
    /// Key file has incorrect permissions.
    #[error("key file {path} has incorrect permissions: expected {expected:o}, got {actual:o}")]
    PermissionError {
        /// Path to the file.
        path: PathBuf,
        /// Expected permissions.
        expected: u32,
        /// Actual permissions.
        actual: u32,
    },

    /// Failed to read or write key file.
    #[error("key file error: {0}")]
    KeyFile(#[from] std::io::Error),

    /// Invalid key format.
    #[error("invalid key: {0}")]
    InvalidKey(#[from] moto_wgtunnel_types::KeyError),

    /// Configuration directory not found.
    #[error("config directory not found")]
    NoConfigDir,

    /// Session not found.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// Session already exists.
    #[error("session already exists for garage: {0}")]
    SessionExists(String),

    /// Connection failed.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// `WireGuard` tunnel error.
    #[error("WireGuard tunnel error: {0}")]
    WireGuard(#[from] WgTunnelError),
}

/// Device identity for `WireGuard` connections.
///
/// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
/// There is no separate device ID - the public key serves as the unique identifier.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    /// Device's `WireGuard` public key (this IS the device identity).
    pub public_key: WgPublicKey,
}

impl DeviceIdentity {
    /// Create a new device identity from a `WireGuard` public key.
    #[must_use]
    pub const fn new(public_key: WgPublicKey) -> Self {
        Self { public_key }
    }
}

/// Status of a tunnel session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelStatus {
    /// Session is being initialized.
    Initializing,

    /// Attempting direct UDP connection.
    ConnectingDirect,

    /// Connecting via DERP relay.
    ConnectingDerp {
        /// DERP region name.
        region: String,
    },

    /// Tunnel is established.
    Connected {
        /// Current path type.
        path: PathType,
    },

    /// Tunnel is disconnected.
    Disconnected,

    /// Tunnel encountered an error.
    Error {
        /// Error message.
        message: String,
    },
}

impl std::fmt::Display for TunnelStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initializing => write!(f, "initializing"),
            Self::ConnectingDirect => write!(f, "connecting (direct)"),
            Self::ConnectingDerp { region } => write!(f, "connecting (DERP: {region})"),
            Self::Connected { path } => match path {
                PathType::Direct { endpoint } => write!(f, "connected (direct: {endpoint})"),
                PathType::Derp { region_name, .. } => write!(f, "connected (DERP: {region_name})"),
            },
            Self::Disconnected => write!(f, "disconnected"),
            Self::Error { message } => write!(f, "error: {message}"),
        }
    }
}

/// A tunnel session to a single garage.
///
/// Each session represents an active or pending connection to a garage.
/// Sessions are managed by [`TunnelManager`].
pub struct TunnelSession {
    /// Session ID (from moto-club).
    session_id: String,

    /// Target garage ID.
    garage_id: String,

    /// Target garage name (for display).
    garage_name: String,

    /// Our overlay IP for this session.
    client_ip: OverlayIp,

    /// Garage's overlay IP.
    garage_ip: OverlayIp,

    /// Garage's `WireGuard` public key.
    garage_public_key: WgPublicKey,

    /// DERP map for relay fallback.
    derp_map: DerpMap,

    /// Current connection status.
    status: TunnelStatus,

    /// `WireGuard` tunnel instance (boringtun-backed).
    ///
    /// This is `None` until the tunnel is configured, then holds the
    /// active `WireGuard` state machine for packet encryption/decryption.
    wg_tunnel: Option<WgTunnel>,
}

impl std::fmt::Debug for TunnelSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelSession")
            .field("session_id", &self.session_id)
            .field("garage_id", &self.garage_id)
            .field("garage_name", &self.garage_name)
            .field("client_ip", &self.client_ip)
            .field("garage_ip", &self.garage_ip)
            .field("garage_public_key", &self.garage_public_key)
            .field("status", &self.status)
            .field(
                "wg_tunnel",
                &self
                    .wg_tunnel
                    .as_ref()
                    .map(moto_wgtunnel_engine::Tunnel::state),
            )
            .finish_non_exhaustive()
    }
}

/// Default `WireGuard` keepalive interval (25 seconds).
pub const DEFAULT_KEEPALIVE_SECS: u64 = 25;

impl TunnelSession {
    /// Create a new tunnel session.
    #[must_use]
    pub const fn new(
        session_id: String,
        garage_id: String,
        garage_name: String,
        client_ip: OverlayIp,
        garage_ip: OverlayIp,
        garage_public_key: WgPublicKey,
        derp_map: DerpMap,
    ) -> Self {
        Self {
            session_id,
            garage_id,
            garage_name,
            client_ip,
            garage_ip,
            garage_public_key,
            derp_map,
            status: TunnelStatus::Initializing,
            wg_tunnel: None,
        }
    }

    /// Get the session ID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the garage ID.
    #[must_use]
    pub fn garage_id(&self) -> &str {
        &self.garage_id
    }

    /// Get the garage name.
    #[must_use]
    pub fn garage_name(&self) -> &str {
        &self.garage_name
    }

    /// Get our client overlay IP.
    #[must_use]
    pub const fn client_ip(&self) -> OverlayIp {
        self.client_ip
    }

    /// Get the garage's overlay IP.
    #[must_use]
    pub const fn garage_ip(&self) -> OverlayIp {
        self.garage_ip
    }

    /// Get the garage's `WireGuard` public key.
    #[must_use]
    pub const fn garage_public_key(&self) -> &WgPublicKey {
        &self.garage_public_key
    }

    /// Get the DERP map.
    #[must_use]
    pub const fn derp_map(&self) -> &DerpMap {
        &self.derp_map
    }

    /// Get the current connection status.
    #[must_use]
    pub const fn status(&self) -> &TunnelStatus {
        &self.status
    }

    /// Set the connection status.
    pub fn set_status(&mut self, status: TunnelStatus) {
        debug!(
            session_id = %self.session_id,
            old_status = %self.status,
            new_status = %status,
            "tunnel status changed"
        );
        self.status = status;
    }

    /// Check if the session is connected.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self.status, TunnelStatus::Connected { .. })
    }

    /// Check if the session is in an error state.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self.status, TunnelStatus::Error { .. })
    }

    /// Check if the `WireGuard` tunnel is configured.
    #[must_use]
    pub const fn is_wg_configured(&self) -> bool {
        self.wg_tunnel.is_some()
    }

    /// Get the `WireGuard` tunnel state, if configured.
    #[must_use]
    pub fn wg_state(&self) -> Option<WgTunnelState> {
        self.wg_tunnel
            .as_ref()
            .map(moto_wgtunnel_engine::Tunnel::state)
    }

    /// Check if the `WireGuard` handshake is complete.
    #[must_use]
    pub fn is_wg_established(&self) -> bool {
        self.wg_tunnel
            .as_ref()
            .is_some_and(moto_wgtunnel_engine::Tunnel::is_established)
    }

    /// Configure the `WireGuard` tunnel with the given private key.
    ///
    /// This creates the boringtun tunnel instance that handles packet
    /// encryption/decryption. The tunnel is configured with:
    /// - Our device's private key
    /// - The garage's public key
    /// - Default keepalive interval (25 seconds)
    ///
    /// # Arguments
    ///
    /// * `private_key` - Our device's `WireGuard` private key
    ///
    /// # Errors
    ///
    /// Returns error if tunnel creation fails.
    pub fn configure_wg_tunnel(&mut self, private_key: &WgPrivateKey) -> Result<(), TunnelError> {
        self.configure_wg_tunnel_with_keepalive(
            private_key,
            std::time::Duration::from_secs(DEFAULT_KEEPALIVE_SECS),
        )
    }

    /// Configure the `WireGuard` tunnel with custom keepalive.
    ///
    /// # Arguments
    ///
    /// * `private_key` - Our device's `WireGuard` private key
    /// * `keepalive` - Keepalive interval for NAT traversal
    ///
    /// # Errors
    ///
    /// Returns error if tunnel creation fails.
    pub fn configure_wg_tunnel_with_keepalive(
        &mut self,
        private_key: &WgPrivateKey,
        keepalive: std::time::Duration,
    ) -> Result<(), TunnelError> {
        // Create a copy of the private key by converting to/from bytes
        // (WgPrivateKey doesn't implement Clone to prevent accidental exposure)
        let private_key_copy = WgPrivateKey::from_bytes(&private_key.as_bytes())
            .map_err(|e| TunnelError::ConnectionFailed(format!("invalid private key: {e}")))?;

        let tunnel = WgTunnelBuilder::new(private_key_copy, self.garage_public_key.clone())
            .keepalive(keepalive)
            .build()?;

        debug!(
            session_id = %self.session_id,
            garage = %self.garage_name,
            tunnel_index = tunnel.index(),
            "WireGuard tunnel configured"
        );

        self.wg_tunnel = Some(tunnel);
        Ok(())
    }

    /// Initiate the `WireGuard` handshake.
    ///
    /// Returns the handshake initiation packet that should be sent to the peer
    /// (via `MagicConn`). The tunnel transitions to `Handshaking` state.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Tunnel is not configured
    /// - Handshake initiation fails
    pub fn initiate_handshake(&mut self) -> Result<Vec<WgTunnelEvent>, TunnelError> {
        let tunnel = self.wg_tunnel.as_mut().ok_or_else(|| {
            TunnelError::ConnectionFailed("WireGuard tunnel not configured".into())
        })?;

        let events = tunnel.force_handshake()?;

        debug!(
            session_id = %self.session_id,
            events_count = events.len(),
            "handshake initiated"
        );

        Ok(events)
    }

    /// Encapsulate an IP packet for sending over the `WireGuard` tunnel.
    ///
    /// Takes a plaintext IP packet and returns encrypted `WireGuard` packets
    /// to send to the peer via `MagicConn`.
    ///
    /// # Arguments
    ///
    /// * `ip_packet` - The IP packet to encapsulate
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Tunnel is not configured
    /// - Encapsulation fails
    pub fn encapsulate(&mut self, ip_packet: &[u8]) -> Result<Vec<WgTunnelEvent>, TunnelError> {
        let tunnel = self.wg_tunnel.as_mut().ok_or_else(|| {
            TunnelError::ConnectionFailed("WireGuard tunnel not configured".into())
        })?;

        let events = tunnel.encapsulate(ip_packet)?;
        Ok(events)
    }

    /// Decapsulate a received `WireGuard` packet.
    ///
    /// Takes an encrypted `WireGuard` packet from the peer and returns
    /// decrypted IP packets for processing.
    ///
    /// # Arguments
    ///
    /// * `wg_packet` - The encrypted `WireGuard` packet
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Tunnel is not configured
    /// - Decapsulation fails
    pub fn decapsulate(&mut self, wg_packet: &[u8]) -> Result<Vec<WgTunnelEvent>, TunnelError> {
        let tunnel = self.wg_tunnel.as_mut().ok_or_else(|| {
            TunnelError::ConnectionFailed("WireGuard tunnel not configured".into())
        })?;

        let events = tunnel.decapsulate(wg_packet)?;
        Ok(events)
    }

    /// Check for pending timer actions.
    ///
    /// Should be called periodically to handle keepalives and timeouts.
    /// Returns any packets that need to be sent.
    ///
    /// # Errors
    ///
    /// Returns error if timer processing fails.
    pub fn update_timers(&mut self) -> Result<Vec<WgTunnelEvent>, TunnelError> {
        let tunnel = self.wg_tunnel.as_mut().ok_or_else(|| {
            TunnelError::ConnectionFailed("WireGuard tunnel not configured".into())
        })?;

        let events = tunnel.update_timers()?;
        Ok(events)
    }
}

/// Manager for `WireGuard` tunnel sessions.
///
/// The tunnel manager handles:
/// - Device identity (`WireGuard` keypair, device UUID)
/// - Active tunnel sessions
/// - Key file management
///
/// # Thread Safety
///
/// The manager is thread-safe and can be shared across tasks.
pub struct TunnelManager {
    /// Device's `WireGuard` private key.
    private_key: WgPrivateKey,

    /// Device identity (public key + device ID).
    identity: DeviceIdentity,

    /// Path to the config directory.
    config_dir: PathBuf,

    /// Active sessions by session ID.
    sessions: Arc<RwLock<HashMap<String, TunnelSession>>>,
}

impl std::fmt::Debug for TunnelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelManager")
            .field("identity", &self.identity)
            .field("config_dir", &self.config_dir)
            .finish_non_exhaustive()
    }
}

impl TunnelManager {
    /// Create a new tunnel manager.
    ///
    /// Loads existing device identity from `~/.config/moto/` or generates
    /// new keys if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Config directory cannot be determined
    /// - Key files exist but have incorrect permissions
    /// - Key files are corrupted
    pub async fn new() -> Result<Self, TunnelError> {
        let config_dir = get_config_dir()?;
        Self::with_config_dir(config_dir).await
    }

    /// Create a tunnel manager with a specific config directory.
    ///
    /// # Errors
    ///
    /// Returns error if key files have incorrect permissions or are corrupted.
    pub async fn with_config_dir(config_dir: PathBuf) -> Result<Self, TunnelError> {
        // Ensure config directory exists with correct permissions
        ensure_dir_exists(&config_dir)?;

        // Load or generate device identity (WG public key IS the identity)
        let (private_key, public_key) = load_or_generate_keypair(&config_dir).await?;

        let identity = DeviceIdentity::new(public_key);

        info!(
            public_key = %identity.public_key,
            "tunnel manager initialized"
        );

        Ok(Self {
            private_key,
            identity,
            config_dir,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get the device identity.
    #[must_use]
    pub const fn device_info(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Get the device's `WireGuard` public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.identity.public_key
    }

    /// Get a reference to the private key.
    ///
    /// # Security
    ///
    /// Handle the returned key carefully - it is secret material.
    #[must_use]
    pub const fn private_key(&self) -> &WgPrivateKey {
        &self.private_key
    }

    /// Get the config directory path.
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Add a new tunnel session.
    ///
    /// # Errors
    ///
    /// Returns error if a session for this garage already exists.
    pub async fn add_session(&self, session: TunnelSession) -> Result<(), TunnelError> {
        let mut sessions = self.sessions.write().await;

        // Check if session already exists for this garage
        if sessions.values().any(|s| s.garage_id == session.garage_id) {
            return Err(TunnelError::SessionExists(session.garage_id));
        }

        let session_id = session.session_id.clone();
        sessions.insert(session_id.clone(), session);
        drop(sessions);

        debug!(session_id = %session_id, "added tunnel session");
        Ok(())
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<TunnelSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Get a session by garage ID.
    pub async fn get_session_by_garage(&self, garage_id: &str) -> Option<TunnelSession> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .find(|s| s.garage_id == garage_id)
            .cloned()
    }

    /// Update a session's status.
    ///
    /// # Errors
    ///
    /// Returns error if session is not found.
    pub async fn update_session_status(
        &self,
        session_id: &str,
        status: TunnelStatus,
    ) -> Result<(), TunnelError> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| TunnelError::SessionNotFound(session_id.to_string()))?;

        session.set_status(status);
        drop(sessions);
        Ok(())
    }

    /// Configure the `WireGuard` tunnel for a session.
    ///
    /// Creates the boringtun tunnel instance using the device's private key
    /// and the garage's public key from the session.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Session is not found
    /// - Tunnel configuration fails
    pub async fn configure_wg_tunnel(&self, session_id: &str) -> Result<(), TunnelError> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| TunnelError::SessionNotFound(session_id.to_string()))?;

        session.configure_wg_tunnel(&self.private_key)?;
        drop(sessions);
        Ok(())
    }

    /// Configure the `WireGuard` tunnel with custom keepalive.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Session is not found
    /// - Tunnel configuration fails
    pub async fn configure_wg_tunnel_with_keepalive(
        &self,
        session_id: &str,
        keepalive: std::time::Duration,
    ) -> Result<(), TunnelError> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| TunnelError::SessionNotFound(session_id.to_string()))?;

        session.configure_wg_tunnel_with_keepalive(&self.private_key, keepalive)?;
        drop(sessions);
        Ok(())
    }

    /// Initiate the `WireGuard` handshake for a session.
    ///
    /// Returns the handshake initiation packets that should be sent to the peer.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Session is not found
    /// - Tunnel is not configured
    /// - Handshake initiation fails
    pub async fn initiate_handshake(
        &self,
        session_id: &str,
    ) -> Result<Vec<WgTunnelEvent>, TunnelError> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| TunnelError::SessionNotFound(session_id.to_string()))?;

        let result = session.initiate_handshake();
        drop(sessions);
        result
    }

    /// Remove a session by ID.
    ///
    /// Returns the removed session, if it existed.
    pub async fn remove_session(&self, session_id: &str) -> Option<TunnelSession> {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(session_id);
        drop(sessions);

        if removed.is_some() {
            debug!(session_id = %session_id, "removed tunnel session");
        }

        removed
    }

    /// List all active sessions.
    pub async fn list_sessions(&self) -> Vec<TunnelSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Get the number of active sessions.
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Close all sessions.
    pub async fn close_all(&self) {
        let mut sessions = self.sessions.write().await;
        let count = sessions.len();
        sessions.clear();
        drop(sessions);

        if count > 0 {
            info!(count, "closed all tunnel sessions");
        }
    }
}

impl Clone for TunnelSession {
    /// Clone the session metadata without the `WireGuard` tunnel.
    ///
    /// Note: The `WireGuard` tunnel (`wg_tunnel`) is NOT cloned and will be
    /// `None` in the cloned session. This is intentional - the tunnel state
    /// machine should not be duplicated.
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            garage_id: self.garage_id.clone(),
            garage_name: self.garage_name.clone(),
            client_ip: self.client_ip,
            garage_ip: self.garage_ip,
            garage_public_key: self.garage_public_key.clone(),
            derp_map: self.derp_map.clone(),
            status: self.status.clone(),
            // WireGuard tunnel is not cloned - the state machine is unique per session
            wg_tunnel: None,
        }
    }
}

/// Get the default config directory (`~/.config/moto`).
fn get_config_dir() -> Result<PathBuf, TunnelError> {
    // Check for override via environment variable
    if let Ok(key_file) = std::env::var(ENV_WG_KEY_FILE) {
        let path = PathBuf::from(&key_file);
        if let Some(parent) = path.parent() {
            return Ok(parent.to_path_buf());
        }
    }

    // Use standard config directory
    dirs::config_dir()
        .map(|p| p.join("moto"))
        .ok_or(TunnelError::NoConfigDir)
}

/// Ensure a directory exists with correct permissions.
fn ensure_dir_exists(path: &Path) -> Result<(), TunnelError> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(KEY_DIR_PERMISSIONS);
            std::fs::set_permissions(path, perms)?;
        }

        debug!(path = %path.display(), "created config directory");
    }

    Ok(())
}

/// Check file permissions on Unix systems.
#[cfg(unix)]
fn check_permissions(path: &Path, expected: u32) -> Result<(), TunnelError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)?;
    let mode = metadata.permissions().mode() & 0o777;

    if mode != expected {
        return Err(TunnelError::PermissionError {
            path: path.to_path_buf(),
            expected,
            actual: mode,
        });
    }

    Ok(())
}

/// Check file permissions (no-op on non-Unix).
#[cfg(not(unix))]
fn check_permissions(_path: &Path, _expected: u32) -> Result<(), TunnelError> {
    Ok(())
}

/// Set file permissions on Unix systems.
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<(), TunnelError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// Set file permissions (no-op on non-Unix).
#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), TunnelError> {
    Ok(())
}

/// Load or generate the `WireGuard` keypair.
async fn load_or_generate_keypair(
    config_dir: &Path,
) -> Result<(WgPrivateKey, WgPublicKey), TunnelError> {
    let private_path = config_dir.join(PRIVATE_KEY_FILE);
    let public_path = config_dir.join(PUBLIC_KEY_FILE);

    if private_path.exists() {
        // Load existing keys
        check_permissions(&private_path, KEY_FILE_PERMISSIONS)?;

        let private_b64 = tokio::fs::read_to_string(&private_path).await?;
        let private_key = WgPrivateKey::from_base64(private_b64.trim())?;
        let public_key = private_key.public_key();

        debug!(path = %private_path.display(), "loaded existing WireGuard keypair");

        // Verify public key matches
        if public_path.exists() {
            let stored_public_b64 = tokio::fs::read_to_string(&public_path).await?;
            let stored_public = WgPublicKey::from_base64(stored_public_b64.trim())?;

            if stored_public != public_key {
                warn!("stored public key doesn't match derived public key, regenerating");
                tokio::fs::write(&public_path, public_key.to_base64()).await?;
            }
        } else {
            // Write public key file
            tokio::fs::write(&public_path, public_key.to_base64()).await?;
        }

        Ok((private_key, public_key))
    } else {
        // Generate new keypair
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();

        // Write private key with strict permissions
        tokio::fs::write(&private_path, private_key.to_base64()).await?;
        set_permissions(&private_path, KEY_FILE_PERMISSIONS)?;

        // Write public key (less strict permissions)
        tokio::fs::write(&public_path, public_key.to_base64()).await?;

        info!(
            private_path = %private_path.display(),
            public_key = %public_key,
            "generated new WireGuard keypair"
        );

        Ok((private_key, public_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_manager() -> (TunnelManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = TunnelManager::with_config_dir(temp_dir.path().to_path_buf())
            .await
            .unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn create_manager() {
        let (manager, _temp) = test_manager().await;

        // Should have valid device identity (WG public key IS the identity)
        assert!(!manager.public_key().to_base64().is_empty());
    }

    #[tokio::test]
    async fn manager_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().to_path_buf();

        // Create first manager
        let manager1 = TunnelManager::with_config_dir(config_dir.clone())
            .await
            .unwrap();
        let public_key1 = manager1.public_key().clone();

        // Create second manager with same directory
        let manager2 = TunnelManager::with_config_dir(config_dir).await.unwrap();

        // Should have same identity (WG public key IS the identity)
        assert_eq!(public_key1, *manager2.public_key());
    }

    #[tokio::test]
    async fn session_management() {
        let (manager, _temp) = test_manager().await;

        let garage_key = WgPrivateKey::generate().public_key();
        let session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        // Add session
        Box::pin(manager.add_session(session)).await.unwrap();
        assert_eq!(manager.session_count().await, 1);

        // Get session by ID
        let found = manager.get_session("sess_123").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().garage_name(), "test-garage");

        // Get session by garage ID
        let found = manager.get_session_by_garage("garage_abc").await;
        assert!(found.is_some());

        // Update status
        manager
            .update_session_status(
                "sess_123",
                TunnelStatus::Connected {
                    path: PathType::derp(1, "primary"),
                },
            )
            .await
            .unwrap();

        let updated = manager.get_session("sess_123").await.unwrap();
        assert!(updated.is_connected());

        // Remove session
        let removed = manager.remove_session("sess_123").await;
        assert!(removed.is_some());
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn duplicate_session_error() {
        let (manager, _temp) = test_manager().await;

        let garage_key = WgPrivateKey::generate().public_key();
        let session1 = TunnelSession::new(
            "sess_1".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key.clone(),
            DerpMap::new(),
        );

        let session2 = TunnelSession::new(
            "sess_2".to_string(),
            "garage_abc".to_string(), // Same garage ID
            "test-garage".to_string(),
            OverlayIp::client(2),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        Box::pin(manager.add_session(session1)).await.unwrap();

        // Should fail - same garage ID
        let result = Box::pin(manager.add_session(session2)).await;
        assert!(matches!(result, Err(TunnelError::SessionExists(_))));
    }

    #[tokio::test]
    async fn session_not_found_error() {
        let (manager, _temp) = test_manager().await;

        let result = manager
            .update_session_status("nonexistent", TunnelStatus::Disconnected)
            .await;

        assert!(matches!(result, Err(TunnelError::SessionNotFound(_))));
    }

    #[tokio::test]
    async fn close_all_sessions() {
        let (manager, _temp) = test_manager().await;

        // Add multiple sessions
        for i in 0..3 {
            let garage_key = WgPrivateKey::generate().public_key();
            let session = TunnelSession::new(
                format!("sess_{i}"),
                format!("garage_{i}"),
                format!("test-garage-{i}"),
                OverlayIp::client(u64::try_from(i).unwrap()),
                OverlayIp::garage(u64::try_from(i).unwrap()),
                garage_key,
                DerpMap::new(),
            );
            Box::pin(manager.add_session(session)).await.unwrap();
        }

        assert_eq!(manager.session_count().await, 3);

        manager.close_all().await;
        assert_eq!(manager.session_count().await, 0);
    }

    #[test]
    fn tunnel_status_display() {
        assert_eq!(TunnelStatus::Initializing.to_string(), "initializing");
        assert_eq!(
            TunnelStatus::ConnectingDirect.to_string(),
            "connecting (direct)"
        );
        assert_eq!(
            TunnelStatus::ConnectingDerp {
                region: "us-west".to_string()
            }
            .to_string(),
            "connecting (DERP: us-west)"
        );
        assert_eq!(
            TunnelStatus::Connected {
                path: PathType::Direct {
                    endpoint: "1.2.3.4:51820".parse().unwrap()
                }
            }
            .to_string(),
            "connected (direct: 1.2.3.4:51820)"
        );
        assert_eq!(TunnelStatus::Disconnected.to_string(), "disconnected");
        assert_eq!(
            TunnelStatus::Error {
                message: "test error".to_string()
            }
            .to_string(),
            "error: test error"
        );
    }

    #[test]
    fn device_identity_creation() {
        let public_key = WgPrivateKey::generate().public_key();

        // WG public key IS the device identity (per spec v0.7)
        let identity = DeviceIdentity::new(public_key.clone());

        assert_eq!(identity.public_key, public_key);
    }

    #[test]
    fn session_status_checks() {
        let garage_key = WgPrivateKey::generate().public_key();
        let mut session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        assert!(!session.is_connected());
        assert!(!session.is_error());

        session.set_status(TunnelStatus::Connected {
            path: PathType::Direct {
                endpoint: "1.2.3.4:51820".parse().unwrap(),
            },
        });
        assert!(session.is_connected());
        assert!(!session.is_error());

        session.set_status(TunnelStatus::Error {
            message: "test".to_string(),
        });
        assert!(!session.is_connected());
        assert!(session.is_error());
    }

    #[test]
    fn session_wg_tunnel_configuration() {
        let device_private_key = WgPrivateKey::generate();
        let garage_key = WgPrivateKey::generate().public_key();

        let mut session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        // Initially no WG tunnel configured
        assert!(!session.is_wg_configured());
        assert!(session.wg_state().is_none());
        assert!(!session.is_wg_established());

        // Configure the WG tunnel
        session.configure_wg_tunnel(&device_private_key).unwrap();

        // Now WG tunnel should be configured but not yet established
        assert!(session.is_wg_configured());
        assert!(session.wg_state().is_some());
        assert_eq!(session.wg_state(), Some(WgTunnelState::Init));
        assert!(!session.is_wg_established());
    }

    #[test]
    fn session_wg_handshake_initiation() {
        let device_private_key = WgPrivateKey::generate();
        let garage_key = WgPrivateKey::generate().public_key();

        let mut session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        // Configure the WG tunnel
        session.configure_wg_tunnel(&device_private_key).unwrap();

        // Initiate handshake
        let events = session.initiate_handshake().unwrap();

        // Should produce handshake initiation packet
        assert!(!events.is_empty());
        assert!(events[0].is_network());

        // Tunnel state should be handshaking
        assert_eq!(session.wg_state(), Some(WgTunnelState::Handshaking));
    }

    #[test]
    fn session_wg_tunnel_not_configured_error() {
        let garage_key = WgPrivateKey::generate().public_key();

        let mut session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        // Trying to initiate handshake without configuring tunnel should fail
        let result = session.initiate_handshake();
        assert!(result.is_err());
        assert!(matches!(result, Err(TunnelError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn manager_configure_wg_tunnel() {
        let (manager, _temp) = test_manager().await;

        let garage_key = WgPrivateKey::generate().public_key();
        let session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        Box::pin(manager.add_session(session)).await.unwrap();

        // Configure WG tunnel through manager
        manager.configure_wg_tunnel("sess_123").await.unwrap();

        // Initiate handshake through manager
        let events = manager.initiate_handshake("sess_123").await.unwrap();
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn manager_configure_wg_tunnel_session_not_found() {
        let (manager, _temp) = test_manager().await;

        // Trying to configure tunnel for non-existent session should fail
        let result = manager.configure_wg_tunnel("nonexistent").await;
        assert!(matches!(result, Err(TunnelError::SessionNotFound(_))));
    }

    #[test]
    fn session_clone_does_not_clone_wg_tunnel() {
        let device_private_key = WgPrivateKey::generate();
        let garage_key = WgPrivateKey::generate().public_key();

        let mut session = TunnelSession::new(
            "sess_123".to_string(),
            "garage_abc".to_string(),
            "test-garage".to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );

        // Configure the WG tunnel
        session.configure_wg_tunnel(&device_private_key).unwrap();
        assert!(session.is_wg_configured());

        // Clone the session
        let cloned = session.clone();

        // Cloned session should NOT have WG tunnel configured
        assert!(!cloned.is_wg_configured());
        // But should have same metadata
        assert_eq!(cloned.session_id(), session.session_id());
        assert_eq!(cloned.garage_name(), session.garage_name());
    }
}
