//! `WireGuard` tunnel configuration.
//!
//! This module provides configuration types for `WireGuard` tunnels:
//!
//! - [`TunnelConfig`]: Top-level tunnel configuration
//! - [`InterfaceConfig`]: Local interface settings (private key, address)
//! - [`PeerConfig`]: Remote peer settings (public key, allowed IPs, endpoint)
//! - [`TimingConfig`]: Connection timing parameters (keepalive, timeouts)
//!
//! # Configuration Sources
//!
//! Configuration can come from multiple sources, in order of precedence:
//! 1. Programmatic construction
//! 2. Environment variables (for overrides)
//! 3. Config file (`~/.config/moto/config.toml`)
//!
//! # Environment Variables
//!
//! - `MOTO_WGTUNNEL_DERP_ONLY`: Force DERP relay (skip direct UDP attempts)
//! - `MOTO_WGTUNNEL_LOG`: Set logging level (debug, info, warn, error)
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_engine::config::{TunnelConfig, InterfaceConfig, PeerConfig, TimingConfig};
//! use moto_wgtunnel_types::{WgPrivateKey, WgPublicKey, OverlayIp};
//!
//! // Generate keys
//! let private_key = WgPrivateKey::generate();
//! let peer_public_key = WgPrivateKey::generate().public_key();
//!
//! // Create interface config
//! let interface = InterfaceConfig::new(private_key, OverlayIp::client(1));
//!
//! // Create peer config
//! let peer = PeerConfig::new(peer_public_key, OverlayIp::garage(1));
//!
//! // Build tunnel config with default timing
//! let config = TunnelConfig::builder()
//!     .interface(interface)
//!     .peer(peer)
//!     .build();
//! ```

use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPrivateKey, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

/// Default `WireGuard` keepalive interval (25 seconds).
///
/// This matches the spec's default and is a common value for NAT traversal.
pub const DEFAULT_KEEPALIVE_SECS: u64 = 25;

/// Default direct connection timeout (3 seconds).
///
/// After this timeout, the connection falls back to DERP relay.
pub const DEFAULT_DIRECT_TIMEOUT_SECS: u64 = 3;

/// Default DERP connection timeout (10 seconds per region).
pub const DEFAULT_DERP_TIMEOUT_SECS: u64 = 10;

/// Default MTU for the `WireGuard` tunnel.
///
/// 1420 is a safe default that accounts for `WireGuard` overhead.
pub const DEFAULT_MTU: u16 = 1420;

/// Environment variable to force DERP-only mode.
pub const ENV_DERP_ONLY: &str = "MOTO_WGTUNNEL_DERP_ONLY";

/// Environment variable to set log level.
pub const ENV_LOG_LEVEL: &str = "MOTO_WGTUNNEL_LOG";

/// Error type for configuration operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Missing required field.
    #[error("missing required field: {0}")]
    MissingField(&'static str),

    /// Invalid configuration value.
    #[error("invalid configuration: {message}")]
    Invalid {
        /// Description of what's invalid.
        message: String,
    },

    /// Environment variable parsing failed.
    #[error("invalid environment variable {name}: {message}")]
    EnvVar {
        /// Name of the environment variable.
        name: String,
        /// Description of the error.
        message: String,
    },
}

/// Timing parameters for `WireGuard` connections.
///
/// These control keepalive intervals and connection timeouts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingConfig {
    /// `WireGuard` persistent keepalive interval.
    ///
    /// Packets are sent at this interval to keep NAT mappings alive.
    /// Set to 0 to disable keepalives.
    #[serde(with = "serde_duration_secs")]
    keepalive: Duration,

    /// Timeout for direct UDP connection attempts.
    ///
    /// If no response is received within this time, fall back to DERP.
    #[serde(with = "serde_duration_secs")]
    direct_timeout: Duration,

    /// Timeout for DERP connection attempts (per region).
    ///
    /// If a DERP region doesn't respond within this time, try the next region.
    #[serde(with = "serde_duration_secs")]
    derp_timeout: Duration,
}

impl TimingConfig {
    /// Create timing config with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create timing config with custom keepalive interval.
    #[must_use]
    pub fn with_keepalive(keepalive: Duration) -> Self {
        Self {
            keepalive,
            ..Self::default()
        }
    }

