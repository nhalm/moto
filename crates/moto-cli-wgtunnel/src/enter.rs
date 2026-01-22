//! Garage enter command implementation.
//!
//! This module provides the `moto garage enter <name>` command, which
//! establishes a `WireGuard` tunnel to a garage and opens an SSH session.
//!
//! # Connection Flow
//!
//! 1. Ensure device identity exists (WG keypair, device ID)
//! 2. Register device with moto-club (if not already registered)
//! 3. Request tunnel session from moto-club
//! 4. Configure `WireGuard` tunnel
//! 5. Attempt direct UDP connection (3s timeout)
//! 6. Fall back to DERP relay if direct fails
//! 7. Open SSH session over the tunnel
//!
//! # Example Output
//!
//! ```text
//! $ moto garage enter feature-foo
//!
//! Connecting to garage feature-foo...
//!   Creating session... done
//!   Configuring tunnel... done
//!   Attempting direct connection... timeout
//!   Using DERP relay (primary)... connected
//!   Opening SSH session... done
//!
//! moto@feature-foo:/workspace$
//! ```

// Allow similar names like garage_id and garage_ip which are distinct in context
#![allow(clippy::similar_names)]

use std::time::Duration;

use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{TunnelError, TunnelManager, TunnelSession, TunnelStatus};

/// Default timeout for direct UDP connection attempts (seconds).
pub const DEFAULT_DIRECT_TIMEOUT_SECS: u64 = 3;

/// Default timeout for DERP connection attempts (seconds).
pub const DEFAULT_DERP_TIMEOUT_SECS: u64 = 10;

/// Default session TTL when not specified (follows garage TTL).
pub const DEFAULT_SESSION_TTL_SECS: u64 = 14400; // 4 hours

/// Errors that can occur during garage enter.
#[derive(Debug, Error)]
pub enum EnterError {
    /// Tunnel management error.
    #[error("tunnel error: {0}")]
    Tunnel(#[from] TunnelError),

    /// Garage not found.
    #[error("garage not found: {0}")]
    GarageNotFound(String),

    /// User not authorized to access garage.
    #[error("not authorized to access garage: {0}")]
    NotAuthorized(String),

    /// Session creation failed.
    #[error("failed to create session: {0}")]
    SessionCreation(String),

    /// Connection failed (all paths exhausted).
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// SSH session failed.
    #[error("SSH session failed: {0}")]
    SshFailed(String),

    /// moto-club unreachable.
    #[error("moto-club unreachable: {0}")]
    ClubUnreachable(String),

    /// Device registration failed.
    #[error("device registration failed: {0}")]
    DeviceRegistration(String),
}

/// Configuration for the enter command.
#[derive(Debug, Clone)]
pub struct EnterConfig {
    /// Timeout for direct UDP connection attempts.
    pub direct_timeout: Duration,

    /// Timeout for DERP connection attempts.
    pub derp_timeout: Duration,

    /// Session TTL in seconds (None = use garage TTL).
    pub session_ttl: Option<u64>,

    /// Force DERP only (skip direct connection attempts).
    pub derp_only: bool,
}

impl Default for EnterConfig {
    fn default() -> Self {
        Self {
            direct_timeout: Duration::from_secs(DEFAULT_DIRECT_TIMEOUT_SECS),
            derp_timeout: Duration::from_secs(DEFAULT_DERP_TIMEOUT_SECS),
            session_ttl: None,
            derp_only: std::env::var("MOTO_WGTUNNEL_DERP_ONLY").is_ok(),
        }
    }
}

impl EnterConfig {
    /// Create a new enter configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the direct connection timeout.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_direct_timeout(mut self, timeout: Duration) -> Self {
        self.direct_timeout = timeout;
        self
    }

    /// Set the DERP connection timeout.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_derp_timeout(mut self, timeout: Duration) -> Self {
        self.derp_timeout = timeout;
        self
    }

    /// Set the session TTL.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_session_ttl(mut self, ttl: u64) -> Self {
        self.session_ttl = Some(ttl);
        self
    }

    /// Force DERP-only mode (skip direct connection attempts).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_derp_only(mut self, derp_only: bool) -> Self {
        self.derp_only = derp_only;
        self
    }
}

