//! Peer information for `WireGuard` configuration.
//!
//! This module provides the [`PeerInfo`] type which represents a `WireGuard` peer
//! with its public key, allowed IP, and optional direct endpoint.
//!
//! # Usage
//!
//! Peer information is exchanged during tunnel session creation and via the
//! peer streaming WebSocket for dynamic peer updates.
//!
//! ```
//! use moto_wgtunnel_types::peer::PeerInfo;
//! use moto_wgtunnel_types::keys::WgPublicKey;
//! use moto_wgtunnel_types::ip::OverlayIp;
//! use std::net::SocketAddr;
//!
//! // Create peer info for a garage
//! let peer = PeerInfo::new(
//!     WgPublicKey::from_base64("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=").unwrap(),
//!     OverlayIp::garage(1),
//! );
//!
//! // With a direct endpoint
//! let peer_with_endpoint = peer.with_endpoint("203.0.113.5:51820".parse().unwrap());
//! assert!(peer_with_endpoint.endpoint().is_some());
//! ```

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::ip::OverlayIp;
use crate::keys::WgPublicKey;

/// Information about a `WireGuard` peer.
///
/// This is the core type exchanged between moto-club, clients, and garages
/// for `WireGuard` peer configuration.
///
/// # Fields
/// - `public_key`: The peer's `WireGuard` public key (X25519)
/// - `allowed_ip`: The peer's overlay IP (used in `WireGuard` `AllowedIPs`)
/// - `endpoint`: Optional direct UDP endpoint for P2P connections
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    /// The peer's `WireGuard` public key.
    public_key: WgPublicKey,

    /// The peer's overlay IP address.
    ///
    /// This is used in `WireGuard`'s `AllowedIPs` configuration to route
    /// traffic destined for this IP through the tunnel to this peer.
    allowed_ip: OverlayIp,

    /// Optional direct UDP endpoint for P2P connections.
    ///
    /// If provided, the client will attempt direct connection to this
    /// endpoint before falling back to DERP relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<SocketAddr>,
}

impl PeerInfo {
    /// Create new peer information.
    ///
    /// # Arguments
    /// - `public_key`: The peer's `WireGuard` public key
    /// - `allowed_ip`: The peer's overlay IP address
    #[must_use]
    pub const fn new(public_key: WgPublicKey, allowed_ip: OverlayIp) -> Self {
        Self {
            public_key,
            allowed_ip,
            endpoint: None,
        }
    }

    /// Create peer information with a direct endpoint.
    ///
    /// # Arguments
    /// - `public_key`: The peer's `WireGuard` public key
    /// - `allowed_ip`: The peer's overlay IP address
    /// - `endpoint`: Direct UDP endpoint for P2P connections
    #[must_use]
    pub const fn with_endpoint_addr(
        public_key: WgPublicKey,
        allowed_ip: OverlayIp,
        endpoint: SocketAddr,
    ) -> Self {
        Self {
            public_key,
            allowed_ip,
            endpoint: Some(endpoint),
        }
    }

    /// Add or update the direct endpoint.
    #[must_use]
    pub const fn with_endpoint(mut self, endpoint: SocketAddr) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    /// Get the peer's `WireGuard` public key.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        &self.public_key
    }

    /// Get the peer's overlay IP address.
    #[must_use]
    pub const fn allowed_ip(&self) -> OverlayIp {
        self.allowed_ip
    }

    /// Get the peer's direct endpoint, if available.
    #[must_use]
    pub const fn endpoint(&self) -> Option<SocketAddr> {
        self.endpoint
    }

    /// Check if this peer has a direct endpoint.
    #[must_use]
    pub const fn has_endpoint(&self) -> bool {
        self.endpoint.is_some()
    }
}

/// Actions for peer streaming updates via WebSocket.
///
/// When a garage receives peer updates from moto-club, they come as
/// `PeerAction` messages indicating whether to add or remove a peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum PeerAction {
    /// Add a new peer to the `WireGuard` configuration.
    Add(PeerInfo),

    /// Remove a peer from the `WireGuard` configuration.
    ///
    /// Only the public key is needed to identify which peer to remove.
    Remove {
        /// The public key of the peer to remove.
        public_key: WgPublicKey,
    },
}

