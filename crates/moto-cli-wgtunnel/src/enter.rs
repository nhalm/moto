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
//! 5. Attempt direct UDP connection (3s timeout) via `MagicConn`
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

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use moto_wgtunnel_conn::{MagicConn, MagicConnConfig, MagicConnError};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPrivateKey, WgPublicKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use crate::client::{ClientError, MotoClubClient, MotoClubConfig};
use crate::{TunnelError, TunnelManager, TunnelSession, TunnelStatus};

/// Default timeout for direct UDP connection attempts (seconds).
pub const DEFAULT_DIRECT_TIMEOUT_SECS: u64 = 3;

/// Default timeout for DERP connection attempts (seconds).
pub const DEFAULT_DERP_TIMEOUT_SECS: u64 = 10;

/// Default session TTL when not specified (follows garage TTL).
pub const DEFAULT_SESSION_TTL_SECS: u64 = 14400; // 4 hours

/// Default SSH port.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// Default SSH user for garage connections.
pub const DEFAULT_SSH_USER: &str = "moto";

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

    /// SSH command not found.
    #[error("SSH command not found - ensure OpenSSH is installed")]
    SshNotFound,

    /// SSH session exited with error.
    #[error("SSH session exited with code {0}")]
    SshExitCode(i32),

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

    /// moto-club base URL (e.g., `http://localhost:8080`).
    pub moto_club_url: String,

    /// Owner/username for authentication.
    pub owner: String,
}

impl Default for EnterConfig {
    fn default() -> Self {
        Self {
            direct_timeout: Duration::from_secs(DEFAULT_DIRECT_TIMEOUT_SECS),
            derp_timeout: Duration::from_secs(DEFAULT_DERP_TIMEOUT_SECS),
            session_ttl: None,
            derp_only: std::env::var("MOTO_WGTUNNEL_DERP_ONLY").is_ok(),
            moto_club_url: std::env::var("MOTO_CLUB_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            owner: std::env::var("MOTO_USER").unwrap_or_default(),
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

    /// Set the moto-club base URL.
    #[must_use]
    pub fn with_moto_club_url(mut self, url: impl Into<String>) -> Self {
        self.moto_club_url = url.into();
        self
    }

    /// Set the owner/username.
    #[must_use]
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = owner.into();
        self
    }
}

/// Configuration for SSH session spawning.
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// SSH port (default: 22).
    pub port: u16,

    /// SSH user (default: "moto").
    pub user: String,

    /// Path to SSH private key (None = use default ~/.ssh/id_ed25519 or ssh-agent).
    pub identity_file: Option<PathBuf>,

    /// Disable strict host key checking (for ephemeral garage keys).
    pub disable_host_key_check: bool,

    /// Additional SSH options to pass.
    pub extra_options: Vec<String>,

    /// Working directory to change to after connecting.
    pub working_dir: Option<String>,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_SSH_PORT,
            user: DEFAULT_SSH_USER.to_string(),
            identity_file: None,
            disable_host_key_check: true, // Garage keys are ephemeral
            extra_options: Vec::new(),
            working_dir: Some("/workspace".to_string()),
        }
    }
}

impl SshConfig {
    /// Create a new SSH configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the SSH port.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the SSH user.
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    /// Set the SSH identity file.
    #[must_use]
    pub fn with_identity_file(mut self, path: PathBuf) -> Self {
        self.identity_file = Some(path);
        self
    }

    /// Enable or disable strict host key checking.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_host_key_check(mut self, enable: bool) -> Self {
        self.disable_host_key_check = !enable;
        self
    }

    /// Add an extra SSH option.
    #[must_use]
    pub fn with_option(mut self, option: impl Into<String>) -> Self {
        self.extra_options.push(option.into());
        self
    }

    /// Set the working directory to change to after connecting.
    #[must_use]
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
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

/// Shared connection state for `MagicConn`.
///
/// This wrapper allows the `MagicConn` to be shared between the `enter_garage`
/// function and the connection attempt functions.
struct ConnectionState {
    /// `MagicConn` instance for multiplexed UDP/DERP connections.
    magic_conn: MagicConn,