/// Response from moto-club session creation.
///
/// This mirrors the API response from `POST /api/v1/wg/sessions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    /// Session ID assigned by moto-club.
    pub session_id: String,

    /// Garage information.
    pub garage: GarageWgInfo,

    /// Client's assigned overlay IP.
    pub client_ip: String,

    /// DERP map for relay fallback.
    pub derp_map: DerpMap,

    /// Session expiration time (ISO 8601).
    pub expires_at: String,
}

/// `WireGuard` information for a garage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarageWgInfo {
    /// Garage's `WireGuard` public key (base64).
    pub public_key: String,

    /// Garage's overlay IP.
    pub overlay_ip: String,

    /// Garage's direct endpoints (if known).
    pub endpoints: Vec<String>,
}

/// Progress callback for enter command status updates.
pub trait EnterProgress: Send + Sync {
    /// Called when a step starts.
    fn step_start(&self, step: &str);

    /// Called when a step completes successfully.
    fn step_done(&self, step: &str);

    /// Called when a step fails.
    fn step_failed(&self, step: &str, error: &str);

    /// Called when connection path is determined.
    fn path_info(&self, path: &str);
}

/// Silent progress handler (no output).
pub struct SilentProgress;

impl EnterProgress for SilentProgress {
    fn step_start(&self, _step: &str) {}
    fn step_done(&self, _step: &str) {}
    fn step_failed(&self, _step: &str, _error: &str) {}
    fn path_info(&self, _path: &str) {}
}

/// Console progress handler (prints to stderr).
pub struct ConsoleProgress {
    quiet: bool,
}

impl ConsoleProgress {
    /// Create a new console progress handler.
    #[must_use]
    pub const fn new(quiet: bool) -> Self {
        Self { quiet }
    }
}

impl EnterProgress for ConsoleProgress {
    fn step_start(&self, step: &str) {
        if !self.quiet {
            eprint!("  {step}... ");
        }
    }

    fn step_done(&self, _step: &str) {
        if !self.quiet {
            eprintln!("done");
        }
    }

    fn step_failed(&self, _step: &str, error: &str) {
        if !self.quiet {
            eprintln!("{error}");
        }
    }

    fn path_info(&self, path: &str) {
        if !self.quiet {
            eprintln!("  Connection: {path}");
        }
    }
}

/// Result of a successful enter operation.
#[derive(Debug, Clone)]
pub struct EnterResult {
    /// Session ID.
    pub session_id: String,

    /// Garage name.
    pub garage_name: String,

    /// Garage ID.
    pub garage_id: String,

    /// Client's overlay IP.
    pub client_ip: OverlayIp,

    /// Garage's overlay IP.
    pub garage_ip: OverlayIp,

    /// Connection path type ("direct" or "derp").
    pub path_type: String,

    /// Connection path detail (endpoint or DERP region).
    pub path_detail: String,
}

/// Handle for an active garage session.
///
/// This handle represents an established tunnel to a garage.
/// When dropped, the tunnel remains active until explicitly closed.
pub struct GarageSession {
    /// Tunnel manager reference.
    manager: TunnelManager,

    /// Session ID.
    session_id: String,

    /// Garage name.
    garage_name: String,

    /// Garage's overlay IP (for SSH connection).
    garage_ip: OverlayIp,
}

impl GarageSession {
    /// Get the session ID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the garage name.
    #[must_use]
    pub fn garage_name(&self) -> &str {
        &self.garage_name
    }

    /// Get the garage's overlay IP.
    #[must_use]
    pub const fn garage_ip(&self) -> OverlayIp {
        self.garage_ip
    }

    /// Get the SSH connection target (`garage_ip:22`).
    #[must_use]
    pub fn ssh_target(&self) -> String {
        format!("[{}]:22", self.garage_ip)
    }

