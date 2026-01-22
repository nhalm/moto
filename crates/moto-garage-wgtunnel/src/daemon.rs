//! Main daemon loop for the garage wgtunnel.
//!
//! The daemon coordinates all components of the garage-side `WireGuard` tunnel:
//!
//! 1. Registration with moto-club on startup
//! 2. WebSocket connection for peer streaming updates
//! 3. `WireGuard` tunnel management via boringtun
//! 4. Health endpoint for Kubernetes probes
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                              Daemon                                          │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │  Event Loop                                                          │    │
//! │  │  ├── Peer updates from WebSocket → add/remove WG peers               │    │
//! │  │  ├── WireGuard timer ticks → keepalive, handshake                    │    │
//! │  │  ├── Health check updates → update health state                      │    │
//! │  │  └── Shutdown signal → graceful shutdown                             │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use moto_garage_wgtunnel::daemon::{Daemon, DaemonConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = DaemonConfig {
//!     moto_club_url: "https://moto-club.example.com".to_string(),
//!     garage_id: "my-garage".to_string(),
//!     auth_token: "k8s-service-account-token".to_string(),
//!     health_port: 8080,
//! };
//!
//! let daemon = Daemon::new(config)?;
//! daemon.run().await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use moto_wgtunnel_types::keys::{WgPrivateKey, WgPublicKey};
use moto_wgtunnel_types::peer::PeerAction;

use crate::health::{HealthCheck, WireGuardState};
use crate::register::{GarageRegistrar, RegistrationConfig, RegistrationResponse};

/// Default health check port.
pub const DEFAULT_HEALTH_PORT: u16 = 8080;

/// Default WebSocket reconnect delay.
pub const DEFAULT_RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Default `WireGuard` timer tick interval.
pub const DEFAULT_TIMER_TICK: Duration = Duration::from_secs(1);

/// Disconnect grace period before removing peers (5 minutes per spec).
pub const DISCONNECT_GRACE_PERIOD: Duration = Duration::from_secs(300);

/// Error type for daemon operations.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// Registration with moto-club failed.
    #[error("registration failed: {0}")]
    Registration(#[from] crate::register::RegistrationError),

    /// WebSocket connection error.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// `WireGuard` tunnel error.
    #[error("WireGuard error: {0}")]
    WireGuard(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// Shutdown requested.
    #[error("shutdown requested")]
    Shutdown,
}

/// Result type for daemon operations.
pub type Result<T> = std::result::Result<T, DaemonError>;

/// Configuration for the daemon.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Base URL of the moto-club server.
    pub moto_club_url: String,

    /// Unique identifier for this garage.
    pub garage_id: String,

    /// Authentication token (K8s service account token).
    pub auth_token: String,

    /// Port for health endpoint.
    pub health_port: u16,

    /// `WireGuard` keepalive interval.
    pub keepalive_secs: u16,

    /// Whether to enable retry on registration failure.
    pub registration_retry: bool,
}

impl DaemonConfig {
    /// Create a new daemon config with required fields.
    #[must_use]
    pub const fn new(moto_club_url: String, garage_id: String, auth_token: String) -> Self {
        Self {
            moto_club_url,
            garage_id,
            auth_token,
            health_port: DEFAULT_HEALTH_PORT,
            keepalive_secs: 25,
            registration_retry: true,
        }
    }

    /// Set the health endpoint port.
    #[must_use]
    pub const fn with_health_port(mut self, port: u16) -> Self {
        self.health_port = port;
        self
    }

    /// Set the `WireGuard` keepalive interval.
    #[must_use]
    pub const fn with_keepalive(mut self, secs: u16) -> Self {
        self.keepalive_secs = secs;
        self
    }

    /// Disable registration retry.
    #[must_use]
    pub const fn without_registration_retry(mut self) -> Self {
        self.registration_retry = false;
        self
    }

    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn validate(&self) -> Result<()> {
        if self.moto_club_url.is_empty() {
            return Err(DaemonError::Config(
                "moto_club_url is required".to_string(),
            ));
        }

        if self.garage_id.is_empty() {
            return Err(DaemonError::Config("garage_id is required".to_string()));
        }

        if self.auth_token.is_empty() {
            return Err(DaemonError::Config("auth_token is required".to_string()));
        }

        Ok(())
    }