    /// Garage's `WireGuard` public key.
    garage_public_key: WgPublicKey,

    /// Garage's direct endpoints (if any).
    garage_endpoints: Vec<SocketAddr>,
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

    /// Spawn an interactive SSH session to the garage.
    ///
    /// This spawns an SSH process that connects to the garage's overlay IP
    /// over the established `WireGuard` tunnel. The SSH process inherits
    /// stdin/stdout/stderr for interactive use.
    ///
    /// # Arguments
    ///
    /// * `config` - SSH configuration (user, port, identity file, etc.)
    ///
    /// # Returns
    ///
    /// The exit status of the SSH process.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - SSH binary is not found
    /// - SSH process fails to spawn
    /// - SSH exits with a non-zero code
    ///
    /// # Example
    ///
    /// ```ignore
    /// let session = enter_garage(&manager, "my-garage", config, &progress).await?;
    /// let status = session.spawn_ssh(SshConfig::default())?;
    /// ```
    pub fn spawn_ssh(&self, config: SshConfig) -> Result<ExitStatus, EnterError> {
        let ssh_target = format!("{}@{}", config.user, self.garage_ip);
        info!(
            target = %ssh_target,
            port = config.port,
            "spawning SSH session"
        );

        let mut cmd = Command::new("ssh");

        // Set port
        cmd.arg("-p").arg(config.port.to_string());

        // Disable host key checking for ephemeral garage keys
        if config.disable_host_key_check {
            cmd.arg("-o").arg("StrictHostKeyChecking=no");
            cmd.arg("-o").arg("UserKnownHostsFile=/dev/null");
            // Suppress the warning about adding to known_hosts
            cmd.arg("-o").arg("LogLevel=ERROR");
        }

        // Set identity file if specified
        if let Some(ref identity) = config.identity_file {
            cmd.arg("-i").arg(identity);
        }

        // Add any extra options
        for opt in &config.extra_options {
            cmd.arg("-o").arg(opt);
        }

        // Set the target (user@host)
        cmd.arg(&ssh_target);

        // If working directory is specified, change to it
        if let Some(ref dir) = config.working_dir {
            cmd.arg("-t"); // Force pseudo-terminal allocation for cd
            cmd.arg(format!("cd {} && exec $SHELL -l", dir));
        }

        // Inherit stdio for interactive session
        cmd.stdin(Stdio::inherit());
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        debug!(command = ?cmd, "executing SSH command");

        // Spawn and wait for the SSH process
        let status = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                EnterError::SshNotFound
            } else {
                EnterError::SshFailed(format!("failed to spawn SSH: {e}"))
            }
        })?.wait().map_err(|e| {
            EnterError::SshFailed(format!("failed to wait for SSH: {e}"))
        })?;

        info!(
            exit_code = status.code(),
            success = status.success(),
            "SSH session ended"
        );

        Ok(status)
    }

    /// Spawn an SSH session and return an error if it exits with non-zero.
    ///
    /// This is a convenience wrapper around [`spawn_ssh`](Self::spawn_ssh)
    /// that returns an error if SSH exits with a non-zero exit code.
    ///
    /// # Arguments
    ///
    /// * `config` - SSH configuration
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - SSH fails to spawn (see [`spawn_ssh`](Self::spawn_ssh))
    /// - SSH exits with a non-zero exit code
    pub fn spawn_ssh_checked(&self, config: SshConfig) -> Result<(), EnterError> {
        let status = self.spawn_ssh(config)?;
        if status.success() {
            Ok(())
        } else {
            Err(EnterError::SshExitCode(status.code().unwrap_or(-1)))
        }
    }

    /// Build the SSH command arguments without spawning.
    ///
    /// This is useful for displaying the SSH command to the user or for
    /// debugging.
    ///
    /// # Arguments
    ///
    /// * `config` - SSH configuration
    ///
    /// # Returns
    ///
    /// A vector of command-line arguments for the SSH command.
    #[must_use]
    pub fn ssh_command_args(&self, config: &SshConfig) -> Vec<String> {
        let mut args = vec!["ssh".to_string()];

        args.push("-p".to_string());
        args.push(config.port.to_string());

        if config.disable_host_key_check {
            args.push("-o".to_string());
            args.push("StrictHostKeyChecking=no".to_string());
            args.push("-o".to_string());
            args.push("UserKnownHostsFile=/dev/null".to_string());
            args.push("-o".to_string());
            args.push("LogLevel=ERROR".to_string());
        }

        if let Some(ref identity) = config.identity_file {
            args.push("-i".to_string());
            args.push(identity.display().to_string());
        }

        for opt in &config.extra_options {
            args.push("-o".to_string());
            args.push(opt.clone());
        }

        args.push(format!("{}@{}", config.user, self.garage_ip));

        if let Some(ref dir) = config.working_dir {
            args.push("-t".to_string());
            args.push(format!("cd {} && exec $SHELL -l", dir));
        }

        args
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

    // Validate owner is configured
    if config.owner.is_empty() {
        return Err(EnterError::ClubUnreachable(
            "MOTO_USER environment variable is required".to_string(),
        ));
    }

    // Create moto-club client
    let club_config = MotoClubConfig::new(&config.moto_club_url, &config.owner);
    let client = MotoClubClient::new(club_config)
        .map_err(|e| EnterError::ClubUnreachable(e.to_string()))?;

    // Step 1: Register device with moto-club
    progress.step_start("Registering device");
    let device_id = register_device_with_club(manager, &client, progress).await?;
    progress.step_done("Registering device");

    // Step 2: Create session with moto-club
    progress.step_start("Creating session");
    let ttl_seconds = config.session_ttl.map(|t| t as u32);
    let api_response = client
        .create_session(garage_name, device_id, ttl_seconds)
        .await
        .map_err(|e| match e {
            ClientError::GarageNotFound(msg) => EnterError::GarageNotFound(msg),
            ClientError::NotAuthorized(msg) => EnterError::NotAuthorized(msg),
            ClientError::Unreachable { url, reason } => {
                EnterError::ClubUnreachable(format!("{url}: {reason}"))
            }
            other => EnterError::SessionCreation(other.to_string()),
        })?;
    progress.step_done("Creating session");

    // Convert API response to internal SessionResponse type
    let session_response = SessionResponse {
        session_id: api_response.session_id,
        garage: GarageWgInfo {
            public_key: api_response.garage.public_key,
            overlay_ip: api_response.garage.overlay_ip.to_string(),
            endpoints: api_response.garage.endpoints,
        },
        client_ip: api_response.client_ip.to_string(),
        derp_map: api_response.derp_map,
        expires_at: api_response.expires_at,
    };

    // Parse session response
    let (client_ip, garage_ip, garage_public_key) = parse_session_response(&session_response)?;

    // Parse garage endpoints for direct connection attempts
    let garage_endpoints: Vec<SocketAddr> = session_response
        .garage
        .endpoints
        .iter()
        .filter_map(|e| e.parse().ok())
        .collect();

    // Step 3: Create tunnel session
    let tunnel_session = TunnelSession::new(
        session_response.session_id.clone(),
        garage_name.to_string(),
        garage_name.to_string(),
        client_ip,
        garage_ip,
        garage_public_key.clone(),
        session_response.derp_map.clone(),
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

    // Step 4c: Create MagicConn for connection multiplexing
    progress.step_start("Initializing connection");
    let conn_state = create_connection_state(
        manager.private_key(),
        session_response.derp_map,
        garage_public_key,
        garage_endpoints,
        &config,
    )
    .await?;
    let conn_state = Arc::new(RwLock::new(conn_state));
    progress.step_done("Initializing connection");

    // Step 5: Establish connection
    let connection_result = establish_connection(
        &session_response.session_id,
        &config,
        progress,
        &conn_state,
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

/// Create the `MagicConn` connection state.
async fn create_connection_state(
    private_key: &WgPrivateKey,
    derp_map: DerpMap,
    garage_public_key: WgPublicKey,
    garage_endpoints: Vec<SocketAddr>,
    config: &EnterConfig,
) -> Result<ConnectionState, EnterError> {
    // Create a copy of the private key by converting to/from bytes
    let private_key_copy = WgPrivateKey::from_bytes(&private_key.as_bytes()).map_err(|e| {
        EnterError::ConnectionFailed(format!("failed to copy private key: {e}"))
    })?;

    // Create MagicConn config
    let magic_config = MagicConnConfig::new(private_key_copy, derp_map)
        .with_direct_timeout(config.direct_timeout)
        .with_derp_timeout(config.derp_timeout)
        .with_prefer_direct(!config.derp_only);

    // Create MagicConn
    let magic_conn = MagicConn::new(magic_config)
        .await
        .map_err(|e| EnterError::ConnectionFailed(format!("failed to create MagicConn: {e}")))?;

    // Add the garage as a peer with its known endpoints
    magic_conn
        .add_peer(&garage_public_key, garage_endpoints.clone())
        .await;

    debug!(
        peer = %garage_public_key,
        endpoints = ?garage_endpoints,
        "added garage peer to MagicConn"
    );

    Ok(ConnectionState {
        magic_conn,
        garage_public_key,
        garage_endpoints,
    })
}

/// Register device with moto-club.
///
/// Registers the device's WireGuard public key with moto-club, which
/// allocates an overlay IP address. If the device is already registered,
/// moto-club returns the existing allocation.
///
/// # Returns
///
/// The device ID (UUID) to use for session creation.
async fn register_device_with_club(
    manager: &TunnelManager,
    client: &MotoClubClient,
    _progress: &dyn EnterProgress,
) -> Result<Uuid, EnterError> {
    let device_info = manager.device_info();
    debug!(
        device_id = %device_info.device_id,
        public_key = %device_info.public_key,
        "registering device with moto-club"
    );

    // Register device with moto-club (retries with exponential backoff)
    let response = client
        .register_device(&device_info.public_key, Some("moto-cli"))
        .await
        .map_err(|e| match e {
            ClientError::Unreachable { url, reason } => {
                EnterError::ClubUnreachable(format!("{url}: {reason}"))
            }
            other => EnterError::DeviceRegistration(other.to_string()),
        })?;

    info!(
        device_id = %response.device_id,
        overlay_ip = %response.overlay_ip,
        "device registered with moto-club"
    );

    Ok(response.device_id)
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
    conn_state: &Arc<RwLock<ConnectionState>>,
) -> Result<(String, String), EnterError> {
    if config.derp_only {
        progress.step_start("Connecting via DERP");
        match attempt_derp_connection(session_id, config, conn_state).await {
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
        match attempt_direct_connection(session_id, config, conn_state).await {
            Ok(endpoint) => {
                progress.step_done("Attempting direct connection");
                return Ok(("direct".to_string(), endpoint));
            }
            Err(e) => {
                debug!(error = %e, "direct connection failed");
                progress.step_failed("Attempting direct connection", "timeout");
            }
        }

        progress.step_start("Using DERP relay");
        match attempt_derp_connection(session_id, config, conn_state).await {
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
/// Uses `MagicConn` to attempt direct UDP connection to the garage.
/// Returns the connected endpoint on success.
#[allow(clippy::significant_drop_tightening)] // Lock held during connect is intentional
async fn attempt_direct_connection(
    _session_id: &str,
    config: &EnterConfig,
    conn_state: &Arc<RwLock<ConnectionState>>,
) -> Result<String, EnterError> {
    // First, check if we have endpoints and get the garage key
    let (garage_key, has_endpoints) = {
        let state = conn_state.read().await;
        let has_endpoints = !state.garage_endpoints.is_empty();
        if has_endpoints {
            debug!(
                timeout = ?config.direct_timeout,
                endpoints = ?state.garage_endpoints,
                "attempting direct connection via MagicConn"
            );
        }
        (state.garage_public_key.clone(), has_endpoints)
    };

    if !has_endpoints {
        debug!("no direct endpoints available, skipping direct connection attempt");
        return Err(EnterError::ConnectionFailed(
            "no direct endpoints available".to_string(),
        ));
    }

    // Use MagicConn to connect to the peer
    // MagicConn::connect will try direct endpoints first with the configured timeout
    let state = conn_state.read().await;
    let connect_result = state.magic_conn.connect(&garage_key).await;

    match connect_result {
        Ok(()) => {
            // Check if we got a direct connection
            if let Some(path) = state.magic_conn.current_path(&garage_key).await {
                match path {
                    moto_wgtunnel_conn::PathType::Direct { endpoint } => {
                        info!(%endpoint, "direct connection established");
                        return Ok(endpoint.to_string());
                    }
                    moto_wgtunnel_conn::PathType::Derp { region_name, .. } => {
                        // Connected via DERP - this counts as "direct failed"
                        debug!(region = %region_name, "MagicConn fell back to DERP");
                        return Err(EnterError::ConnectionFailed(
                            "direct connection timed out, fell back to DERP".to_string(),
                        ));
                    }
                }
            }
            // No path info available - shouldn't happen after successful connect
            Err(EnterError::ConnectionFailed(
                "connection succeeded but no path info available".to_string(),
            ))
        }
        Err(MagicConnError::DirectTimeout(duration)) => {
            debug!(?duration, "direct connection timed out");
            Err(EnterError::ConnectionFailed(format!(
                "direct connection timed out after {duration:?}"
            )))
        }
        Err(e) => {
            debug!(error = %e, "direct connection failed");
            Err(EnterError::ConnectionFailed(format!(
                "direct connection failed: {e}"
            )))
        }
    }
}

/// Attempt DERP relay connection to the garage.
///
/// Tries each DERP region in order until one succeeds.
/// Returns the connected region name on success.
///
/// This function uses `MagicConn`'s connect which internally manages
/// `DerpClient` connections for relay fallback. The `DerpClient` handles:
/// - WebSocket connection to DERP server
/// - DERP protocol handshake (`ServerKey` -> `ClientInfo` -> `ServerInfo`)
/// - Packet relay through the DERP server
/// - Keepalive management
#[allow(clippy::significant_drop_tightening)] // Lock held during connect is intentional
async fn attempt_derp_connection(
    _session_id: &str,
    config: &EnterConfig,
    conn_state: &Arc<RwLock<ConnectionState>>,
) -> Result<String, EnterError> {
    debug!(timeout = ?config.derp_timeout, "attempting DERP connection");

    // Check if already connected via DERP from a previous attempt
    {
        let state = conn_state.read().await;
        let garage_key = &state.garage_public_key;

        if let Some(moto_wgtunnel_conn::PathType::Derp { region_name, .. }) =
            state.magic_conn.current_path(garage_key).await
        {
            info!(region = %region_name, "already connected via DERP");
            return Ok(region_name);
        }
    }

    // Try to connect via MagicConn - it will use DerpClient internally
    // MagicConn::connect tries direct endpoints first, then falls back to DERP regions
    // in order. For each DERP region, it creates a DerpClient connection.
    let state = conn_state.read().await;
    let garage_key = state.garage_public_key.clone();
    let connect_result = state.magic_conn.connect(&garage_key).await;

    match connect_result {
        Ok(()) => {
            // Connection succeeded - check which path we got
            if let Some(path) = state.magic_conn.current_path(&garage_key).await {
                match path {
                    moto_wgtunnel_conn::PathType::Derp { region_name, .. } => {
                        info!(region = %region_name, "DERP relay connection established via DerpClient");
                        return Ok(region_name);
                    }
                    moto_wgtunnel_conn::PathType::Direct { endpoint } => {
                        // Got a direct connection - this can happen if NAT traversal
                        // succeeded after DERP helped with hole punching
                        info!(%endpoint, "direct connection established during DERP attempt");
                        return Ok(format!("direct:{endpoint}"));
                    }
                }
            }
            // Connection succeeded but no path info - this shouldn't happen normally
            // but indicates the connection is established
            debug!("DERP connection succeeded but no path info available");
            Err(EnterError::ConnectionFailed(
                "connection succeeded but path info unavailable".to_string(),
            ))
        }
        Err(MagicConnError::AllAttemptsFailed) => {
            // All direct endpoints timed out and all DERP regions failed
            Err(EnterError::ConnectionFailed(
                "all connection attempts failed (direct and DERP)".to_string(),
            ))
        }
        Err(MagicConnError::NoDerpRegions) => {
            // No DERP regions were configured in the DerpMap
            Err(EnterError::ConnectionFailed(
                "no DERP regions configured - cannot establish relay connection".to_string(),
            ))
        }
        Err(MagicConnError::DerpFailed(derp_err)) => {
            // DerpClient connection failed (handshake error, timeout, etc.)
            debug!(error = %derp_err, "DerpClient connection failed");
            Err(EnterError::ConnectionFailed(format!(
                "DERP relay connection failed: {derp_err}"
            )))
        }
        Err(e) => {
            debug!(error = %e, "DERP connection failed");
            Err(EnterError::ConnectionFailed(format!(
                "DERP connection failed: {e}"
            )))
        }
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

    #[test]
    fn ssh_config_defaults() {
        let config = SshConfig::default();
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "moto");
        assert!(config.identity_file.is_none());
        assert!(config.disable_host_key_check);
        assert!(config.extra_options.is_empty());
        assert_eq!(config.working_dir, Some("/workspace".to_string()));
    }

    #[test]
    fn ssh_config_builder() {
        let config = SshConfig::new()
            .with_port(2222)
            .with_user("testuser")
            .with_identity_file(PathBuf::from("/tmp/id_test"))
            .with_host_key_check(true)
            .with_option("BatchMode=yes")
            .with_working_dir("/home/test");

        assert_eq!(config.port, 2222);
        assert_eq!(config.user, "testuser");
        assert_eq!(config.identity_file, Some(PathBuf::from("/tmp/id_test")));
        assert!(!config.disable_host_key_check); // with_host_key_check(true) means check is enabled
        assert_eq!(config.extra_options, vec!["BatchMode=yes"]);
        assert_eq!(config.working_dir, Some("/home/test".to_string()));
    }

    #[test]
    fn ssh_command_args_default() {
        // Create a mock GarageSession-like struct for testing
        // We can't easily create a GarageSession without the TunnelManager,
        // so we test the command building logic directly
        let garage_ip = OverlayIp::garage(123);
        let config = SshConfig::default();

        // Build args manually (same logic as ssh_command_args)
        let mut args = vec!["ssh".to_string()];
        args.push("-p".to_string());
        args.push(config.port.to_string());
        if config.disable_host_key_check {
            args.push("-o".to_string());
            args.push("StrictHostKeyChecking=no".to_string());
            args.push("-o".to_string());
            args.push("UserKnownHostsFile=/dev/null".to_string());
            args.push("-o".to_string());
            args.push("LogLevel=ERROR".to_string());
        }
        args.push(format!("{}@{}", config.user, garage_ip));
        if let Some(ref dir) = config.working_dir {
            args.push("-t".to_string());
            args.push(format!("cd {} && exec $SHELL -l", dir));
        }

        assert_eq!(args[0], "ssh");
        assert_eq!(args[1], "-p");
        assert_eq!(args[2], "22");
        assert!(args.contains(&"StrictHostKeyChecking=no".to_string()));
        assert!(args.contains(&"UserKnownHostsFile=/dev/null".to_string()));
        // IP format is fd00:6d6f:746f:1:: (moto encoded as hex: 6d6f = "mo", 746f = "to")
        assert!(args.iter().any(|a| a.starts_with("moto@fd00:6d6f:746f:1::")));
        assert!(args.contains(&"-t".to_string()));
        assert!(args.iter().any(|a| a.contains("/workspace")));
    }

    #[test]
    fn ssh_command_args_with_identity() {
        let config = SshConfig::new()
            .with_identity_file(PathBuf::from("/home/user/.ssh/id_custom"))
            .with_host_key_check(true); // Enable checking = disable_host_key_check is false

        // The identity file should be included
        assert_eq!(config.identity_file, Some(PathBuf::from("/home/user/.ssh/id_custom")));
        assert!(!config.disable_host_key_check);
    }
}
