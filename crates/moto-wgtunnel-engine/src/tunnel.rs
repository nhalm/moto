//! `WireGuard` tunnel management using boringtun.
//!
//! This module provides the [`Tunnel`] type which wraps boringtun's userspace
//! `WireGuard` implementation. It handles:
//!
//! - Packet encryption (encapsulate outgoing IP packets)
//! - Packet decryption (decapsulate incoming `WireGuard` packets)
//! - Handshake management
//! - Keepalive timers
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                              Tunnel                                      │
//! │  ┌───────────────────────────────────────────────────────────────────┐  │
//! │  │  boringtun::noise::Tunn                                           │  │
//! │  │  - WireGuard state machine                                        │  │
//! │  │  - Noise protocol handshake                                       │  │
//! │  │  - Symmetric encryption (ChaCha20-Poly1305)                       │  │
//! │  └───────────────────────────────────────────────────────────────────┘  │
//! │                                │                                         │
//! │                ┌───────────────┴───────────────┐                        │
//! │                │                               │                        │
//! │                ▼                               ▼                        │
//! │  ┌─────────────────────────┐    ┌─────────────────────────────────┐    │
//! │  │ encapsulate()           │    │ decapsulate()                   │    │
//! │  │ IP packet → WG packet   │    │ WG packet → IP packet           │    │
//! │  └─────────────────────────┘    └─────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use moto_wgtunnel_engine::tunnel::{Tunnel, TunnelEvent};
//! use moto_wgtunnel_types::{WgPrivateKey, WgPublicKey};
//!
//! // Create keys
//! let private_key = WgPrivateKey::generate();
//! let peer_public_key = WgPrivateKey::generate().public_key();
//!
//! // Create tunnel
//! let mut tunnel = Tunnel::new(private_key, peer_public_key, None)?;
//!
//! // Encapsulate an IP packet for sending
//! let ip_packet = &[0x45, 0x00, /* ... */];
//! for event in tunnel.encapsulate(ip_packet)? {
//!     match event {
//!         TunnelEvent::Network(data) => {
//!             // Send `data` via MagicConn
//!         }
//!         _ => {}
//!     }
//! }
//!
//! // Decapsulate a received WireGuard packet
//! let wg_packet = &[/* received from network */];
//! for event in tunnel.decapsulate(wg_packet)? {
//!     match event {
//!         TunnelEvent::TunnelData(data, addr) => {
//!             // Forward `data` to TUN device
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use thiserror::Error;
use tracing::{debug, trace, warn};

use moto_wgtunnel_types::{WgPrivateKey, WgPublicKey};

/// Default buffer size for encapsulate/decapsulate operations.
///
/// Must be at least MTU + `WireGuard` overhead (32 bytes header + 16 bytes auth tag).
const DEFAULT_BUFFER_SIZE: usize = 2048;

/// Minimum buffer size required by boringtun (148 bytes for handshake).
const MIN_BUFFER_SIZE: usize = 148;

/// Global tunnel index counter for unique tunnel identification.
static TUNNEL_INDEX: AtomicU32 = AtomicU32::new(1);

/// Get a unique tunnel index.
fn next_tunnel_index() -> u32 {
    TUNNEL_INDEX.fetch_add(1, Ordering::Relaxed)
}

/// Errors that can occur during tunnel operations.
#[derive(Debug, Error)]
pub enum TunnelError {
    /// `WireGuard` protocol error.
    #[error("WireGuard error: {0}")]
    WireGuard(String),

    /// Buffer too small for operation.
    #[error("buffer too small: need at least {needed} bytes, got {got}")]
    BufferTooSmall {
        /// Minimum buffer size needed.
        needed: usize,
        /// Actual buffer size.
        got: usize,
    },

    /// Invalid packet received.
    #[error("invalid packet: {0}")]
    InvalidPacket(String),

    /// Tunnel not ready (handshake not complete).
    #[error("tunnel not ready: handshake not complete")]
    NotReady,
}

impl From<boringtun::noise::errors::WireGuardError> for TunnelError {
    fn from(err: boringtun::noise::errors::WireGuardError) -> Self {
        Self::WireGuard(format!("{err:?}"))
    }
}