    /// Get the peer streaming WebSocket URL.
    #[must_use]
    pub fn peer_stream_url(&self) -> String {
        let base = self.moto_club_url.trim_end_matches('/');
        // Convert http(s) to ws(s)
        let ws_base = if base.starts_with("https://") {
            base.replace("https://", "wss://")
        } else if base.starts_with("http://") {
            base.replace("http://", "ws://")
        } else {
            format!("wss://{base}")
        };
        format!("{ws_base}/internal/wg/garages/{}/peers", self.garage_id)
    }
}

/// State of an active peer connection.
#[derive(Debug)]
pub struct PeerState {
    /// The peer's public key.
    pub public_key: WgPublicKey,

    /// When the peer was added.
    pub added_at: std::time::Instant,

    /// Last activity time (for grace period tracking).
    pub last_activity: std::time::Instant,
}

impl PeerState {
    /// Create a new peer state.
    #[must_use]
    pub fn new(public_key: WgPublicKey) -> Self {
        let now = std::time::Instant::now();
        Self {
            public_key,
            added_at: now,
            last_activity: now,
        }
    }

    /// Update last activity time.
    pub fn touch(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    /// Check if the peer has exceeded the grace period.
    #[must_use]
    pub fn grace_period_exceeded(&self) -> bool {
        self.last_activity.elapsed() > DISCONNECT_GRACE_PERIOD
    }
}

/// The main daemon that runs in the garage pod.
///
/// Coordinates registration, peer streaming, and `WireGuard` tunnel management.
pub struct Daemon {
    /// Daemon configuration.
    config: DaemonConfig,

    /// Ephemeral `WireGuard` private key (generated on startup).
    private_key: WgPrivateKey,

    /// Health check state (shared with health endpoint).
    health: Arc<HealthCheck>,

    /// Active peers (keyed by public key string).
    peers: HashMap<String, PeerState>,

    /// Shutdown signal sender.
    shutdown_tx: watch::Sender<bool>,

    /// Shutdown signal receiver.
    shutdown_rx: watch::Receiver<bool>,

    /// Registration response (set after successful registration).
    registration: Option<RegistrationResponse>,
}

impl std::fmt::Debug for Daemon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Daemon")
            .field("config", &self.config)
            .field("public_key", &self.private_key.public_key())
            .field("peer_count", &self.peers.len())
            .field("registered", &self.registration.is_some())
            .finish_non_exhaustive()
    }
}

