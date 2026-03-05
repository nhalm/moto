//! `MagicConn` - UDP + DERP connection multiplexer.
//!
//! `MagicConn` handles direct UDP vs DERP relay connections transparently.
//! It attempts direct connections first, falling back to DERP when NAT blocks
//! direct communication.
//!
//! # Connection Strategy
//!
//! 1. Try direct UDP connection (3 second timeout)
//! 2. If direct fails, use DERP relay
//! 3. No upgrade attempts once on DERP (simplicity for v1)
//!
//! # Example
//!
//! ```ignore
//! use moto_wgtunnel_conn::magic::{MagicConn, MagicConnConfig};
//! use moto_wgtunnel_types::{WgPrivateKey, DerpMap};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let private_key = WgPrivateKey::generate();
//! let derp_map = DerpMap::new();
//!
//! let config = MagicConnConfig::new(private_key, derp_map);
//! let conn = MagicConn::new(config).await?;
//!
//! // Send a packet to a peer
//! let peer_key = WgPrivateKey::generate().public_key();
//! conn.send(&peer_key, b"encrypted wireguard packet").await?;
//!
//! // Receive packets from any peer
//! let (src, data) = conn.recv().await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

use crate::endpoint::{EndpointConfig, EndpointSelector};
use crate::path::{PathState, PathType};
use moto_wgtunnel_derp::{
    ClientError as DerpClientError, DerpClient, DerpClientConfig, DerpClientHandle, DerpEvent,
};
use moto_wgtunnel_types::{DerpMap, WgPrivateKey, WgPublicKey};

/// Default direct connection timeout.
pub const DEFAULT_DIRECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Default DERP connection timeout per region.
pub const DEFAULT_DERP_REGION_TIMEOUT: Duration = Duration::from_secs(10);

/// Default UDP receive buffer size.
const UDP_RECV_BUF_SIZE: usize = 65535;

/// Errors that can occur during `MagicConn` operations.
#[derive(Debug, Error)]
#[allow(clippy::result_large_err)] // DerpClientError is large but acceptable
pub enum MagicConnError {
    /// I/O error during socket operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Direct connection timed out.
    #[error("direct connection timed out after {0:?}")]
    DirectTimeout(Duration),

    /// DERP connection failed.
    #[error("DERP connection failed: {0}")]
    DerpFailed(#[from] DerpClientError),

    /// All connection attempts failed.
    #[error("all connection attempts failed")]
    AllAttemptsFailed,

    /// Peer not found.
    #[error("peer not found: {0}")]
    PeerNotFound(WgPublicKey),

    /// Connection closed.
    #[error("connection closed")]
    ConnectionClosed,

    /// Send channel full.
    #[error("send channel full")]
    ChannelFull,

    /// No DERP regions configured.
    #[error("no DERP regions configured")]
    NoDerpRegions,
}

/// Configuration for `MagicConn`.
pub struct MagicConnConfig {
    /// Our private key bytes (stored for recreating keys).
    private_key_bytes: [u8; 32],

    /// Our public key (computed once at construction).
    public_key: WgPublicKey,

    /// DERP map for relay fallback.
    derp_map: DerpMap,

    /// Timeout for direct UDP connection attempts.
    direct_timeout: Duration,

    /// Timeout for DERP connection attempts per region.
    derp_timeout: Duration,

    /// Whether to prefer direct connections over DERP.
    prefer_direct: bool,