/// Events produced by tunnel operations.
///
/// When processing packets, the tunnel may produce multiple events that
/// need to be handled by the caller.
#[derive(Debug)]
pub enum TunnelEvent {
    /// Data to send over the network (UDP or DERP).
    ///
    /// This is an encrypted `WireGuard` packet that should be sent to the peer.
    Network(Vec<u8>),

    /// Decrypted data to write to the TUN device (IPv4).
    TunnelDataV4(Vec<u8>, Ipv4Addr),

    /// Decrypted data to write to the TUN device (IPv6).
    TunnelDataV6(Vec<u8>, Ipv6Addr),
}

impl TunnelEvent {
    /// Get the data as a byte slice.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        match self {
            Self::Network(data) | Self::TunnelDataV4(data, _) | Self::TunnelDataV6(data, _) => data,
        }
    }

    /// Check if this is a network event.
    #[must_use]
    pub const fn is_network(&self) -> bool {
        matches!(self, Self::Network(_))
    }

    /// Check if this is a tunnel data event.
    #[must_use]
    pub const fn is_tunnel_data(&self) -> bool {
        matches!(self, Self::TunnelDataV4(_, _) | Self::TunnelDataV6(_, _))
    }
}

/// Current state of the tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    /// Tunnel is initializing, no handshake started.
    Init,

    /// Handshake in progress.
    Handshaking,

    /// Tunnel is established and ready for data.
    Established,

    /// Tunnel is in an error state.
    Error,
}

impl std::fmt::Display for TunnelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init => write!(f, "init"),
            Self::Handshaking => write!(f, "handshaking"),
            Self::Established => write!(f, "established"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Statistics about tunnel activity.
#[derive(Debug, Clone, Default)]
pub struct TunnelStats {
    /// Number of packets sent (after encryption).
    pub packets_sent: u64,

    /// Number of packets received (before decryption).
    pub packets_received: u64,

    /// Bytes sent (after encryption).
    pub bytes_sent: u64,

    /// Bytes received (before decryption).
    pub bytes_received: u64,

    /// Number of handshakes completed.
    pub handshakes: u64,

    /// Time of last handshake.
    pub last_handshake: Option<Instant>,

    /// Time of last packet sent.
    pub last_sent: Option<Instant>,

    /// Time of last packet received.
    pub last_received: Option<Instant>,
}

impl TunnelStats {
    /// Create new empty stats.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            packets_sent: 0,
            packets_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            handshakes: 0,
            last_handshake: None,
            last_sent: None,
            last_received: None,
        }
    }

    /// Record a sent packet.
    pub fn record_sent(&mut self, bytes: usize) {
        self.packets_sent += 1;
        self.bytes_sent += bytes as u64;
        self.last_sent = Some(Instant::now());
    }

    /// Record a received packet.
    pub fn record_received(&mut self, bytes: usize) {
        self.packets_received += 1;
        self.bytes_received += bytes as u64;
        self.last_received = Some(Instant::now());
    }

    /// Record a completed handshake.
    pub fn record_handshake(&mut self) {
        self.handshakes += 1;
        self.last_handshake = Some(Instant::now());
    }

    /// Get time since last activity (send or receive).
    #[must_use]
    pub fn time_since_activity(&self) -> Option<Duration> {
        let last = match (self.last_sent, self.last_received) {
            (Some(s), Some(r)) => Some(s.max(r)),
            (Some(t), None) | (None, Some(t)) => Some(t),
            (None, None) => None,
        };
        last.map(|t| t.elapsed())
    }
}

/// A `WireGuard` tunnel managed by boringtun.
///
/// This is a point-to-point tunnel to a single peer. For multiple peers,
/// create multiple `Tunnel` instances.
pub struct Tunnel {
    /// The boringtun tunnel state machine.
    inner: Tunn,

    /// Our public key.
    public_key: WgPublicKey,

    /// Peer's public key.
    peer_public_key: WgPublicKey,

    /// Current tunnel state.
    state: TunnelState,

    /// Tunnel statistics.
    stats: TunnelStats,

    /// Unique tunnel index.
    index: u32,

    /// Persistent keepalive interval (None = disabled).
    keepalive: Option<Duration>,

