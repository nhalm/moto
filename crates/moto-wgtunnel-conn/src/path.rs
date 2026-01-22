//! Path status tracking for `WireGuard` connections.
//!
//! This module provides types for tracking the current connection path to a peer.
//! A path can be either direct (UDP) or via a DERP relay.
//!
//! # Path Types
//!
//! - **Direct**: UDP connection directly to the peer's public IP:port
//! - **Derp**: Relay connection through a DERP server
//!
//! # Path Selection Strategy
//!
//! The connection strategy (implemented in `MagicConn`) is:
//! 1. Try direct UDP connection (3 second timeout)
//! 2. If direct fails, use DERP relay
//! 3. No upgrade attempts once on DERP (simplicity for v1)
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_conn::path::{PathType, PathState, PathQuality};
//! use std::net::SocketAddr;
//!
//! // Create a direct path
//! let direct_path = PathType::direct("203.0.113.5:51820".parse().unwrap());
//! assert!(direct_path.is_direct());
//!
//! // Create a DERP relay path
//! let derp_path = PathType::derp(1, "us-east");
//! assert!(derp_path.is_derp());
//!
//! // Track path state for a peer
//! let mut state = PathState::new();
//! state.set_path(direct_path);
//! assert!(state.is_connected());
//! ```

use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// The type of path used to reach a `WireGuard` peer.
///
/// This represents the current active connection path, not a candidate endpoint.
/// Use [`Endpoint`](crate::endpoint::Endpoint) for endpoint selection during
/// connection establishment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathType {
    /// Direct UDP connection to peer's endpoint.
    Direct {
        /// The peer's public IP:port.
        endpoint: SocketAddr,
    },

    /// Connection via DERP relay.
    Derp {
        /// The DERP region ID.
        region_id: u16,
        /// The DERP region name (for display/logging).
        region_name: String,
    },
}

impl PathType {
    /// Create a direct UDP path.
    #[must_use]
    pub const fn direct(endpoint: SocketAddr) -> Self {
        Self::Direct { endpoint }
    }

    /// Create a DERP relay path.
    #[must_use]
    pub fn derp(region_id: u16, region_name: impl Into<String>) -> Self {
        Self::Derp {
            region_id,
            region_name: region_name.into(),
        }
    }

    /// Check if this is a direct UDP path.
    #[must_use]
    pub const fn is_direct(&self) -> bool {
        matches!(self, Self::Direct { .. })
    }

    /// Check if this is a DERP relay path.
    #[must_use]
    pub const fn is_derp(&self) -> bool {
        matches!(self, Self::Derp { .. })
    }

    /// Get the direct endpoint, if this is a direct path.
    #[must_use]
    pub const fn direct_endpoint(&self) -> Option<SocketAddr> {
        match self {
            Self::Direct { endpoint } => Some(*endpoint),
            Self::Derp { .. } => None,
        }
    }

    /// Get the DERP region ID, if this is a DERP path.
    #[must_use]
    pub const fn derp_region_id(&self) -> Option<u16> {
        match self {
            Self::Direct { .. } => None,
            Self::Derp { region_id, .. } => Some(*region_id),
        }
    }

    /// Get the DERP region name, if this is a DERP path.
    #[must_use]
    pub fn derp_region_name(&self) -> Option<&str> {
        match self {
            Self::Direct { .. } => None,
            Self::Derp { region_name, .. } => Some(region_name),
        }
    }

    /// Get a short description of the path type for logging.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Direct { .. } => "direct",
            Self::Derp { .. } => "derp",
        }
    }
}

impl std::fmt::Display for PathType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct { endpoint } => write!(f, "direct:{endpoint}"),
            Self::Derp { region_name, .. } => write!(f, "derp:{region_name}"),
        }
    }
}

/// Quality metrics for a connection path.
///
/// These metrics help determine if a path is healthy and can be used
/// for status display to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathQuality {
    /// Round-trip time estimate.
    rtt: Option<Duration>,

    /// Time of last successful packet send/receive.
    last_activity: Option<Instant>,

    /// Number of packets sent on this path.
    packets_sent: u64,

    /// Number of packets received on this path.
    packets_received: u64,
}