    /// Close the session explicitly.
    ///
    /// This removes the session from the tunnel manager.
    /// The tunnel will be torn down.
    pub async fn close(self) {
        let _ = self.manager.remove_session(&self.session_id).await;
        info!(
            session_id = %self.session_id,
            garage = %self.garage_name,
            "garage session closed"
        );
    }
}

/// Enter a garage and establish a `WireGuard` tunnel.
///
/// This function:
/// 1. Ensures the device identity exists (creates if needed)
/// 2. Registers the device with moto-club (if not already registered)
/// 3. Creates a tunnel session with moto-club
/// 4. Establishes the `WireGuard` tunnel (direct or DERP)
///
/// # Arguments
///
/// * `manager` - Tunnel manager (must be initialized)
/// * `garage_name` - Name of the garage to enter
/// * `config` - Enter configuration
/// * `progress` - Progress callback for status updates
///
/// # Returns
///
/// A `GarageSession` handle for the established tunnel.
///
/// # Errors
///
/// Returns an error if:
/// - Garage is not found
/// - User is not authorized
/// - moto-club is unreachable
/// - Connection fails (all paths exhausted)
///
/// # Example
///
/// ```ignore
/// use moto_cli_wgtunnel::{TunnelManager, enter::{enter_garage, EnterConfig, ConsoleProgress}};
///
/// let manager = TunnelManager::new().await?;
/// let config = EnterConfig::default();
/// let progress = ConsoleProgress::new(false);
///
/// let session = enter_garage(&manager, "my-garage", config, &progress).await?;
/// println!("Connected! SSH target: {}", session.ssh_target());
/// ```
#[allow(clippy::too_many_lines)]
pub async fn enter_garage(
    manager: &TunnelManager,
    garage_name: &str,
    config: EnterConfig,
    progress: &dyn EnterProgress,
) -> Result<GarageSession, EnterError> {
    info!(garage = %garage_name, "entering garage");

    // Step 1: Ensure device is registered
    register_device(manager, progress);

    // Step 2: Create session with moto-club
    progress.step_start("Creating session");
    let session_response = create_mock_session(garage_name, &config);
    progress.step_done("Creating session");

    // Parse session response
    let (client_ip, garage_ip, garage_public_key) = parse_session_response(&session_response)?;

    // Step 3: Create tunnel session
    let tunnel_session = TunnelSession::new(
        session_response.session_id.clone(),
        garage_name.to_string(),
        garage_name.to_string(),
        client_ip,
        garage_ip,
        garage_public_key,
        session_response.derp_map,
    );
    manager.add_session(tunnel_session).await?;

    // Step 4: Configure WireGuard tunnel
    progress.step_start("Configuring tunnel");
    manager
        .configure_wg_tunnel(&session_response.session_id)
        .await
        .map_err(EnterError::Tunnel)?;
    progress.step_done("Configuring tunnel");

    // Step 4b: Initiate WireGuard handshake
    let handshake_packets = manager
        .initiate_handshake(&session_response.session_id)
        .await
        .map_err(EnterError::Tunnel)?;
    debug!(
        packet_count = handshake_packets.len(),
        "WireGuard handshake initiated"
    );

    // Step 5: Establish connection
    let connection_result = establish_connection(
        &session_response.session_id,
        &config,
        progress,
    )
    .await;

    let (path_type, path_detail) = match connection_result {
        Ok(result) => result,
        Err(e) => {
            manager.remove_session(&session_response.session_id).await;
            return Err(e);
        }
    };

    // Update session status
    let path = create_path_type(&path_type, &path_detail);
    manager
        .update_session_status(&session_response.session_id, TunnelStatus::Connected { path })
        .await?;

    progress.path_info(&format!("{path_type} ({path_detail})"));
    info!(
        session_id = %session_response.session_id,
        garage = %garage_name,
        path_type = %path_type,
        "garage tunnel established"
    );

    // Create session handle
    let session_manager = TunnelManager::with_config_dir(manager.config_dir().to_path_buf())
        .await
        .map_err(EnterError::Tunnel)?;

    Ok(GarageSession {
        manager: session_manager,
        session_id: session_response.session_id,
        garage_name: garage_name.to_string(),
        garage_ip,
    })
}