    /// Time when the tunnel was created.
    created_at: Instant,
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tunnel")
            .field("public_key", &self.public_key)
            .field("peer_public_key", &self.peer_public_key)
            .field("state", &self.state)
            .field("index", &self.index)
            .field("keepalive", &self.keepalive)
            .finish_non_exhaustive()
    }
}

impl Tunnel {
    /// Create a new tunnel to a peer.
    ///
    /// # Arguments
    ///
    /// * `private_key` - Our `WireGuard` private key
    /// * `peer_public_key` - The peer's `WireGuard` public key
    /// * `preshared_key` - Optional preshared key for additional security
    ///
    /// # Errors
    ///
    /// Returns error if tunnel creation fails.
    pub fn new(
        private_key: WgPrivateKey,
        peer_public_key: WgPublicKey,
        preshared_key: Option<[u8; 32]>,
    ) -> Result<Self, TunnelError> {
        Self::with_keepalive(private_key, peer_public_key, preshared_key, None)
    }

    /// Create a new tunnel with persistent keepalive.
    ///
    /// # Arguments
    ///
    /// * `private_key` - Our `WireGuard` private key
    /// * `peer_public_key` - The peer's `WireGuard` public key
    /// * `preshared_key` - Optional preshared key for additional security
    /// * `keepalive` - Keepalive interval (None = disabled)
    ///
    /// # Errors
    ///
    /// Returns error if tunnel creation fails.
    #[allow(clippy::needless_pass_by_value)] // Intentionally consumes the key
    pub fn with_keepalive(
        private_key: WgPrivateKey,
        peer_public_key: WgPublicKey,
        preshared_key: Option<[u8; 32]>,
        keepalive: Option<Duration>,
    ) -> Result<Self, TunnelError> {
        let index = next_tunnel_index();
        let public_key = private_key.public_key();

        // Convert our key types to boringtun's x25519 types
        let static_private = StaticSecret::from(private_key.as_bytes());
        let peer_public = PublicKey::from(*peer_public_key.as_bytes());

        // Convert keepalive to u16 seconds for boringtun
        let keepalive_secs =
            keepalive.map(|d| u16::try_from(d.as_secs()).unwrap_or(u16::MAX));

        let inner = Tunn::new(
            static_private,
            peer_public,
            preshared_key,
            keepalive_secs,
            index,
            None, // No rate limiter for now
        );

        debug!(
            %public_key,
            %peer_public_key,
            index,
            keepalive_secs = ?keepalive_secs,
            "created tunnel"
        );

        Ok(Self {
            inner,
            public_key,
            peer_public_key,
            state: TunnelState::Init,
            stats: TunnelStats::new(),
            index,
            keepalive,
            created_at: Instant::now(),
        })
    }

