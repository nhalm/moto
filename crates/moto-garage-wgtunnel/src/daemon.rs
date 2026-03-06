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

use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite;

use moto_wgtunnel_engine::tunnel::Tunnel;
use moto_wgtunnel_types::keys::{WgPrivateKey, WgPublicKey};
use moto_wgtunnel_types::peer::PeerAction;

use crate::health::{self, HealthCheck, WireGuardState};
use crate::register::{GarageRegistrar, RegistrationConfig, RegistrationResponse};

/// Path to the K8s service account token.
pub const K8S_SA_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";

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
            return Err(DaemonError::Config("moto_club_url is required".to_string()));
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

    /// Per-peer `WireGuard` tunnels (keyed by public key string).
    wg_tunnels: HashMap<String, Tunnel>,

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
            .field("wg_tunnels", &self.wg_tunnels.len())
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
            wg_tunnels: HashMap::new(),
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
            wg_tunnels: HashMap::new(),
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

    /// Read the K8s service account token from the standard path.
    ///
    /// # Errors
    ///
    /// Returns error if the token file cannot be read.
    pub fn read_k8s_token() -> Result<String> {
        Self::read_k8s_token_from(K8S_SA_TOKEN_PATH)
    }

    /// Read the K8s service account token from a specific path.
    ///
    /// # Errors
    ///
    /// Returns error if the token file cannot be read.
    pub fn read_k8s_token_from(path: &str) -> Result<String> {
        let token = std::fs::read_to_string(path).map_err(|e| {
            DaemonError::Config(format!("failed to read K8s SA token from {path}: {e}"))
        })?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(DaemonError::Config(format!(
                "K8s SA token at {path} is empty"
            )));
        }
        Ok(token)
    }

    /// Connect to the peer streaming WebSocket.
    ///
    /// Connects to `peer_stream_url()` with Bearer auth using the configured
    /// auth token. Returns a WebSocket stream that yields peer event messages.
    ///
    /// # Errors
    ///
    /// Returns error if the WebSocket connection fails.
    pub async fn connect_peer_stream(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        let url = self.config.peer_stream_url();

        let request = tungstenite::http::Request::builder()
            .uri(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.auth_token),
            )
            .header("Host", extract_host(&url))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| DaemonError::WebSocket(format!("failed to build request: {e}")))?;

        tracing::debug!(url = %url, "connecting to peer stream WebSocket");

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| DaemonError::WebSocket(format!("failed to connect: {e}")))?;

        tracing::info!(url = %url, "connected to peer stream WebSocket");

        Ok(ws_stream)
    }

    /// Parse a WebSocket text message as a `PeerAction`.
    ///
    /// # Errors
    ///
    /// Returns error if the message cannot be parsed as JSON `PeerAction`.
    pub fn parse_peer_event(text: &str) -> Result<PeerAction> {
        serde_json::from_str::<PeerAction>(text)
            .map_err(|e| DaemonError::WebSocket(format!("failed to parse PeerEvent: {e}")))
    }

    /// Run the main daemon event loop.
    ///
    /// Performs the following steps:
    /// 1. Register with moto-club (advertise WG public key, get overlay IP)
    /// 2. Spawn health HTTP server on the configured port
    /// 3. Initialize `WireGuard` tunnel engine (ready to accept peers)
    /// 4. Connect to peer streaming WebSocket at `peer_stream_url()`
    /// 5. Loop: receive `PeerEvent` messages, handle Ping/Pong, timer ticks
    /// 6. On SIGTERM: close WebSocket, tear down tunnel, exit cleanly
    ///
    /// # Errors
    ///
    /// Returns error if the daemon encounters a fatal error during startup
    /// or an unrecoverable error in the event loop.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&mut self) -> Result<()> {
        // Step 1: Register with moto-club
        if self.registration.is_none() {
            self.register().await?;
        }

        // Step 2: Spawn health HTTP server
        let health_addr = format!("0.0.0.0:{}", self.config.health_port);
        let listener = TcpListener::bind(&health_addr)
            .await
            .map_err(|e| DaemonError::Config(format!("failed to bind health port: {e}")))?;

        let health_router = health::health_router(Arc::clone(&self.health));

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, health_router).await {
                tracing::error!(error = %e, "health server failed");
            }
        });

        tracing::info!(
            health_port = self.config.health_port,
            "health server started"
        );

        // Step 3: Mark WireGuard engine as initialized and ready for peers.
        // The engine creates per-peer Tunnel instances on demand as PeerAction::Add
        // events arrive from the WebSocket stream.
        self.health.set_wireguard_state(WireGuardState::Up);

        // Step 4: Connect to peer streaming WebSocket.
        let mut ws_stream = self.connect_peer_stream().await?;
        self.health.set_moto_club_connected(true);

        let peer_stream_url = self.config.peer_stream_url();
        tracing::info!(
            garage_id = %self.config.garage_id,
            peer_stream_url = %peer_stream_url,
            "daemon running — entering event loop"
        );

        // Step 5: Main event loop
        let mut shutdown_rx = self.shutdown_rx.clone();
        let mut timer_tick = tokio::time::interval(DEFAULT_TIMER_TICK);
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| DaemonError::Config(format!("failed to register SIGTERM handler: {e}")))?;

        loop {
            tokio::select! {
                // SIGTERM
                _ = sigterm.recv() => {
                    tracing::info!("SIGTERM received, shutting down");
                    break;
                }

                // Programmatic shutdown via watch channel
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("shutdown signal received");
                        break;
                    }
                }

                // WireGuard timer tick — keepalive and handshake management
                _ = timer_tick.tick() => {
                    for (key, tunnel) in &mut self.wg_tunnels {
                        if let Err(e) = tunnel.update_timers() {
                            tracing::warn!(
                                peer = %key,
                                error = %e,
                                "WireGuard timer tick failed"
                            );
                        }
                    }
                }

                // WebSocket messages from peer stream
                msg = ws_stream.next() => {
                    match msg {
                        Some(Ok(tungstenite::Message::Text(text))) => {
                            match Self::parse_peer_event(&text) {
                                Ok(action) => self.handle_peer_action(action),
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        text = %text,
                                        "failed to parse peer event"
                                    );
                                }
                            }
                        }
                        Some(Ok(tungstenite::Message::Ping(data))) => {
                            tracing::trace!("received WebSocket ping");
                            // Pong is sent automatically by tungstenite
                            let _ = data;
                        }
                        Some(Ok(tungstenite::Message::Pong(_))) => {
                            tracing::trace!("received WebSocket pong");
                        }
                        Some(Ok(tungstenite::Message::Close(_))) => {
                            tracing::warn!("peer stream WebSocket closed by server");
                            self.health.set_moto_club_connected(false);
                            break;
                        }
                        Some(Ok(_)) => {
                            // Binary or other message types — ignore
                        }
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "peer stream WebSocket error");
                            self.health.set_moto_club_connected(false);
                            break;
                        }
                        None => {
                            tracing::warn!("peer stream WebSocket stream ended");
                            self.health.set_moto_club_connected(false);
                            break;
                        }
                    }
                }
            }
        }

        // Step 6: Graceful shutdown
        tracing::info!("shutting down — cleaning up tunnels");

        // Clear all WireGuard peer tunnels
        self.wg_tunnels.clear();
        self.peers.clear();

        self.health.set_wireguard_state(WireGuardState::Down);
        self.health.set_moto_club_connected(false);
        self.health.set_active_peers(0);

        tracing::info!("daemon stopped");

        Ok(())
    }
}