    /// Get the keepalive interval.
    #[must_use]
    pub const fn keepalive(&self) -> Duration {
        self.keepalive
    }

    /// Get the keepalive interval in seconds (for `WireGuard` configuration).
    ///
    /// Returns 0 if keepalive is disabled.
    #[must_use]
    pub fn keepalive_secs(&self) -> u16 {
        u16::try_from(self.keepalive.as_secs()).unwrap_or(u16::MAX)
    }

    /// Set the keepalive interval.
    pub fn set_keepalive(&mut self, keepalive: Duration) {
        self.keepalive = keepalive;
    }

    /// Get the direct connection timeout.
    #[must_use]
    pub const fn direct_timeout(&self) -> Duration {
        self.direct_timeout
    }

    /// Set the direct connection timeout.
    pub fn set_direct_timeout(&mut self, timeout: Duration) {
        self.direct_timeout = timeout;
    }

    /// Get the DERP connection timeout.
    #[must_use]
    pub const fn derp_timeout(&self) -> Duration {
        self.derp_timeout
    }

    /// Set the DERP connection timeout.
    pub fn set_derp_timeout(&mut self, timeout: Duration) {
        self.derp_timeout = timeout;
    }
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            keepalive: Duration::from_secs(DEFAULT_KEEPALIVE_SECS),
            direct_timeout: Duration::from_secs(DEFAULT_DIRECT_TIMEOUT_SECS),
            derp_timeout: Duration::from_secs(DEFAULT_DERP_TIMEOUT_SECS),
        }
    }
}

/// Local `WireGuard` interface configuration.
///
/// This represents the local side of the tunnel.
#[derive(Debug)]
pub struct InterfaceConfig {
    /// The local `WireGuard` private key.
    private_key: WgPrivateKey,

    /// The local overlay IP address.
    address: OverlayIp,

    /// MTU for the tunnel interface.
    mtu: u16,
}

impl InterfaceConfig {
    /// Create interface config with the given key and address.
    #[must_use]
    pub fn new(private_key: WgPrivateKey, address: OverlayIp) -> Self {
        Self {
            private_key,
            address,
            mtu: DEFAULT_MTU,
        }
    }

    /// Create interface config with custom MTU.
    #[must_use]
    pub fn with_mtu(private_key: WgPrivateKey, address: OverlayIp, mtu: u16) -> Self {
        Self {
            private_key,
            address,
            mtu,
        }
    }

    /// Get the private key.
    #[must_use]
    pub const fn private_key(&self) -> &WgPrivateKey {
        &self.private_key
    }

    /// Get the local overlay IP address.
    #[must_use]
    pub const fn address(&self) -> OverlayIp {
        self.address
    }

    /// Get the public key derived from the private key.
    #[must_use]
    pub fn public_key(&self) -> WgPublicKey {
        self.private_key.public_key()
    }

    /// Get the MTU.
    #[must_use]
    pub const fn mtu(&self) -> u16 {
        self.mtu
    }

    /// Set the MTU.
    pub fn set_mtu(&mut self, mtu: u16) {
        self.mtu = mtu;
    }
}

/// Remote `WireGuard` peer configuration.
///
/// This represents a peer we want to connect to.
#[derive(Debug, Clone)]
pub struct PeerConfig {
    /// The peer's `WireGuard` public key.
    public_key: WgPublicKey,

    /// The peer's overlay IP address (used for allowed IPs).
    allowed_ip: OverlayIp,

    /// Optional direct endpoint for the peer.
    ///
    /// If set, direct UDP connection will be attempted to this address.
    endpoint: Option<SocketAddr>,

    /// Whether this peer is persistent (kept even when inactive).
    persistent: bool,
}

impl PeerConfig {
    /// Create peer config with the given public key and allowed IP.
    #[must_use]
    pub fn new(public_key: WgPublicKey, allowed_ip: OverlayIp) -> Self {
        Self {
            public_key,
            allowed_ip,
            endpoint: None,
            persistent: false,
        }
    }

    /// Create peer config with a direct endpoint.
    #[must_use]
    pub fn with_endpoint(
        public_key: WgPublicKey,
        allowed_ip: OverlayIp,
        endpoint: SocketAddr,
    ) -> Self {
        Self {
            public_key,
            allowed_ip,
            endpoint: Some(endpoint),
            persistent: false,
        }
    }