impl PathQuality {
    /// Create new path quality metrics with no data.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rtt: None,
            last_activity: None,
            packets_sent: 0,
            packets_received: 0,
        }
    }

    /// Get the estimated round-trip time.
    #[must_use]
    pub const fn rtt(&self) -> Option<Duration> {
        self.rtt
    }

    /// Set the round-trip time estimate.
    pub const fn set_rtt(&mut self, rtt: Duration) {
        self.rtt = Some(rtt);
    }

    /// Get the time of last activity.
    #[must_use]
    pub const fn last_activity(&self) -> Option<Instant> {
        self.last_activity
    }

    /// Record activity (packet sent or received).
    pub fn record_activity(&mut self) {
        self.last_activity = Some(Instant::now());
    }

    /// Get the number of packets sent.
    #[must_use]
    pub const fn packets_sent(&self) -> u64 {
        self.packets_sent
    }

    /// Record a packet sent.
    pub fn record_sent(&mut self) {
        self.packets_sent += 1;
        self.record_activity();
    }

    /// Get the number of packets received.
    #[must_use]
    pub const fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Record a packet received.
    pub fn record_received(&mut self) {
        self.packets_received += 1;
        self.record_activity();
    }

    /// Check if the path has been idle for longer than the given duration.
    #[must_use]
    pub fn is_idle(&self, threshold: Duration) -> bool {
        self.last_activity
            .is_none_or(|last| last.elapsed() > threshold)
    }
}

impl Default for PathQuality {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks the current path state for a single peer.
///
/// This maintains the active path (if connected) along with quality metrics.
/// It does not handle path selection or failover - that's `MagicConn`'s job.
#[derive(Debug, Clone)]
pub struct PathState {
    /// Current active path, if connected.
    path: Option<PathType>,

    /// Quality metrics for the current path.
    quality: PathQuality,

    /// Time when connection was established.
    connected_at: Option<Instant>,
}

impl PathState {
    /// Create a new disconnected path state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            path: None,
            quality: PathQuality::new(),
            connected_at: None,
        }
    }

    /// Check if connected to the peer.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        self.path.is_some()
    }

    /// Get the current path type, if connected.
    #[must_use]
    pub const fn path(&self) -> Option<&PathType> {
        self.path.as_ref()
    }

    /// Get quality metrics for the current path.
    #[must_use]
    pub const fn quality(&self) -> &PathQuality {
        &self.quality
    }

    /// Get mutable quality metrics for the current path.
    pub const fn quality_mut(&mut self) -> &mut PathQuality {
        &mut self.quality
    }

    /// Get the time when the connection was established.
    #[must_use]
    pub const fn connected_at(&self) -> Option<Instant> {
        self.connected_at
    }

    /// Get how long the connection has been established.
    #[must_use]
    pub fn connection_duration(&self) -> Option<Duration> {
        self.connected_at.map(|t| t.elapsed())
    }

    /// Set the active path (connect to peer).
    ///
    /// This resets quality metrics since we're on a new path.
    pub fn set_path(&mut self, path: PathType) {
        self.path = Some(path);
        self.quality = PathQuality::new();
        self.connected_at = Some(Instant::now());
    }

    /// Clear the active path (disconnect from peer).
    pub fn clear_path(&mut self) {
        self.path = None;
        self.quality = PathQuality::new();
        self.connected_at = None;
    }

    /// Check if currently using a direct path.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        self.path.as_ref().is_some_and(PathType::is_direct)
    }

    /// Check if currently using a DERP path.
    #[must_use]
    pub fn is_derp(&self) -> bool {
        self.path.as_ref().is_some_and(PathType::is_derp)
    }
}

impl Default for PathState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn path_type_direct() {
        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        let path = PathType::direct(addr);