/// Register device with moto-club (or verify identity exists).
fn register_device(manager: &TunnelManager, progress: &dyn EnterProgress) {
    progress.step_start("Registering device");
    let device_info = manager.device_info();
    debug!(
        device_id = %device_info.device_id,
        public_key = %device_info.public_key,
        "device identity ready"
    );
    // TODO: Actually register with moto-club when moto-club-wg is implemented
    progress.step_done("Registering device");
}

/// Parse the session response into typed values.
fn parse_session_response(
    response: &SessionResponse,
) -> Result<(OverlayIp, OverlayIp, WgPublicKey), EnterError> {
    let client_ip: OverlayIp = response
        .client_ip
        .parse()
        .map_err(|e| EnterError::SessionCreation(format!("invalid client IP: {e}")))?;

    let garage_ip: OverlayIp = response
        .garage
        .overlay_ip
        .parse()
        .map_err(|e| EnterError::SessionCreation(format!("invalid garage IP: {e}")))?;

    let garage_public_key = WgPublicKey::from_base64(&response.garage.public_key)
        .map_err(|e| EnterError::SessionCreation(format!("invalid garage public key: {e}")))?;

    Ok((client_ip, garage_ip, garage_public_key))
}

/// Establish connection to garage (try direct, then DERP).
async fn establish_connection(
    session_id: &str,
    config: &EnterConfig,
    progress: &dyn EnterProgress,
) -> Result<(String, String), EnterError> {
    if config.derp_only {
        progress.step_start("Connecting via DERP");
        match attempt_derp_connection(session_id, config).await {
            Ok(region) => {
                progress.step_done("Connecting via DERP");
                Ok(("derp".to_string(), region))
            }
            Err(e) => {
                progress.step_failed("Connecting via DERP", &e.to_string());
                Err(e)
            }
        }
    } else {
        progress.step_start("Attempting direct connection");
        if let Ok(endpoint) = attempt_direct_connection(session_id, config).await {
            progress.step_done("Attempting direct connection");
            return Ok(("direct".to_string(), endpoint));
        }

        progress.step_failed("Attempting direct connection", "timeout");
        progress.step_start("Using DERP relay");
        match attempt_derp_connection(session_id, config).await {
            Ok(region) => {
                progress.step_done("Using DERP relay");
                Ok(("derp".to_string(), region))
            }
            Err(e) => {
                progress.step_failed("Using DERP relay", &e.to_string());
                Err(e)
            }
        }
    }
}

/// Create a `PathType` from string values.
fn create_path_type(path_type: &str, path_detail: &str) -> moto_wgtunnel_conn::PathType {
    use std::net::SocketAddr;

    if path_type == "direct" {
        let endpoint: SocketAddr = path_detail
            .parse()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().expect("fallback address is valid"));
        moto_wgtunnel_conn::PathType::Direct { endpoint }
    } else {
        moto_wgtunnel_conn::PathType::derp(1, path_detail)
    }
}

/// Attempt a direct UDP connection to the garage.
///
/// Returns the connected endpoint on success.
async fn attempt_direct_connection(
    _session_id: &str,
    config: &EnterConfig,
) -> Result<String, EnterError> {
    // TODO: Actually attempt direct connection when MagicConn is integrated
    // For now, simulate a timeout
    debug!(timeout = ?config.direct_timeout, "attempting direct connection");
    tokio::time::sleep(config.direct_timeout).await;

    // In production, this would return Ok(endpoint) if direct connection succeeds
    // For now, always fail to demonstrate DERP fallback
    Err(EnterError::ConnectionFailed("direct connection timeout".to_string()))
}

/// Attempt DERP relay connection to the garage.
///
/// Tries each DERP region in order until one succeeds.
/// Returns the connected region name on success.
async fn attempt_derp_connection(
    _session_id: &str,
    config: &EnterConfig,
) -> Result<String, EnterError> {
    // TODO: Actually attempt DERP connection when MagicConn is integrated
    // For now, simulate success after a short delay
    debug!(timeout = ?config.derp_timeout, "attempting DERP connection");

    // Simulate connection time (much shorter than timeout for demo)
    tokio::time::sleep(Duration::from_millis(100)).await;

    // In production, this would try each DERP region
    // For now, return a mock success
    Ok("primary".to_string())
}