    /// Local address to bind UDP socket to (None = auto-bind).
    local_addr: Option<SocketAddr>,
}

impl std::fmt::Debug for MagicConnConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagicConnConfig")
            .field("public_key", &self.public_key)
            .field("derp_map", &self.derp_map)
            .field("direct_timeout", &self.direct_timeout)
            .field("derp_timeout", &self.derp_timeout)
            .field("prefer_direct", &self.prefer_direct)
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl MagicConnConfig {
    /// Create a new configuration.
    ///
    /// Takes ownership of the private key to ensure it can be securely
    /// stored and later recreated when needed for DERP connections.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // Intentionally consumes the key
    pub fn new(private_key: WgPrivateKey, derp_map: DerpMap) -> Self {
        let public_key = private_key.public_key();
        let private_key_bytes = private_key.as_bytes();
        Self {
            private_key_bytes,
            public_key,
            derp_map,
            direct_timeout: DEFAULT_DIRECT_TIMEOUT,
            derp_timeout: DEFAULT_DERP_REGION_TIMEOUT,
            prefer_direct: true,
            local_addr: None,
        }
    }

    /// Set the direct connection timeout.
    #[must_use]
    pub const fn with_direct_timeout(mut self, timeout: Duration) -> Self {
        self.direct_timeout = timeout;
        self
    }

    /// Set the DERP connection timeout per region.
    #[must_use]
    pub const fn with_derp_timeout(mut self, timeout: Duration) -> Self {
        self.derp_timeout = timeout;
        self
    }

    /// Set whether to prefer direct connections.
    #[must_use]
    pub const fn with_prefer_direct(mut self, prefer: bool) -> Self {
        self.prefer_direct = prefer;
        self
    }

    /// Set the local address to bind to.
    #[must_use]
    pub const fn with_local_addr(mut self, addr: SocketAddr) -> Self {
        self.local_addr = Some(addr);
        self
    }

    /// Get our public key.
    #[must_use]
    pub fn public_key(&self) -> WgPublicKey {
        self.public_key.clone()
    }

    /// Create a new private key from stored bytes.
    ///
    /// # Panics
    /// This should never panic as we store valid key bytes.
    fn make_private_key(&self) -> WgPrivateKey {
        WgPrivateKey::from_bytes(&self.private_key_bytes).expect("stored key bytes should be valid")
    }
}

/// Information about a connected peer.
#[derive(Debug)]
struct PeerState {
    /// Current path state.
    path_state: PathState,

    /// Known direct endpoints for this peer.
    direct_endpoints: Vec<SocketAddr>,

    /// DERP client handle if connected via DERP.
    derp_handle: Option<DerpClientHandle>,

    /// DERP region we're connected through.
    derp_region: Option<(u16, String)>,
}

impl PeerState {
    /// Create new peer state with direct endpoints.
    #[allow(clippy::missing_const_for_fn)] // Vec cannot be const-constructed
    fn new(direct_endpoints: Vec<SocketAddr>) -> Self {
        Self {
            path_state: PathState::new(),
            direct_endpoints,
            derp_handle: None,
            derp_region: None,
        }
    }

    /// Set the peer as connected via direct path.
    fn set_direct(&mut self, endpoint: SocketAddr) {
        self.path_state.set_path(PathType::direct(endpoint));
        self.derp_handle = None;
        self.derp_region = None;
    }

    /// Set the peer as connected via DERP.
    fn set_derp(&mut self, region_id: u16, region_name: String, handle: DerpClientHandle) {
        self.path_state
            .set_path(PathType::derp(region_id, &region_name));
        self.derp_handle = Some(handle);
        self.derp_region = Some((region_id, region_name));
    }
}

/// A received packet from a peer.
#[derive(Debug, Clone)]
pub struct ReceivedPacket {
    /// Source peer's public key.
    pub src: WgPublicKey,
    /// Packet data.
    pub data: Bytes,
}

/// `MagicConn` multiplexes direct UDP and DERP relay connections.
///
/// It provides a unified interface for sending and receiving packets,
/// automatically choosing the best available path.
pub struct MagicConn {
    /// Our public key.
    public_key: WgPublicKey,

    /// Configuration (includes private key bytes for DERP connections).
    config: MagicConnConfig,

    /// UDP socket for direct connections.
    udp_socket: Arc<UdpSocket>,