    /// Get the peer's public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.public_key
    }

    /// Get the peer's allowed IP.
    #[must_use]
    pub const fn allowed_ip(&self) -> OverlayIp {
        self.allowed_ip
    }

    /// Get the allowed IPs as a `/128` subnet string for `WireGuard` config.
    #[must_use]
    pub fn allowed_ips_cidr(&self) -> String {
        format!("{}/128", self.allowed_ip)
    }

    /// Get the direct endpoint, if set.
    #[must_use]
    pub const fn endpoint(&self) -> Option<SocketAddr> {
        self.endpoint
    }

    /// Set the direct endpoint.
    pub fn set_endpoint(&mut self, endpoint: Option<SocketAddr>) {
        self.endpoint = endpoint;
    }

    /// Check if this peer is persistent.
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Set whether this peer is persistent.
    pub fn set_persistent(&mut self, persistent: bool) {
        self.persistent = persistent;
    }
}

/// Connection mode for the tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    /// Prefer direct connections, fall back to DERP.
    ///
    /// This is the default mode. It tries direct UDP first,
    /// then falls back to DERP relay if direct fails.
    #[default]
    PreferDirect,

    /// Force DERP relay only.
    ///
    /// Skip direct UDP attempts and connect only via DERP.
    /// Useful for testing or when direct connections are known to fail.
    DerpOnly,
}

impl ConnectionMode {
    /// Create from environment variable.
    ///
    /// Returns `DerpOnly` if `MOTO_WGTUNNEL_DERP_ONLY` is set to a truthy value.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var(ENV_DERP_ONLY) {
            Ok(v) if is_truthy(&v) => Self::DerpOnly,
            _ => Self::PreferDirect,
        }
    }

    /// Check if direct connections should be attempted.
    #[must_use]
    pub const fn try_direct(&self) -> bool {
        matches!(self, Self::PreferDirect)
    }
}

/// Full tunnel configuration.
///
/// Combines interface, peer, timing, and DERP map configuration.
#[derive(Debug)]
pub struct TunnelConfig {
    /// Local interface configuration.
    interface: InterfaceConfig,

    /// Remote peer configurations.
    peers: Vec<PeerConfig>,

    /// Timing parameters.
    timing: TimingConfig,

    /// DERP relay map.
    derp_map: DerpMap,

    /// Connection mode (prefer direct or DERP only).
    connection_mode: ConnectionMode,
}

impl TunnelConfig {
    /// Create a builder for tunnel configuration.
    #[must_use]
    pub fn builder() -> TunnelConfigBuilder {
        TunnelConfigBuilder::new()
    }

    /// Get the interface configuration.
    #[must_use]
    pub const fn interface(&self) -> &InterfaceConfig {
        &self.interface
    }

    /// Get the peer configurations.
    #[must_use]
    pub fn peers(&self) -> &[PeerConfig] {
        &self.peers
    }

    /// Get a specific peer by public key.
    #[must_use]
    pub fn peer(&self, public_key: &WgPublicKey) -> Option<&PeerConfig> {
        self.peers.iter().find(|p| p.public_key() == public_key)
    }

    /// Get the timing configuration.
    #[must_use]
    pub const fn timing(&self) -> &TimingConfig {
        &self.timing
    }

    /// Get the DERP map.
    #[must_use]
    pub const fn derp_map(&self) -> &DerpMap {
        &self.derp_map
    }

    /// Get the connection mode.
    #[must_use]
    pub const fn connection_mode(&self) -> ConnectionMode {
        self.connection_mode
    }

    /// Check if direct connections should be attempted.
    #[must_use]
    pub const fn try_direct(&self) -> bool {
        self.connection_mode.try_direct()
    }

    /// Add a peer to the configuration.
    pub fn add_peer(&mut self, peer: PeerConfig) {
        self.peers.push(peer);
    }

    /// Remove a peer by public key.
    ///
    /// Returns true if a peer was removed.
    pub fn remove_peer(&mut self, public_key: &WgPublicKey) -> bool {
        let len_before = self.peers.len();
        self.peers.retain(|p| p.public_key() != public_key);
        self.peers.len() != len_before
    }
}