    /// Get our public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.public_key
    }

    /// Get the peer's public key.
    #[must_use]
    pub const fn peer_public_key(&self) -> &WgPublicKey {
        &self.peer_public_key
    }

    /// Get the current tunnel state.
    #[must_use]
    pub const fn state(&self) -> TunnelState {
        self.state
    }

    /// Check if the tunnel is established (handshake complete).
    #[must_use]
    pub const fn is_established(&self) -> bool {
        matches!(self.state, TunnelState::Established)
    }

    /// Get tunnel statistics.
    #[must_use]
    pub const fn stats(&self) -> &TunnelStats {
        &self.stats
    }

    /// Get the tunnel index.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Get the keepalive interval.
    #[must_use]
    pub const fn keepalive(&self) -> Option<Duration> {
        self.keepalive
    }

    /// Get time since tunnel creation.
    #[must_use]
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Encapsulate an IP packet for sending over the network.
    ///
    /// Takes a plaintext IP packet and produces encrypted `WireGuard` packet(s)
    /// that should be sent to the peer.
    ///
    /// # Arguments
    ///
    /// * `src` - The IP packet to encapsulate
    ///
    /// # Returns
    ///
    /// A vector of events. Typically contains a single `TunnelEvent::Network`
    /// with the encrypted packet, but may contain additional events if a
    /// handshake needs to be initiated.
    ///
    /// # Errors
    ///
    /// Returns error if encapsulation fails.
    pub fn encapsulate(&mut self, src: &[u8]) -> Result<Vec<TunnelEvent>, TunnelError> {
        let mut dst = vec![0u8; src.len() + DEFAULT_BUFFER_SIZE];
        let mut events = Vec::new();

        let result = self.inner.encapsulate(src, &mut dst);
        self.process_tunn_result(result, &mut events)?;

        // Update stats for sent data
        for event in &events {
            if let TunnelEvent::Network(data) = event {
                self.stats.record_sent(data.len());
            }
        }

        Ok(events)
    }

    /// Decapsulate a received `WireGuard` packet.
    ///
    /// Takes an encrypted `WireGuard` packet from the network and produces
    /// decrypted IP packet(s) for the TUN device.
    ///
    /// # Arguments
    ///
    /// * `src` - The `WireGuard` packet received from the network
    ///
    /// # Returns
    ///
    /// A vector of events. May contain:
    /// - `TunnelEvent::TunnelDataV4` / `TunnelDataV6` for decrypted IP packets
    /// - `TunnelEvent::Network` for response packets (handshake, keepalive)
    ///
    /// # Errors
    ///
    /// Returns error if decapsulation fails.
    pub fn decapsulate(&mut self, src: &[u8]) -> Result<Vec<TunnelEvent>, TunnelError> {
        self.decapsulate_with_addr(None, src)
    }

    /// Decapsulate a received `WireGuard` packet with source address.
    ///
    /// Like [`decapsulate`](Self::decapsulate), but also records the source
    /// address for potential endpoint learning.
    ///
    /// # Arguments
    ///
    /// * `src_addr` - Source IP address of the packet (for endpoint learning)
    /// * `src` - The `WireGuard` packet received from the network
    ///
    /// # Returns
    ///
    /// A vector of tunnel events.
    ///
    /// # Errors
    ///
    /// Returns error if decapsulation fails.
    pub fn decapsulate_with_addr(
        &mut self,
        src_addr: Option<IpAddr>,
        src: &[u8],
    ) -> Result<Vec<TunnelEvent>, TunnelError> {
        self.stats.record_received(src.len());

        let mut dst = vec![0u8; DEFAULT_BUFFER_SIZE];
        let mut events = Vec::new();

        // First decapsulate call
        let result = self.inner.decapsulate(src_addr, src, &mut dst);
        self.process_tunn_result(result, &mut events)?;

        // Continue calling decapsulate with empty input until Done
        // (required by boringtun API)
        loop {
            dst.resize(DEFAULT_BUFFER_SIZE, 0);
            let result = self.inner.decapsulate(None, &[], &mut dst);
            match result {
                TunnResult::Done => break,
                _ => {
                    self.process_tunn_result(result, &mut events)?;
                }
            }
        }

        Ok(events)
    }

    /// Check for pending timer actions.
    ///
    /// This should be called periodically (e.g., every second) to handle:
    /// - Keepalive packets
    /// - Handshake timeouts
    /// - Session key rotation
    ///
    /// # Returns
    ///
    /// A vector of events, typically `TunnelEvent::Network` for keepalive
    /// or handshake packets.
    ///
    /// # Errors
    ///
    /// Returns error if timer processing fails.
    pub fn update_timers(&mut self) -> Result<Vec<TunnelEvent>, TunnelError> {
        let mut dst = vec![0u8; MIN_BUFFER_SIZE];
        let mut events = Vec::new();

        let result = self.inner.update_timers(&mut dst);
        self.process_tunn_result(result, &mut events)?;

        Ok(events)
    }

    /// Force initiation of a new handshake.
    ///
    /// This is useful when:
    /// - First connecting to a peer
    /// - Recovering from network changes
    /// - Proactively refreshing session keys
    ///
    /// # Returns
    ///
    /// A vector containing the handshake initiation packet to send.
    ///
    /// # Errors
    ///
    /// Returns error if handshake initiation fails.
    pub fn force_handshake(&mut self) -> Result<Vec<TunnelEvent>, TunnelError> {
        let mut dst = vec![0u8; MIN_BUFFER_SIZE];
        let mut events = Vec::new();

        self.state = TunnelState::Handshaking;

        let result = self.inner.format_handshake_initiation(&mut dst, false);
        self.process_tunn_result(result, &mut events)?;

        debug!(index = self.index, "initiated handshake");

        Ok(events)
    }

    /// Process a `TunnResult` and convert to events.
    fn process_tunn_result(
        &mut self,
        result: TunnResult<'_>,
        events: &mut Vec<TunnelEvent>,
    ) -> Result<(), TunnelError> {
        match result {
            TunnResult::Done => {
                trace!(index = self.index, "operation complete");
            }

            TunnResult::Err(err) => {
                warn!(index = self.index, error = ?err, "WireGuard error");
                self.state = TunnelState::Error;
                return Err(err.into());
            }

            TunnResult::WriteToNetwork(data) => {
                trace!(index = self.index, len = data.len(), "write to network");
                events.push(TunnelEvent::Network(data.to_vec()));

                // If we're sending a handshake response, we might be establishing
                if self.state == TunnelState::Init || self.state == TunnelState::Handshaking {
                    self.state = TunnelState::Handshaking;
                }
            }

            TunnResult::WriteToTunnelV4(data, addr) => {
                trace!(index = self.index, len = data.len(), %addr, "write to tunnel (IPv4)");
                events.push(TunnelEvent::TunnelDataV4(data.to_vec(), addr));

                // Receiving tunnel data means handshake is complete
                if self.state != TunnelState::Established {
                    self.state = TunnelState::Established;
                    self.stats.record_handshake();
                    debug!(index = self.index, "tunnel established");
                }
            }

            TunnResult::WriteToTunnelV6(data, addr) => {
                trace!(index = self.index, len = data.len(), %addr, "write to tunnel (IPv6)");
                events.push(TunnelEvent::TunnelDataV6(data.to_vec(), addr));

                // Receiving tunnel data means handshake is complete
                if self.state != TunnelState::Established {
                    self.state = TunnelState::Established;
                    self.stats.record_handshake();
                    debug!(index = self.index, "tunnel established");
                }
            }
        }

        Ok(())
    }

    /// Get time until next timer event.
    ///
    /// Returns the duration until `update_timers` should be called next.
    /// If `None`, there are no pending timers.
    #[must_use]
    pub fn time_to_next_timer(&self) -> Option<Duration> {
        // boringtun doesn't expose this directly, so we use a conservative default
        // based on the keepalive interval or a 10-second maximum
        Some(
            self.keepalive
                .map_or(Duration::from_secs(10), |ka| ka.min(Duration::from_secs(10))),
        )
    }
}