    /// Connected peers.
    peers: Arc<RwLock<HashMap<WgPublicKey, PeerState>>>,

    /// Active DERP client handles by region ID.
    /// We store handles (not full clients) because the `DerpClient` is moved
    /// to the receiver task. The handle allows sending packets while the
    /// receiver task handles incoming events.
    derp_handles: Arc<Mutex<HashMap<u16, DerpClientHandle>>>,

    /// Channel for received packets.
    recv_tx: mpsc::Sender<ReceivedPacket>,

    /// Channel to receive packets from.
    recv_rx: Mutex<mpsc::Receiver<ReceivedPacket>>,
}

/// Default local bind address for UDP socket.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:0";

impl MagicConn {
    /// Create a new `MagicConn`.
    ///
    /// # Errors
    /// Returns error if unable to bind UDP socket.
    #[allow(clippy::missing_panics_doc)] // Parse of constant string cannot fail
    pub async fn new(config: MagicConnConfig) -> Result<Self, MagicConnError> {
        let local_addr = config
            .local_addr
            .unwrap_or_else(|| DEFAULT_BIND_ADDR.parse().expect("valid bind address"));

        let udp_socket = UdpSocket::bind(local_addr).await?;
        let bound_addr = udp_socket.local_addr()?;
        debug!(%bound_addr, "MagicConn bound to UDP socket");

        let public_key = config.public_key.clone();

        let (recv_tx, recv_rx) = mpsc::channel(256);

        let conn = Self {
            public_key,
            config,
            udp_socket: Arc::new(udp_socket),
            peers: Arc::new(RwLock::new(HashMap::new())),
            derp_handles: Arc::new(Mutex::new(HashMap::new())),
            recv_tx,
            recv_rx: Mutex::new(recv_rx),
        };

        // Start UDP receive loop
        conn.start_udp_receiver();

        Ok(conn)
    }

    /// Get our public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.public_key
    }

    /// Get the local UDP address.
    ///
    /// # Errors
    /// Returns error if unable to get local address.
    #[allow(clippy::result_large_err)] // MagicConnError is shared across API
    pub fn local_addr(&self) -> Result<SocketAddr, MagicConnError> {
        Ok(self.udp_socket.local_addr()?)
    }