/// Builder for [`TunnelConfig`].
#[derive(Debug, Default)]
pub struct TunnelConfigBuilder {
    interface: Option<InterfaceConfig>,
    peers: Vec<PeerConfig>,
    timing: Option<TimingConfig>,
    derp_map: Option<DerpMap>,
    connection_mode: Option<ConnectionMode>,
}

impl TunnelConfigBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the interface configuration.
    #[must_use]
    pub fn interface(mut self, interface: InterfaceConfig) -> Self {
        self.interface = Some(interface);
        self
    }

    /// Add a peer configuration.
    #[must_use]
    pub fn peer(mut self, peer: PeerConfig) -> Self {
        self.peers.push(peer);
        self
    }

    /// Add multiple peer configurations.
    #[must_use]
    pub fn peers(mut self, peers: impl IntoIterator<Item = PeerConfig>) -> Self {
        self.peers.extend(peers);
        self
    }

    /// Set the timing configuration.
    #[must_use]
    pub fn timing(mut self, timing: TimingConfig) -> Self {
        self.timing = Some(timing);
        self
    }

    /// Set the DERP map.
    #[must_use]
    pub fn derp_map(mut self, derp_map: DerpMap) -> Self {
        self.derp_map = Some(derp_map);
        self
    }

    /// Set the connection mode.
    #[must_use]
    pub fn connection_mode(mut self, mode: ConnectionMode) -> Self {
        self.connection_mode = Some(mode);
        self
    }

    /// Build the tunnel configuration.
    ///
    /// # Panics
    ///
    /// Panics if the interface configuration is not set.
    /// Use [`try_build`](Self::try_build) for fallible construction.
    #[must_use]
    pub fn build(self) -> TunnelConfig {
        self.try_build()
            .expect("interface configuration is required")
    }

    /// Try to build the tunnel configuration.
    ///
    /// # Errors
    ///
    /// Returns error if required fields are missing.
    pub fn try_build(self) -> Result<TunnelConfig, ConfigError> {
        let interface = self
            .interface
            .ok_or(ConfigError::MissingField("interface"))?;

        // Use environment variable for connection mode if not explicitly set
        let connection_mode = self
            .connection_mode
            .unwrap_or_else(ConnectionMode::from_env);

        Ok(TunnelConfig {
            interface,
            peers: self.peers,
            timing: self.timing.unwrap_or_default(),
            derp_map: self.derp_map.unwrap_or_default(),
            connection_mode,
        })
    }
}