impl PeerAction {
    /// Create an add action for a peer.
    #[must_use]
    pub const fn add(peer: PeerInfo) -> Self {
        Self::Add(peer)
    }

    /// Create a remove action for a peer.
    #[must_use]
    pub const fn remove(public_key: WgPublicKey) -> Self {
        Self::Remove { public_key }
    }

    /// Get the public key associated with this action.
    #[must_use]
    pub const fn public_key(&self) -> &WgPublicKey {
        match self {
            Self::Add(peer) => &peer.public_key,
            Self::Remove { public_key } => public_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_public_key() -> WgPublicKey {
        // Valid 32-byte key encoded as base64
        WgPublicKey::from_base64("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=").unwrap()
    }

    fn another_public_key() -> WgPublicKey {
        WgPublicKey::from_base64("MTIzNDU2Nzg5MGFiY2RlZmdoaWprbG1ub3BxcnN0dXY=").unwrap()
    }

    #[test]
    fn peer_info_new() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(42);

        let peer = PeerInfo::new(public_key.clone(), allowed_ip);

        assert_eq!(peer.public_key(), &public_key);
        assert_eq!(peer.allowed_ip(), allowed_ip);
        assert!(peer.endpoint().is_none());
        assert!(!peer.has_endpoint());
    }

    #[test]
    fn peer_info_with_endpoint() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::garage(1);
        let endpoint: SocketAddr = "203.0.113.5:51820".parse().unwrap();

        let peer = PeerInfo::with_endpoint_addr(public_key.clone(), allowed_ip, endpoint);

        assert_eq!(peer.public_key(), &public_key);
        assert_eq!(peer.allowed_ip(), allowed_ip);
        assert_eq!(peer.endpoint(), Some(endpoint));
        assert!(peer.has_endpoint());
    }

    #[test]
    fn peer_info_builder() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(1);
        let endpoint: SocketAddr = "[::1]:51820".parse().unwrap();

        let peer = PeerInfo::new(public_key, allowed_ip).with_endpoint(endpoint);

        assert_eq!(peer.endpoint(), Some(endpoint));
    }

    #[test]
    fn peer_info_serde() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::garage(123);
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();

        let peer = PeerInfo::with_endpoint_addr(public_key, allowed_ip, endpoint);
        let json = serde_json::to_string(&peer).unwrap();
        let peer2: PeerInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(peer, peer2);
    }

    #[test]
    fn peer_info_serde_without_endpoint() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(1);

        let peer = PeerInfo::new(public_key, allowed_ip);
        let json = serde_json::to_string(&peer).unwrap();

        // endpoint should be omitted from JSON
        assert!(!json.contains("endpoint"));

        let peer2: PeerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(peer, peer2);
    }

    #[test]
    fn peer_action_add() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(1);
        let peer = PeerInfo::new(public_key.clone(), allowed_ip);

        let action = PeerAction::add(peer);

        assert_eq!(action.public_key(), &public_key);
        assert!(matches!(action, PeerAction::Add(_)));
    }

    #[test]
    fn peer_action_remove() {
        let public_key = test_public_key();

        let action = PeerAction::remove(public_key.clone());

        assert_eq!(action.public_key(), &public_key);
        assert!(matches!(action, PeerAction::Remove { .. }));
    }

    #[test]
    fn peer_action_serde_add() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(42);
        let peer = PeerInfo::new(public_key, allowed_ip);

        let action = PeerAction::add(peer);
        let json = serde_json::to_string(&action).unwrap();

        // Should contain action field
        assert!(json.contains("\"action\":\"add\""));

        let action2: PeerAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, action2);
    }

    #[test]
    fn peer_action_serde_remove() {
        let public_key = test_public_key();

        let action = PeerAction::remove(public_key);
        let json = serde_json::to_string(&action).unwrap();

        // Should contain action field
        assert!(json.contains("\"action\":\"remove\""));

        let action2: PeerAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, action2);
    }

    #[test]
    fn peer_info_equality() {
        let public_key = test_public_key();
        let allowed_ip = OverlayIp::client(1);

        let peer1 = PeerInfo::new(public_key.clone(), allowed_ip);
        let peer2 = PeerInfo::new(public_key, allowed_ip);
        let peer3 = PeerInfo::new(another_public_key(), allowed_ip);

        assert_eq!(peer1, peer2);
        assert_ne!(peer1, peer3);
    }
}