/// Builder for creating tunnels with additional configuration.
#[derive(Debug)]
pub struct TunnelBuilder {
    private_key: WgPrivateKey,
    peer_public_key: WgPublicKey,
    preshared_key: Option<[u8; 32]>,
    keepalive: Option<Duration>,
}

impl TunnelBuilder {
    /// Create a new tunnel builder.
    #[must_use]
    pub fn new(private_key: WgPrivateKey, peer_public_key: WgPublicKey) -> Self {
        Self {
            private_key,
            peer_public_key,
            preshared_key: None,
            keepalive: None,
        }
    }

    /// Set the preshared key.
    #[must_use]
    pub fn preshared_key(mut self, key: [u8; 32]) -> Self {
        self.preshared_key = Some(key);
        self
    }

    /// Set the persistent keepalive interval.
    #[must_use]
    pub fn keepalive(mut self, interval: Duration) -> Self {
        self.keepalive = Some(interval);
        self
    }

    /// Build the tunnel.
    ///
    /// # Errors
    ///
    /// Returns error if tunnel creation fails.
    pub fn build(self) -> Result<Tunnel, TunnelError> {
        Tunnel::with_keepalive(
            self.private_key,
            self.peer_public_key,
            self.preshared_key,
            self.keepalive,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keys() -> (WgPrivateKey, WgPublicKey) {
        let private = WgPrivateKey::generate();
        let public = private.public_key();
        (private, public)
    }

    #[test]
    fn create_tunnel() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel = Tunnel::new(our_private, peer_public, None).unwrap();

        assert_eq!(tunnel.state(), TunnelState::Init);
        assert!(!tunnel.is_established());
        assert!(tunnel.index() > 0);
    }

    #[test]
    fn create_tunnel_with_keepalive() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel = Tunnel::with_keepalive(
            our_private,
            peer_public,
            None,
            Some(Duration::from_secs(25)),
        )
        .unwrap();

        assert_eq!(tunnel.keepalive(), Some(Duration::from_secs(25)));
    }