/// Check if a string value is truthy (1, true, yes, on).
fn is_truthy(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// Serde helper for Duration as seconds.
mod serde_duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_config_defaults() {
        let timing = TimingConfig::default();

        assert_eq!(
            timing.keepalive(),
            Duration::from_secs(DEFAULT_KEEPALIVE_SECS)
        );
        assert_eq!(
            timing.direct_timeout(),
            Duration::from_secs(DEFAULT_DIRECT_TIMEOUT_SECS)
        );
        assert_eq!(
            timing.derp_timeout(),
            Duration::from_secs(DEFAULT_DERP_TIMEOUT_SECS)
        );
    }

    #[test]
    fn timing_config_keepalive_secs() {
        let mut timing = TimingConfig::default();
        assert_eq!(timing.keepalive_secs(), 25);

        timing.set_keepalive(Duration::from_secs(60));
        assert_eq!(timing.keepalive_secs(), 60);

        // Very large values are clamped to u16::MAX
        timing.set_keepalive(Duration::from_secs(100_000));
        assert_eq!(timing.keepalive_secs(), u16::MAX);
    }

    #[test]
    fn interface_config() {
        let private_key = WgPrivateKey::generate();
        let address = OverlayIp::client(42);

        let interface = InterfaceConfig::new(private_key, address);

        assert_eq!(interface.address(), address);
        assert_eq!(interface.mtu(), DEFAULT_MTU);
    }

    #[test]
    fn interface_config_with_mtu() {
        let private_key = WgPrivateKey::generate();
        let address = OverlayIp::client(42);

        let interface = InterfaceConfig::with_mtu(private_key, address, 1280);

        assert_eq!(interface.mtu(), 1280);
    }

    #[test]
    fn peer_config() {
        let peer_key = WgPrivateKey::generate().public_key();
        let allowed_ip = OverlayIp::garage(1);

        let peer = PeerConfig::new(peer_key.clone(), allowed_ip);

        assert_eq!(peer.public_key(), &peer_key);
        assert_eq!(peer.allowed_ip(), allowed_ip);
        assert!(peer.endpoint().is_none());
        assert!(!peer.is_persistent());
    }

    #[test]
    fn peer_config_with_endpoint() {
        let peer_key = WgPrivateKey::generate().public_key();
        let allowed_ip = OverlayIp::garage(1);
        let endpoint: SocketAddr = "203.0.113.5:51820".parse().unwrap();

        let peer = PeerConfig::with_endpoint(peer_key.clone(), allowed_ip, endpoint);

        assert_eq!(peer.endpoint(), Some(endpoint));
    }

    #[test]
    fn peer_config_allowed_ips_cidr() {
        let peer_key = WgPrivateKey::generate().public_key();
        let allowed_ip = OverlayIp::garage(1);

        let peer = PeerConfig::new(peer_key, allowed_ip);

        let cidr = peer.allowed_ips_cidr();
        assert!(cidr.ends_with("/128"));
    }

    #[test]
    fn connection_mode_default() {
        let mode = ConnectionMode::default();
        assert!(mode.try_direct());
    }

    #[test]
    fn connection_mode_derp_only() {
        let mode = ConnectionMode::DerpOnly;
        assert!(!mode.try_direct());
    }

    #[test]
    fn tunnel_config_builder() {
        let private_key = WgPrivateKey::generate();
        let address = OverlayIp::client(1);
        let interface = InterfaceConfig::new(private_key, address);

        let peer_key = WgPrivateKey::generate().public_key();
        let peer = PeerConfig::new(peer_key.clone(), OverlayIp::garage(1));

        let config = TunnelConfig::builder()
            .interface(interface)
            .peer(peer)
            .timing(TimingConfig::with_keepalive(Duration::from_secs(30)))
            .connection_mode(ConnectionMode::DerpOnly)
            .build();

        assert_eq!(config.peers().len(), 1);
        assert_eq!(config.timing().keepalive(), Duration::from_secs(30));
        assert_eq!(config.connection_mode(), ConnectionMode::DerpOnly);
        assert!(!config.try_direct());
    }

    #[test]
    fn tunnel_config_builder_missing_interface() {
        let result = TunnelConfig::builder().try_build();

        assert!(matches!(
            result,
            Err(ConfigError::MissingField("interface"))
        ));
    }

    #[test]
    fn tunnel_config_add_remove_peer() {
        let private_key = WgPrivateKey::generate();
        let interface = InterfaceConfig::new(private_key, OverlayIp::client(1));

        let mut config = TunnelConfig::builder().interface(interface).build();

        assert!(config.peers().is_empty());

        let peer_key1 = WgPrivateKey::generate().public_key();
        let peer1 = PeerConfig::new(peer_key1.clone(), OverlayIp::garage(1));
        config.add_peer(peer1);

        assert_eq!(config.peers().len(), 1);
        assert!(config.peer(&peer_key1).is_some());

        let peer_key2 = WgPrivateKey::generate().public_key();
        let peer2 = PeerConfig::new(peer_key2.clone(), OverlayIp::garage(2));
        config.add_peer(peer2);

        assert_eq!(config.peers().len(), 2);

        // Remove first peer
        assert!(config.remove_peer(&peer_key1));
        assert_eq!(config.peers().len(), 1);
        assert!(config.peer(&peer_key1).is_none());
        assert!(config.peer(&peer_key2).is_some());

        // Remove non-existent peer
        assert!(!config.remove_peer(&peer_key1));
        assert_eq!(config.peers().len(), 1);
    }

    #[test]
    fn is_truthy_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy("True"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy("yes"));
        assert!(is_truthy("YES"));
        assert!(is_truthy("on"));
        assert!(is_truthy("ON"));

        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy("no"));
        assert!(!is_truthy("off"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("other"));
    }

    #[test]
    fn timing_config_serde() {
        let timing = TimingConfig::default();

        let json = serde_json::to_string(&timing).unwrap();
        let timing2: TimingConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(timing, timing2);
    }
}