    /// Add a peer with known direct endpoints.
    ///
    /// This doesn't establish a connection; use [`connect`] for that.
    ///
    /// [`connect`]: MagicConn::connect
    pub async fn add_peer(&self, peer_key: &WgPublicKey, endpoints: Vec<SocketAddr>) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_key.clone(), PeerState::new(endpoints));
        debug!(%peer_key, endpoints = ?peers.get(peer_key).map(|p| &p.direct_endpoints), "added peer");
    }

    /// Remove a peer.
    pub async fn remove_peer(&self, peer_key: &WgPublicKey) {
        let mut peers = self.peers.write().await;
        if peers.remove(peer_key).is_some() {
            debug!(%peer_key, "removed peer");
        }
    }

    /// Connect to a peer, establishing the best available path.
    ///
    /// Tries direct UDP first (if endpoints are known), then falls back to DERP.
    ///
    /// # Errors
    /// Returns error if all connection attempts fail.
    #[allow(clippy::significant_drop_tightening)] // Lock scope is correct
    pub async fn connect(&self, peer_key: &WgPublicKey) -> Result<(), MagicConnError> {
        let endpoints = {
            let peers = self.peers.read().await;
            let peer = peers
                .get(peer_key)
                .ok_or_else(|| MagicConnError::PeerNotFound(peer_key.clone()))?;
            peer.direct_endpoints.clone()
        };

        // Build endpoint selector
        let endpoint_config = EndpointConfig::default()
            .with_direct_timeout(self.config.direct_timeout)
            .with_derp_timeout(self.config.derp_timeout)
            .with_prefer_direct(self.config.prefer_direct);

        let mut selector = EndpointSelector::new(endpoint_config);

        // Add direct endpoints
        selector.add_direct_all(endpoints);

        // Add DERP regions
        selector.add_derp_map(&self.config.derp_map);

        if !selector.has_endpoints() {
            return Err(MagicConnError::AllAttemptsFailed);
        }

        // Try endpoints in order
        while let Some(endpoint) = selector.next_endpoint() {
            debug!(%peer_key, endpoint = %endpoint, "trying endpoint");

            match &endpoint {
                crate::endpoint::Endpoint::Direct(addr) => {
                    match self.try_direct_connect(peer_key, *addr).await {
                        Ok(()) => {
                            debug!(%peer_key, %addr, "direct connection established");
                            return Ok(());
                        }
                        Err(e) => {
                            debug!(%peer_key, %addr, error = %e, "direct connection failed");
                            // If direct failed, switch to DERP mode (skip remaining direct endpoints)
                            selector.switch_to_derp();
                        }
                    }
                }
                crate::endpoint::Endpoint::Derp {
                    region_id,
                    region_name,
                } => {
                    match self
                        .try_derp_connect(peer_key, *region_id, region_name)
                        .await
                    {
                        Ok(()) => {
                            debug!(%peer_key, %region_name, "DERP connection established");
                            return Ok(());
                        }
                        Err(e) => {
                            debug!(%peer_key, %region_name, error = %e, "DERP connection failed");
                            // Continue to next DERP region
                        }
                    }
                }
            }
        }

        Err(MagicConnError::AllAttemptsFailed)
    }

    /// Try to establish a direct UDP connection to a peer.
    #[allow(clippy::significant_drop_tightening)] // Lock scope is correct
    async fn try_direct_connect(
        &self,
        peer_key: &WgPublicKey,
        addr: SocketAddr,
    ) -> Result<(), MagicConnError> {
        // For direct connections, we send a probe packet and wait for response.
        // In a real implementation, this would be a WireGuard handshake initiation.
        // For now, we just verify we can send to the address.

        let probe = b"PROBE";
        let result = timeout(
            self.config.direct_timeout,
            self.udp_socket.send_to(probe, addr),
        )
        .await;

        match result {
            Ok(Ok(_)) => {
                // Mark peer as connected via direct path
                let mut peers = self.peers.write().await;
                if let Some(peer) = peers.get_mut(peer_key) {
                    peer.set_direct(addr);
                }
                Ok(())
            }
            Ok(Err(e)) => Err(MagicConnError::Io(e)),
            Err(_) => Err(MagicConnError::DirectTimeout(self.config.direct_timeout)),
        }
    }

    /// Try to establish a DERP relay connection to a peer.
    async fn try_derp_connect(
        &self,
        peer_key: &WgPublicKey,
        region_id: u16,
        region_name: &str,
    ) -> Result<(), MagicConnError> {
        // Get or create DERP client for this region
        let handle = self.get_or_create_derp_client(region_id).await?;

        // Mark peer as connected via DERP
        {
            let mut peers = self.peers.write().await;
            if let Some(peer) = peers.get_mut(peer_key) {
                peer.set_derp(region_id, region_name.to_string(), handle);
            }
        }

        Ok(())
    }

    /// Get or create a DERP client for a region.
    ///
    /// This method manages DERP client connections efficiently:
    /// - Returns an existing handle if we already have an active connection to the region
    /// - Creates a new `DerpClient`, starts its receiver task, and caches the handle
    ///
    /// The `DerpClient` is moved to the receiver task which handles incoming events.
    /// We cache the `DerpClientHandle` for sending packets to that region.
    async fn get_or_create_derp_client(
        &self,
        region_id: u16,
    ) -> Result<DerpClientHandle, MagicConnError> {
        // First, check if we already have an active handle for this region (read-only check)
        {
            let handles = self.derp_handles.lock().await;
            if let Some(handle) = handles.get(&region_id) {
                debug!(region_id, "reusing existing DERP client handle");
                return Ok(handle.clone());
            }
        }

        // No existing handle - need to create a new DERP client connection
        debug!(region_id, "creating new DERP client connection");

        // Get the region from the DERP map
        let region = self
            .config
            .derp_map
            .get_region(region_id)
            .ok_or(MagicConnError::NoDerpRegions)?;

        // Get the first node in the region
        let node = region.nodes.first().ok_or(MagicConnError::NoDerpRegions)?;

        // Create DERP client config with a fresh private key instance
        let private_key = self.config.make_private_key();
        let derp_config =
            DerpClientConfig::new(private_key, node).with_connect_timeout(self.config.derp_timeout);

        // Connect with timeout - this performs the DERP handshake:
        // 1. WebSocket connection to wss://host:port/derp
        // 2. Receive ServerKey frame
        // 3. Send ClientInfo frame with our public key
        // 4. Receive ServerInfo frame
        let client = timeout(self.config.derp_timeout, DerpClient::connect(derp_config))
            .await
            .map_err(|_| MagicConnError::DerpFailed(DerpClientError::Timeout))??;

        let handle = client.handle();

        // Start DERP receive loop - this spawns a task that:
        // - Receives packets relayed through the DERP server
        // - Handles PeerPresent/PeerGone notifications
        // - Processes health and restart messages
        // The DerpClient is moved to this task; we keep only the handle for sending
        self.start_derp_receiver(region_id, client);

        // Cache the handle for future use with this region
        {
            let mut handles = self.derp_handles.lock().await;
            handles.insert(region_id, handle.clone());
        }

        info!(region_id, "DERP client connected and ready for relay");
        Ok(handle)
    }

    /// Start the UDP receive loop.
    fn start_udp_receiver(&self) {
        let socket = Arc::clone(&self.udp_socket);
        let recv_tx = self.recv_tx.clone();
        let peers = Arc::clone(&self.peers);

        tokio::spawn(async move {
            let mut buf = vec![0u8; UDP_RECV_BUF_SIZE];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, src_addr)) => {
                        trace!(%src_addr, len, "received UDP packet");

                        // Try to find the peer by their endpoint
                        let src_key = {
                            let peers_guard = peers.read().await;
                            peers_guard
                                .iter()
                                .find(|(_, state)| {
                                    state
                                        .path_state
                                        .path()
                                        .and_then(PathType::direct_endpoint)
                                        .is_some_and(|ep| ep == src_addr)
                                })
                                .map(|(key, _)| key.clone())
                        };

                        if let Some(src) = src_key {
                            let packet = ReceivedPacket {
                                src,
                                data: Bytes::copy_from_slice(&buf[..len]),
                            };

                            if recv_tx.send(packet).await.is_err() {
                                debug!("receiver dropped, stopping UDP receive loop");
                                break;
                            }
                        } else {
                            trace!(%src_addr, "received packet from unknown peer");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "UDP receive error");
                    }
                }
            }
        });
    }

    /// Start a DERP receive loop for a client.
    fn start_derp_receiver(&self, region_id: u16, mut client: DerpClient) {
        let recv_tx = self.recv_tx.clone();
        let peers = Arc::clone(&self.peers);

        tokio::spawn(async move {
            debug!(region_id, "starting DERP receive loop");

            while let Some(event) = client.recv().await {
                match event {
                    DerpEvent::Packet(packet) => {
                        trace!(src = %packet.src, len = packet.data.len(), "received DERP packet");

                        let recv_packet = ReceivedPacket {
                            src: packet.src,
                            data: packet.data,
                        };

                        if recv_tx.send(recv_packet).await.is_err() {
                            debug!("receiver dropped, stopping DERP receive loop");
                            break;
                        }
                    }
                    DerpEvent::PeerPresent(key) => {
                        debug!(%key, region_id, "peer present on DERP");
                    }
                    DerpEvent::PeerGone(key) => {
                        debug!(%key, region_id, "peer gone from DERP");
                        // Clear DERP connection for this peer
                        let mut peers_guard = peers.write().await;
                        if let Some(peer) = peers_guard.get_mut(&key)
                            && peer
                                .derp_region
                                .as_ref()
                                .is_some_and(|(id, _)| *id == region_id)
                        {
                            peer.path_state.clear_path();
                            peer.derp_handle = None;
                            peer.derp_region = None;
                        }
                    }
                    DerpEvent::Health(msg) => {
                        if msg.is_empty() {
                            trace!(region_id, "DERP healthy");
                        } else {
                            warn!(region_id, message = %msg, "DERP health issue");
                        }
                    }
                    DerpEvent::Restarting {
                        reconnect_in_ms,
                        try_for_ms,
                    } => {
                        warn!(
                            region_id,
                            reconnect_in_ms, try_for_ms, "DERP server restarting"
                        );
                    }
                }
            }

            debug!(region_id, "DERP receive loop ended");
        });
    }

    /// Send a packet to a peer, using the best available path.
    ///
    /// # Errors
    /// Returns error if the peer is not connected or send fails.
    #[allow(clippy::significant_drop_tightening)] // Need lock held during send
    pub async fn send(&self, peer_key: &WgPublicKey, data: &[u8]) -> Result<(), MagicConnError> {
        let peers = self.peers.read().await;
        let peer = peers
            .get(peer_key)
            .ok_or_else(|| MagicConnError::PeerNotFound(peer_key.clone()))?;

        if !peer.path_state.is_connected() {
            return Err(MagicConnError::PeerNotFound(peer_key.clone()));
        }

        match peer.path_state.path() {
            Some(PathType::Direct { endpoint }) => {
                self.udp_socket.send_to(data, endpoint).await?;
                trace!(%peer_key, %endpoint, len = data.len(), "sent via direct");
                Ok(())
            }
            Some(PathType::Derp { .. }) => {
                if let Some(ref handle) = peer.derp_handle {
                    handle.send(peer_key, Bytes::copy_from_slice(data)).await?;
                    trace!(%peer_key, len = data.len(), "sent via DERP");
                    Ok(())
                } else {
                    Err(MagicConnError::ConnectionClosed)
                }
            }
            None => Err(MagicConnError::PeerNotFound(peer_key.clone())),
        }
    }

    /// Receive a packet from any peer.
    ///
    /// # Errors
    /// Returns error if the connection is closed.
    pub async fn recv(&self) -> Result<ReceivedPacket, MagicConnError> {
        let mut rx = self.recv_rx.lock().await;
        rx.recv().await.ok_or(MagicConnError::ConnectionClosed)
    }

    /// Get the current path type for a peer.
    ///
    /// Returns `None` if the peer is not connected.
    pub async fn current_path(&self, peer_key: &WgPublicKey) -> Option<PathType> {
        let peers = self.peers.read().await;
        peers
            .get(peer_key)
            .and_then(|p| p.path_state.path().cloned())
    }

    /// Get the current path state for a peer.
    ///
    /// Returns `None` if the peer is not known.
    pub async fn path_state(&self, peer_key: &WgPublicKey) -> Option<PathState> {
        let peers = self.peers.read().await;
        peers.get(peer_key).map(|p| p.path_state.clone())
    }

    /// Check if a peer is connected.
    pub async fn is_connected(&self, peer_key: &WgPublicKey) -> bool {
        let peers = self.peers.read().await;
        peers
            .get(peer_key)
            .is_some_and(|p| p.path_state.is_connected())
    }

    /// Get a list of connected peers.
    pub async fn connected_peers(&self) -> Vec<WgPublicKey> {
        let peers = self.peers.read().await;
        peers
            .iter()
            .filter(|(_, state)| state.path_state.is_connected())
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Update quality metrics after sending a packet.
    pub async fn record_sent(&self, peer_key: &WgPublicKey) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_key) {
            peer.path_state.quality_mut().record_sent();
        }
    }

    /// Update quality metrics after receiving a packet.
    pub async fn record_received(&self, peer_key: &WgPublicKey) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_key) {
            peer.path_state.quality_mut().record_received();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

    fn test_key() -> WgPrivateKey {
        WgPrivateKey::generate()
    }

    fn test_derp_map() -> DerpMap {
        DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.example.com")),
        )
    }

    #[tokio::test]
    async fn config_builder() {
        let private_key = test_key();
        let expected_public_key = private_key.public_key();
        let derp_map = test_derp_map();

        let config = MagicConnConfig::new(private_key, derp_map)
            .with_direct_timeout(Duration::from_secs(5))
            .with_derp_timeout(Duration::from_secs(15))
            .with_prefer_direct(false);

        assert_eq!(config.direct_timeout, Duration::from_secs(5));
        assert_eq!(config.derp_timeout, Duration::from_secs(15));
        assert!(!config.prefer_direct);
        assert_eq!(config.public_key(), expected_public_key);
    }

    #[tokio::test]
    async fn create_magic_conn() {
        let private_key = test_key();
        let expected_public_key = private_key.public_key();
        let derp_map = test_derp_map();

        let config = MagicConnConfig::new(private_key, derp_map);
        let conn = MagicConn::new(config).await.unwrap();

        assert_eq!(conn.public_key(), &expected_public_key);
        assert!(conn.local_addr().is_ok());
    }

    #[tokio::test]
    async fn add_and_remove_peer() {
        let private_key = test_key();
        let derp_map = test_derp_map();
        let peer_key = test_key().public_key();

        let config = MagicConnConfig::new(private_key, derp_map);
        let conn = MagicConn::new(config).await.unwrap();

        // Add peer
        let endpoints = vec!["192.0.2.1:51820".parse().unwrap()];
        conn.add_peer(&peer_key, endpoints).await;

        // Peer should not be connected yet (just added)
        assert!(!conn.is_connected(&peer_key).await);

        // Remove peer
        conn.remove_peer(&peer_key).await;

        // Current path should be None
        assert!(conn.current_path(&peer_key).await.is_none());
    }

    #[tokio::test]
    async fn connected_peers_empty() {
        let private_key = test_key();
        let derp_map = test_derp_map();

        let config = MagicConnConfig::new(private_key, derp_map);
        let conn = MagicConn::new(config).await.unwrap();

        let connected = conn.connected_peers().await;
        assert!(connected.is_empty());
    }

    #[tokio::test]
    async fn send_to_unknown_peer_fails() {
        let private_key = test_key();
        let derp_map = test_derp_map();
        let peer_key = test_key().public_key();

        let config = MagicConnConfig::new(private_key, derp_map);
        let conn = MagicConn::new(config).await.unwrap();

        let result = conn.send(&peer_key, b"hello").await;
        assert!(matches!(result, Err(MagicConnError::PeerNotFound(_))));
    }

    #[tokio::test]
    async fn connect_to_unknown_peer_fails() {
        let private_key = test_key();
        let derp_map = test_derp_map();
        let peer_key = test_key().public_key();

        let config = MagicConnConfig::new(private_key, derp_map);
        let conn = MagicConn::new(config).await.unwrap();

        let result = conn.connect(&peer_key).await;
        assert!(matches!(result, Err(MagicConnError::PeerNotFound(_))));
    }

    #[tokio::test]
    async fn config_with_local_addr() {
        let private_key = test_key();
        let derp_map = test_derp_map();

        let config = MagicConnConfig::new(private_key, derp_map)
            .with_local_addr("127.0.0.1:0".parse().unwrap());

        let conn = MagicConn::new(config).await.unwrap();
        let local_addr = conn.local_addr().unwrap();

        assert!(local_addr.ip().is_loopback());
    }
}
