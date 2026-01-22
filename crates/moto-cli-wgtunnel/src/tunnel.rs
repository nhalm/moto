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
//! - `device-id`: Device UUID (0644 permissions)
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
//! // Get device info for moto-club registration
//! let info = manager.device_info();
//! ```

// Allow similar names like garage_id and garage_ip which are distinct in context
#![allow(clippy::similar_names)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use moto_wgtunnel_conn::PathType;
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPrivateKey, WgPublicKey};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Environment variable to override WireGuard key file location.
pub const ENV_WG_KEY_FILE: &str = "MOTO_WG_KEY_FILE";

/// Required permissions for key files (0600 = owner read/write only).
pub const KEY_FILE_PERMISSIONS: u32 = 0o600;

/// Required permissions for key directory (0700 = owner only).
pub const KEY_DIR_PERMISSIONS: u32 = 0o700;

/// Private key filename.
const PRIVATE_KEY_FILE: &str = "wg-private.key";

/// Public key filename.
const PUBLIC_KEY_FILE: &str = "wg-public.key";

/// Device ID filename.
const DEVICE_ID_FILE: &str = "device-id";

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

    /// Invalid device ID.
    #[error("invalid device ID: {0}")]
    InvalidDeviceId(String),
}

/// Device identity for WireGuard connections.
///
/// This represents the local device's identity, consisting of a persistent
/// WireGuard keypair and a unique device UUID.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    /// Unique device identifier (UUID v7).
    pub device_id: Uuid,

    /// Device's WireGuard public key.
    pub public_key: WgPublicKey,
}

impl DeviceIdentity {
    /// Create a new device identity.
    #[must_use]
    pub fn new(device_id: Uuid, public_key: WgPublicKey) -> Self {
        Self {
            device_id,
            public_key,
        }
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
#[derive(Debug)]
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

    /// Garage's WireGuard public key.
    garage_public_key: WgPublicKey,

    /// DERP map for relay fallback.
    derp_map: DerpMap,

    /// Current connection status.
    status: TunnelStatus,
}

impl TunnelSession {
    /// Create a new tunnel session.
    #[must_use]
    pub fn new(
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

    /// Get the garage's WireGuard public key.
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
    pub fn status(&self) -> &TunnelStatus {
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
    pub fn is_connected(&self) -> bool {
        matches!(self.status, TunnelStatus::Connected { .. })
    }

    /// Check if the session is in an error state.
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self.status, TunnelStatus::Error { .. })
    }
}

/// Manager for WireGuard tunnel sessions.
///
/// The tunnel manager handles:
/// - Device identity (WireGuard keypair, device UUID)
/// - Active tunnel sessions
/// - Key file management
///
/// # Thread Safety
///
/// The manager is thread-safe and can be shared across tasks.
pub struct TunnelManager {
    /// Device's WireGuard private key.
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

        // Load or generate device identity
        let (private_key, public_key) = load_or_generate_keypair(&config_dir).await?;
        let device_id = load_or_generate_device_id(&config_dir).await?;

        let identity = DeviceIdentity::new(device_id, public_key);

        info!(
            device_id = %identity.device_id,
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
    pub fn device_info(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Get the device ID.
    #[must_use]
    pub fn device_id(&self) -> Uuid {
        self.identity.device_id
    }

    /// Get the device's WireGuard public key.
    #[must_use]
    pub fn public_key(&self) -> &WgPublicKey {
        &self.identity.public_key
    }

    /// Get a reference to the private key.
    ///
    /// # Security
    ///
    /// Handle the returned key carefully - it is secret material.
    #[must_use]
    pub fn private_key(&self) -> &WgPrivateKey {
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
        if sessions
            .values()
            .any(|s| s.garage_id == session.garage_id)
        {
            return Err(TunnelError::SessionExists(session.garage_id));
        }

        let session_id = session.session_id.clone();
        sessions.insert(session_id.clone(), session);

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
        Ok(())
    }

    /// Remove a session by ID.
    ///
    /// Returns the removed session, if it existed.
    pub async fn remove_session(&self, session_id: &str) -> Option<TunnelSession> {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(session_id);

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

        if count > 0 {
            info!(count, "closed all tunnel sessions");
        }
    }
}

impl Clone for TunnelSession {
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

/// Load or generate the WireGuard keypair.
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

/// Load or generate the device ID.
async fn load_or_generate_device_id(config_dir: &Path) -> Result<Uuid, TunnelError> {
    let path = config_dir.join(DEVICE_ID_FILE);

    if path.exists() {
        let content = tokio::fs::read_to_string(&path).await?;
        let device_id = Uuid::parse_str(content.trim())
            .map_err(|e| TunnelError::InvalidDeviceId(e.to_string()))?;

        debug!(device_id = %device_id, "loaded existing device ID");
        Ok(device_id)
    } else {
        // Generate new UUID v7 (time-ordered)
        let device_id = Uuid::now_v7();

        tokio::fs::write(&path, device_id.to_string()).await?;

        info!(device_id = %device_id, "generated new device ID");
        Ok(device_id)
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

        // Should have valid device identity
        assert!(!manager.device_id().is_nil());
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
        let device_id1 = manager1.device_id();
        let public_key1 = manager1.public_key().clone();

        // Create second manager with same directory
        let manager2 = TunnelManager::with_config_dir(config_dir)
            .await
            .unwrap();

        // Should have same identity
        assert_eq!(device_id1, manager2.device_id());
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
        manager.add_session(session).await.unwrap();
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

        manager.add_session(session1).await.unwrap();

        // Should fail - same garage ID
        let result = manager.add_session(session2).await;
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
                OverlayIp::client(i as u64),
                OverlayIp::garage(i as u64),
                garage_key,
                DerpMap::new(),
            );
            manager.add_session(session).await.unwrap();
        }

        assert_eq!(manager.session_count().await, 3);

        manager.close_all().await;
        assert_eq!(manager.session_count().await, 0);
    }

    #[test]
    fn tunnel_status_display() {
        assert_eq!(TunnelStatus::Initializing.to_string(), "initializing");
        assert_eq!(TunnelStatus::ConnectingDirect.to_string(), "connecting (direct)");
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
        let device_id = Uuid::now_v7();
        let public_key = WgPrivateKey::generate().public_key();

        let identity = DeviceIdentity::new(device_id, public_key.clone());

        assert_eq!(identity.device_id, device_id);
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
}