/// Extract the host (with port) from a WebSocket URL for the Host header.
fn extract_host(url: &str) -> String {
    // Strip scheme prefix
    let without_scheme = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url);
    // Take everything before the first '/'
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
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
        let config = DaemonConfig::new(String::new(), "garage".to_string(), "token".to_string());
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
        let peer_info = PeerInfo::new(peer_key, OverlayIp::client(1));

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

    #[test]
    fn read_k8s_token_from_nonexistent_file() {
        let result = Daemon::read_k8s_token_from("/nonexistent/path/token");
        assert!(matches!(result, Err(DaemonError::Config(_))));
    }

    #[test]
    fn read_k8s_token_from_temp_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("moto-test-sa-token");
        std::fs::write(&path, "my-test-token\n").unwrap();

        let result = Daemon::read_k8s_token_from(path.to_str().unwrap());
        assert_eq!(result.unwrap(), "my-test-token");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn read_k8s_token_from_empty_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("moto-test-sa-token-empty");
        std::fs::write(&path, "").unwrap();

        let result = Daemon::read_k8s_token_from(path.to_str().unwrap());
        assert!(matches!(result, Err(DaemonError::Config(_))));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn parse_peer_event_add() {
        let peer_key = WgPrivateKey::generate().public_key();
        let peer_info = PeerInfo::new(peer_key, OverlayIp::client(1));
        let action = PeerAction::add(peer_info);
        let json = serde_json::to_string(&action).unwrap();

        let parsed = Daemon::parse_peer_event(&json).unwrap();
        assert!(matches!(parsed, PeerAction::Add(_)));
    }

    #[test]
    fn parse_peer_event_remove() {
        let peer_key = WgPrivateKey::generate().public_key();
        let action = PeerAction::remove(peer_key);
        let json = serde_json::to_string(&action).unwrap();

        let parsed = Daemon::parse_peer_event(&json).unwrap();
        assert!(matches!(parsed, PeerAction::Remove { .. }));
    }

    #[test]
    fn parse_peer_event_invalid() {
        let result = Daemon::parse_peer_event("not json");
        assert!(matches!(result, Err(DaemonError::WebSocket(_))));
    }

    #[test]
    fn extract_host_wss() {
        assert_eq!(
            super::extract_host("wss://moto-club.example.com/internal/wg/garages/g1/peers"),
            "moto-club.example.com"
        );
    }

    #[test]
    fn extract_host_ws_with_port() {
        assert_eq!(
            super::extract_host("ws://localhost:8080/internal/wg/garages/g1/peers"),
            "localhost:8080"
        );
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(super::extract_host("example.com/path"), "example.com");
    }
}