        assert!(path.is_direct());
        assert!(!path.is_derp());
        assert_eq!(path.direct_endpoint(), Some(addr));
        assert_eq!(path.derp_region_id(), None);
        assert_eq!(path.derp_region_name(), None);
        assert_eq!(path.kind(), "direct");
        assert_eq!(path.to_string(), "direct:192.0.2.1:51820");
    }

    #[test]
    fn path_type_derp() {
        let path = PathType::derp(1, "us-east");

        assert!(!path.is_direct());
        assert!(path.is_derp());
        assert_eq!(path.direct_endpoint(), None);
        assert_eq!(path.derp_region_id(), Some(1));
        assert_eq!(path.derp_region_name(), Some("us-east"));
        assert_eq!(path.kind(), "derp");
        assert_eq!(path.to_string(), "derp:us-east");
    }

    #[test]
    fn path_type_equality() {
        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();

        let direct1 = PathType::direct(addr);
        let direct2 = PathType::direct(addr);
        let direct3 = PathType::direct("192.0.2.2:51820".parse().unwrap());

        assert_eq!(direct1, direct2);
        assert_ne!(direct1, direct3);

        let derp1 = PathType::derp(1, "us-east");
        let derp2 = PathType::derp(1, "us-east");
        let derp3 = PathType::derp(2, "us-west");

        assert_eq!(derp1, derp2);
        assert_ne!(derp1, derp3);

        assert_ne!(direct1, derp1);
    }

    #[test]
    fn path_quality_new() {
        let quality = PathQuality::new();

        assert!(quality.rtt().is_none());
        assert!(quality.last_activity().is_none());
        assert_eq!(quality.packets_sent(), 0);
        assert_eq!(quality.packets_received(), 0);
    }

    #[test]
    fn path_quality_record_activity() {
        let mut quality = PathQuality::new();

        quality.record_sent();
        assert_eq!(quality.packets_sent(), 1);
        assert!(quality.last_activity().is_some());

        quality.record_received();
        assert_eq!(quality.packets_received(), 1);

        quality.record_sent();
        quality.record_sent();
        assert_eq!(quality.packets_sent(), 3);
    }

    #[test]
    fn path_quality_rtt() {
        let mut quality = PathQuality::new();

        assert!(quality.rtt().is_none());

        quality.set_rtt(Duration::from_millis(50));
        assert_eq!(quality.rtt(), Some(Duration::from_millis(50)));
    }

    #[test]
    fn path_quality_idle() {
        let mut quality = PathQuality::new();

        // No activity ever = idle
        assert!(quality.is_idle(Duration::from_millis(1)));

        quality.record_activity();

        // Just recorded activity, not idle
        assert!(!quality.is_idle(Duration::from_secs(1)));

        // With a very short threshold, should be idle after brief sleep
        thread::sleep(Duration::from_millis(10));
        assert!(quality.is_idle(Duration::from_millis(1)));
    }

    #[test]
    fn path_state_new() {
        let state = PathState::new();

        assert!(!state.is_connected());
        assert!(state.path().is_none());
        assert!(state.connected_at().is_none());
        assert!(!state.is_direct());
        assert!(!state.is_derp());
    }

    #[test]
    fn path_state_set_direct() {
        let mut state = PathState::new();
        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();

        state.set_path(PathType::direct(addr));

        assert!(state.is_connected());
        assert!(state.is_direct());
        assert!(!state.is_derp());
        assert!(state.path().unwrap().is_direct());
        assert!(state.connected_at().is_some());
    }

    #[test]
    fn path_state_set_derp() {
        let mut state = PathState::new();

        state.set_path(PathType::derp(1, "us-east"));

        assert!(state.is_connected());
        assert!(!state.is_direct());
        assert!(state.is_derp());
        assert!(state.path().unwrap().is_derp());
    }

    #[test]
    fn path_state_clear() {
        let mut state = PathState::new();
        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();

        state.set_path(PathType::direct(addr));
        assert!(state.is_connected());

        state.clear_path();

        assert!(!state.is_connected());
        assert!(state.path().is_none());
        assert!(state.connected_at().is_none());
    }

    #[test]
    fn path_state_quality_reset_on_new_path() {
        let mut state = PathState::new();
        let addr1: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        let addr2: SocketAddr = "192.0.2.2:51820".parse().unwrap();

        state.set_path(PathType::direct(addr1));
        state.quality_mut().record_sent();
        state.quality_mut().record_sent();
        assert_eq!(state.quality().packets_sent(), 2);

        // Setting a new path resets quality
        state.set_path(PathType::direct(addr2));
        assert_eq!(state.quality().packets_sent(), 0);
    }

    #[test]
    fn path_state_connection_duration() {
        let mut state = PathState::new();

        assert!(state.connection_duration().is_none());

        state.set_path(PathType::derp(1, "test"));
        thread::sleep(Duration::from_millis(10));

        let duration = state.connection_duration().unwrap();
        assert!(duration >= Duration::from_millis(10));
    }
}