impl Daemon {
    /// Create a new daemon.
    ///
    /// Generates an ephemeral `WireGuard` keypair for this garage instance.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn new(config: DaemonConfig) -> Result<Self> {
        config.validate()?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            config,
            private_key: WgPrivateKey::generate(),
            health: Arc::new(HealthCheck::new()),
            peers: HashMap::new(),
            shutdown_tx,
            shutdown_rx,
            registration: None,
        })
    }

    /// Create a daemon with a specific private key (for testing).
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn with_key(config: DaemonConfig, private_key: WgPrivateKey) -> Result<Self> {
        config.validate()?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            config,
            private_key,
            health: Arc::new(HealthCheck::new()),
            peers: HashMap::new(),
            shutdown_tx,
            shutdown_rx,
            registration: None,
        })
    }

    /// Get the daemon configuration.
    #[must_use]
    pub const fn config(&self) -> &DaemonConfig {
        &self.config
    }

    /// Get our `WireGuard` public key.
    #[must_use]
    pub fn public_key(&self) -> WgPublicKey {
        self.private_key.public_key()
    }

    /// Get the health check state.
    #[must_use]
    pub fn health(&self) -> Arc<HealthCheck> {
        Arc::clone(&self.health)
    }

    /// Get the number of active peers.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Check if the daemon has registered with moto-club.
    #[must_use]
    pub const fn is_registered(&self) -> bool {
        self.registration.is_some()
    }

    /// Get the registration response if registered.
    #[must_use]
    pub const fn registration(&self) -> Option<&RegistrationResponse> {
        self.registration.as_ref()
    }

    /// Request daemon shutdown.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Check if shutdown has been requested.
    #[must_use]
    pub fn is_shutdown_requested(&self) -> bool {
        *self.shutdown_rx.borrow()
    }

    /// Register with moto-club.
    ///
    /// This must be called before running the main event loop.
    ///
    /// # Errors
    ///
    /// Returns error if registration fails.
    ///
    /// # Panics
    ///
    /// This function will not panic under normal operation. The internal
    /// `expect` is guarded by the assignment on the line before.
    pub async fn register(&mut self) -> Result<&RegistrationResponse> {
        let reg_config = RegistrationConfig::new(
            self.config.moto_club_url.clone(),
            self.config.garage_id.clone(),
            self.config.auth_token.clone(),
        );

        let reg_config = if self.config.registration_retry {
            reg_config
        } else {
            reg_config.without_retry()
        };

        let registrar = GarageRegistrar::new(reg_config);

        tracing::info!(
            garage_id = %self.config.garage_id,
            public_key = %self.public_key(),
            "registering with moto-club"
        );

        let response = registrar.register(&self.private_key, &[]).await?;

        tracing::info!(
            assigned_ip = %response.assigned_ip,
            derp_regions = response.derp_map.len(),
            "registered with moto-club"
        );

        self.registration = Some(response);

        // Registration is part of becoming healthy, but we're not fully up yet
        // (still need WG tunnel and WebSocket connection)

        // SAFETY: We just set self.registration to Some above
        Ok(self.registration.as_ref().expect("registration just set"))
    }

    /// Handle a peer action from the WebSocket stream.
    ///
    /// # Arguments
    ///
    /// * `action` - The peer action to process (add or remove)
    pub fn handle_peer_action(&mut self, action: PeerAction) {
        match action {
            PeerAction::Add(peer_info) => {
                let key_str = peer_info.public_key().to_string();

                tracing::info!(
                    public_key = %peer_info.public_key(),
                    allowed_ip = %peer_info.allowed_ip(),
                    endpoint = ?peer_info.endpoint(),
                    "adding peer"
                );

                // Add to our peer map
                let state = PeerState::new(peer_info.public_key().clone());
                self.peers.insert(key_str, state);

                // Update health check
                #[allow(clippy::cast_possible_truncation)]
                self.health.set_active_peers(self.peers.len() as u32);

                // In a real implementation, we would also configure the WireGuard tunnel
                // to accept this peer. This is a placeholder for the tunnel integration.
            }

            PeerAction::Remove { public_key } => {
                let key_str = public_key.to_string();

                tracing::info!(
                    public_key = %public_key,
                    "removing peer"
                );

                // Remove from our peer map
                self.peers.remove(&key_str);

                // Update health check
                #[allow(clippy::cast_possible_truncation)]
                self.health.set_active_peers(self.peers.len() as u32);

                // In a real implementation, we would also remove the peer from
                // the WireGuard tunnel configuration.
            }
        }
    }

    /// Clean up peers that have exceeded the grace period.
    ///
    /// Returns the number of peers removed.
    pub fn cleanup_stale_peers(&mut self) -> usize {
        let stale_keys: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, state)| state.grace_period_exceeded())
            .map(|(key, _)| key.clone())
            .collect();

        let count = stale_keys.len();

        for key in stale_keys {
            if let Some(state) = self.peers.remove(&key) {
                tracing::info!(
                    public_key = %state.public_key,
                    elapsed_secs = state.last_activity.elapsed().as_secs(),
                    "removing stale peer (grace period exceeded)"
                );
            }
        }

        if count > 0 {
            #[allow(clippy::cast_possible_truncation)]
            self.health.set_active_peers(self.peers.len() as u32);
        }

        count
    }

    /// Run the main daemon event loop.
    ///
    /// This is a placeholder implementation that demonstrates the structure
    /// of the event loop. The actual implementation will integrate with:
    /// - WebSocket client for peer streaming
    /// - `WireGuard` tunnel engine
    /// - Health endpoint server
    ///
    /// # Errors
    ///
    /// Returns error if the daemon encounters a fatal error.
    #[allow(clippy::unused_async)]
    pub async fn run(&mut self) -> Result<()> {
        // Ensure we're registered
        if self.registration.is_none() {
            self.register().await?;
        }

        // Mark WireGuard as "up" - in a real implementation this would happen
        // after the tunnel is actually configured
        self.health.set_wireguard_state(WireGuardState::Up);
        self.health.set_moto_club_connected(true);

        tracing::info!(
            garage_id = %self.config.garage_id,
            health_port = self.config.health_port,
            peer_stream_url = %self.config.peer_stream_url(),
            "daemon running"
        );

        // The actual event loop would:
        // 1. Spawn health endpoint server
        // 2. Connect to peer streaming WebSocket
        // 3. Run WireGuard timer ticks
        // 4. Handle incoming packets
        //
        // For now, we just wait for shutdown signal
        let mut shutdown_rx = self.shutdown_rx.clone();
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("shutdown signal received");
                        break;
                    }
                }
            }
        }

        // Graceful shutdown
        self.health.set_wireguard_state(WireGuardState::Down);
        self.health.set_moto_club_connected(false);

        tracing::info!("daemon stopped");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::ip::OverlayIp;
    use moto_wgtunnel_types::peer::PeerInfo;

    fn test_config() -> DaemonConfig {
        DaemonConfig::new(
            "https://moto-club.example.com".to_string(),
            "test-garage".to_string(),
            "test-token".to_string(),
        )
    }

    #[test]
    fn config_creation() {
        let config = test_config();

        assert_eq!(config.moto_club_url, "https://moto-club.example.com");
        assert_eq!(config.garage_id, "test-garage");
        assert_eq!(config.auth_token, "test-token");
        assert_eq!(config.health_port, DEFAULT_HEALTH_PORT);
        assert_eq!(config.keepalive_secs, 25);
        assert!(config.registration_retry);
    }

    #[test]
    fn config_builders() {
        let config = DaemonConfig::new(
            "https://moto-club.example.com".to_string(),
            "test".to_string(),
            "token".to_string(),
        )
        .with_health_port(9090)
        .with_keepalive(30)
        .without_registration_retry();

        assert_eq!(config.health_port, 9090);
        assert_eq!(config.keepalive_secs, 30);
        assert!(!config.registration_retry);
    }

    #[test]
    fn config_validation() {
        // Valid config
        let config = test_config();
        assert!(config.validate().is_ok());

        // Empty URL
        let config = DaemonConfig::new(
            String::new(),
            "garage".to_string(),
            "token".to_string(),
        );
        assert!(matches!(config.validate(), Err(DaemonError::Config(_))));

        // Empty garage_id
        let config = DaemonConfig::new(
            "https://example.com".to_string(),
            String::new(),
            "token".to_string(),
        );
        assert!(matches!(config.validate(), Err(DaemonError::Config(_))));

        // Empty auth_token
        let config = DaemonConfig::new(
            "https://example.com".to_string(),
            "garage".to_string(),
            String::new(),
        );
        assert!(matches!(config.validate(), Err(DaemonError::Config(_))));
    }

    #[test]
    fn config_peer_stream_url() {
        let config = test_config();
        assert_eq!(
            config.peer_stream_url(),
            "wss://moto-club.example.com/internal/wg/garages/test-garage/peers"
        );

        // With http
        let config = DaemonConfig::new(
            "http://localhost:8080".to_string(),
            "my-garage".to_string(),
            "token".to_string(),
        );
        assert_eq!(
            config.peer_stream_url(),
            "ws://localhost:8080/internal/wg/garages/my-garage/peers"
        );

        // With trailing slash
        let config = DaemonConfig::new(
            "https://moto-club.example.com/".to_string(),
            "garage".to_string(),
            "token".to_string(),
        );
        assert_eq!(
            config.peer_stream_url(),
            "wss://moto-club.example.com/internal/wg/garages/garage/peers"
        );
    }

    #[test]
    fn daemon_creation() {
        let config = test_config();
        let daemon = Daemon::new(config).unwrap();

        assert!(!daemon.is_registered());
        assert!(!daemon.is_shutdown_requested());
        assert_eq!(daemon.peer_count(), 0);
    }

    #[test]
    fn daemon_with_key() {
        let config = test_config();
        let private_key = WgPrivateKey::generate();
        let expected_public = private_key.public_key();

        let daemon = Daemon::with_key(config, private_key).unwrap();

        assert_eq!(daemon.public_key(), expected_public);
    }

    #[test]
    fn daemon_shutdown() {
        let config = test_config();
        let daemon = Daemon::new(config).unwrap();

        assert!(!daemon.is_shutdown_requested());

        daemon.shutdown();

        assert!(daemon.is_shutdown_requested());
    }

    #[test]
    fn handle_peer_add() {
        let config = test_config();
        let mut daemon = Daemon::new(config).unwrap();

        let peer_key = WgPrivateKey::generate().public_key();
        let peer_info = PeerInfo::new(peer_key.clone(), OverlayIp::client(1));

        daemon.handle_peer_action(PeerAction::add(peer_info));

        assert_eq!(daemon.peer_count(), 1);
        assert_eq!(daemon.health.active_peers(), 1);
    }

    #[test]
    fn handle_peer_remove() {
        let config = test_config();
        let mut daemon = Daemon::new(config).unwrap();

        let peer_key = WgPrivateKey::generate().public_key();
        let peer_info = PeerInfo::new(peer_key.clone(), OverlayIp::client(1));

        // Add then remove
        daemon.handle_peer_action(PeerAction::add(peer_info));
        assert_eq!(daemon.peer_count(), 1);

        daemon.handle_peer_action(PeerAction::remove(peer_key));
        assert_eq!(daemon.peer_count(), 0);
        assert_eq!(daemon.health.active_peers(), 0);
    }

    #[test]
    fn handle_multiple_peers() {
        let config = test_config();
        let mut daemon = Daemon::new(config).unwrap();

        for i in 0..5 {
            let peer_key = WgPrivateKey::generate().public_key();
            let peer_info = PeerInfo::new(peer_key, OverlayIp::client(i));
            daemon.handle_peer_action(PeerAction::add(peer_info));
        }

        assert_eq!(daemon.peer_count(), 5);
        assert_eq!(daemon.health.active_peers(), 5);
    }

    #[test]
    fn peer_state_creation() {
        let public_key = WgPrivateKey::generate().public_key();
        let state = PeerState::new(public_key.clone());

        assert_eq!(state.public_key, public_key);
        assert!(!state.grace_period_exceeded());
    }

    #[test]
    fn peer_state_touch() {
        let public_key = WgPrivateKey::generate().public_key();
        let mut state = PeerState::new(public_key);

        let before = state.last_activity;
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.touch();

        assert!(state.last_activity > before);
    }

    #[test]
    fn cleanup_stale_peers_none() {
        let config = test_config();
        let mut daemon = Daemon::new(config).unwrap();

        // Add a fresh peer
        let peer_key = WgPrivateKey::generate().public_key();
        let peer_info = PeerInfo::new(peer_key, OverlayIp::client(1));
        daemon.handle_peer_action(PeerAction::add(peer_info));

        // Fresh peer should not be cleaned up
        let removed = daemon.cleanup_stale_peers();
        assert_eq!(removed, 0);
        assert_eq!(daemon.peer_count(), 1);
    }

    #[test]
    fn health_shared() {
        let config = test_config();
        let daemon = Daemon::new(config).unwrap();

        let health1 = daemon.health();
        let health2 = daemon.health();

        health1.set_active_peers(5);
        assert_eq!(health2.active_peers(), 5);
    }

    #[test]
    fn error_display() {
        let err = DaemonError::Config("test error".to_string());
        assert!(err.to_string().contains("test error"));

        let err = DaemonError::WebSocket("connection failed".to_string());
        assert!(err.to_string().contains("connection failed"));

        let err = DaemonError::WireGuard("tunnel error".to_string());
        assert!(err.to_string().contains("tunnel error"));

        let err = DaemonError::Shutdown;
        assert!(err.to_string().contains("shutdown"));
    }
}