/// Create a mock session response for testing.
///
/// TODO: Remove this when moto-club-wg is implemented.
fn create_mock_session(garage_name: &str, _config: &EnterConfig) -> SessionResponse {
    use moto_wgtunnel_types::WgPrivateKey;

    // Generate deterministic-ish session ID from garage name
    let session_id = format!(
        "sess_{:x}",
        garage_name
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(u64::from(b)))
    );

    // Generate a mock garage keypair
    let garage_key = WgPrivateKey::generate();

    // Create mock garage IP from name hash
    let garage_index = garage_name
        .as_bytes()
        .iter()
        .fold(0u64, |acc, &b| acc.wrapping_add(u64::from(b)));
    let garage_ip = OverlayIp::garage(garage_index);

    // Client gets a fixed IP for now
    let client_ip = OverlayIp::client(1);

    warn!(
        garage = %garage_name,
        "using mock session - moto-club integration not yet implemented"
    );

    SessionResponse {
        session_id,
        garage: GarageWgInfo {
            public_key: garage_key.public_key().to_base64(),
            overlay_ip: garage_ip.to_string(),
            endpoints: vec![],
        },
        client_ip: client_ip.to_string(),
        derp_map: DerpMap::new(),
        expires_at: "2099-12-31T23:59:59Z".to_string(),
    }
}

/// Check if there's an existing session for a garage.
///
/// If a session exists and is still valid, returns it for reattachment.
pub async fn get_existing_session(
    manager: &TunnelManager,
    garage_name: &str,
) -> Option<TunnelSession> {
    let session = manager.get_session_by_garage(garage_name).await?;

    // Only return if session is connected or connecting
    match session.status() {
        TunnelStatus::Connected { .. }
        | TunnelStatus::ConnectingDirect
        | TunnelStatus::ConnectingDerp { .. } => Some(session),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_config_defaults() {
        let config = EnterConfig::default();
        assert_eq!(config.direct_timeout, Duration::from_secs(3));
        assert_eq!(config.derp_timeout, Duration::from_secs(10));
        assert!(config.session_ttl.is_none());
    }

    #[test]
    fn enter_config_builder() {
        let config = EnterConfig::new()
            .with_direct_timeout(Duration::from_secs(5))
            .with_derp_timeout(Duration::from_secs(15))
            .with_session_ttl(7200)
            .with_derp_only(true);

        assert_eq!(config.direct_timeout, Duration::from_secs(5));
        assert_eq!(config.derp_timeout, Duration::from_secs(15));
        assert_eq!(config.session_ttl, Some(7200));
        assert!(config.derp_only);
    }

    #[test]
    fn mock_session_creation() {
        let config = EnterConfig::default();
        let response = create_mock_session("test-garage", &config);

        assert!(!response.session_id.is_empty());
        assert!(!response.garage.public_key.is_empty());
        assert!(!response.garage.overlay_ip.is_empty());
        assert!(!response.client_ip.is_empty());
    }

    #[test]
    fn console_progress_creation() {
        let progress = ConsoleProgress::new(true);
        assert!(progress.quiet);

        let progress = ConsoleProgress::new(false);
        assert!(!progress.quiet);
    }

    #[tokio::test]
    async fn enter_result_fields() {
        let result = EnterResult {
            session_id: "sess_123".to_string(),
            garage_name: "test-garage".to_string(),
            garage_id: "garage_abc".to_string(),
            client_ip: OverlayIp::client(1),
            garage_ip: OverlayIp::garage(1),
            path_type: "direct".to_string(),
            path_detail: "1.2.3.4:51820".to_string(),
        };

        assert_eq!(result.session_id, "sess_123");
        assert_eq!(result.garage_name, "test-garage");
        assert_eq!(result.path_type, "direct");
    }
}