    #[test]
    fn tunnel_builder() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel = TunnelBuilder::new(our_private, peer_public)
            .keepalive(Duration::from_secs(30))
            .build()
            .unwrap();

        assert_eq!(tunnel.keepalive(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn force_handshake() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let mut tunnel = Tunnel::new(our_private, peer_public, None).unwrap();

        let events = tunnel.force_handshake().unwrap();

        // Should produce a handshake initiation packet
        assert!(!events.is_empty());
        assert!(events[0].is_network());
        assert_eq!(tunnel.state(), TunnelState::Handshaking);
    }

    #[test]
    fn tunnel_stats() {
        let mut stats = TunnelStats::new();

        assert_eq!(stats.packets_sent, 0);
        assert_eq!(stats.packets_received, 0);

        stats.record_sent(100);
        stats.record_received(200);

        assert_eq!(stats.packets_sent, 1);
        assert_eq!(stats.packets_received, 1);
        assert_eq!(stats.bytes_sent, 100);
        assert_eq!(stats.bytes_received, 200);
        assert!(stats.last_sent.is_some());
        assert!(stats.last_received.is_some());
    }

    #[test]
    fn tunnel_state_display() {
        assert_eq!(TunnelState::Init.to_string(), "init");
        assert_eq!(TunnelState::Handshaking.to_string(), "handshaking");
        assert_eq!(TunnelState::Established.to_string(), "established");
        assert_eq!(TunnelState::Error.to_string(), "error");
    }

    #[test]
    fn tunnel_event_methods() {
        let network = TunnelEvent::Network(vec![1, 2, 3]);
        assert!(network.is_network());
        assert!(!network.is_tunnel_data());
        assert_eq!(network.data(), &[1, 2, 3]);

        let tunnel_v4 = TunnelEvent::TunnelDataV4(vec![4, 5, 6], Ipv4Addr::LOCALHOST);
        assert!(!tunnel_v4.is_network());
        assert!(tunnel_v4.is_tunnel_data());

        let tunnel_v6 = TunnelEvent::TunnelDataV6(vec![7, 8, 9], Ipv6Addr::LOCALHOST);
        assert!(!tunnel_v6.is_network());
        assert!(tunnel_v6.is_tunnel_data());
    }

    #[test]
    fn unique_tunnel_indices() {
        let (our_private1, _) = test_keys();
        let (our_private2, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel1 = Tunnel::new(our_private1, peer_public.clone(), None).unwrap();
        let tunnel2 = Tunnel::new(our_private2, peer_public, None).unwrap();

        // Indices should be unique
        assert_ne!(tunnel1.index(), tunnel2.index());
    }

    #[test]
    fn time_to_next_timer() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel = Tunnel::with_keepalive(
            our_private,
            peer_public,
            None,
            Some(Duration::from_secs(25)),
        )
        .unwrap();

        // Should return some duration for timer
        let next = tunnel.time_to_next_timer();
        assert!(next.is_some());
        assert!(next.unwrap() <= Duration::from_secs(25));
    }

    #[test]
    fn uptime() {
        let (our_private, _) = test_keys();
        let (_, peer_public) = test_keys();

        let tunnel = Tunnel::new(our_private, peer_public, None).unwrap();

        // Uptime should be very small right after creation
        let uptime = tunnel.uptime();
        assert!(uptime < Duration::from_secs(1));
    }

    #[test]
    fn stats_time_since_activity() {
        let mut stats = TunnelStats::new();

        // No activity yet
        assert!(stats.time_since_activity().is_none());

        // After sending
        stats.record_sent(100);
        assert!(stats.time_since_activity().is_some());
        assert!(stats.time_since_activity().unwrap() < Duration::from_secs(1));
    }
}
