//! Peer event broadcasting for `WireGuard` coordination.
//!
//! This module provides a pub/sub mechanism for garages to receive real-time
//! peer updates when sessions are created or closed.
//!
//! # Architecture
//!
//! ```text
//! Session Created → PeerBroadcaster → Garage WebSocket → WireGuard peer add
//! Session Closed  → PeerBroadcaster → Garage WebSocket → WireGuard peer remove
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::broadcaster::{PeerBroadcaster, PeerEvent, PeerAction};
//! use moto_wgtunnel_types::keys::WgPrivateKey;
//! use moto_wgtunnel_types::ip::OverlayIp;
//!
//! # tokio_test::block_on(async {
//! let broadcaster = PeerBroadcaster::new();
//!
//! // Garage subscribes to peer updates
//! let mut rx = broadcaster.subscribe("feature-foo");
//!
//! // When a session is created, broadcast the peer add event
//! let public_key = WgPrivateKey::generate().public_key();
//! let allowed_ip = OverlayIp::client(1);
//! broadcaster.broadcast_add("feature-foo", public_key.clone(), allowed_ip);
//!
//! // Garage receives the event
//! let event = rx.recv().await.unwrap();
//! assert!(matches!(event.action, PeerAction::Add));
//! # });
//! ```

use moto_wgtunnel_types::{OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::broadcast;

/// Default channel capacity for peer events.
const CHANNEL_CAPACITY: usize = 64;

/// Action to perform on a `WireGuard` peer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PeerAction {
    /// Add a peer to the `WireGuard` configuration.
    Add,
    /// Remove a peer from the `WireGuard` configuration.
    Remove,
}

/// Peer event sent to garages via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEvent {
    /// Action to perform (add or remove).
    pub action: PeerAction,

    /// Peer's `WireGuard` public key.
    pub public_key: WgPublicKey,

    /// Peer's allowed IP (their overlay IP).
    /// Only present for `add` action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_ip: Option<String>,
}

impl PeerEvent {
    /// Create a new peer add event.
    #[must_use]
    pub fn add(public_key: WgPublicKey, allowed_ip: OverlayIp) -> Self {
        Self {
            action: PeerAction::Add,
            public_key,
            allowed_ip: Some(format!("{allowed_ip}/128")),
        }
    }

    /// Create a new peer remove event.
    #[must_use]
    pub const fn remove(public_key: WgPublicKey) -> Self {
        Self {
            action: PeerAction::Remove,
            public_key,
            allowed_ip: None,
        }
    }

    /// Serialize the event to JSON.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Peer event broadcaster for real-time peer updates.
///
/// Garages subscribe to receive peer add/remove events when sessions
/// are created or closed.
pub struct PeerBroadcaster {
    inner: Mutex<BroadcasterInner>,
}

struct BroadcasterInner {
    /// Per-garage broadcast channels.
    channels: HashMap<String, broadcast::Sender<PeerEvent>>,
}

impl PeerBroadcaster {
    /// Create a new peer broadcaster.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BroadcasterInner {
                channels: HashMap::new(),
            }),
        }
    }

    /// Subscribe to peer events for a garage.
    ///
    /// Returns a receiver that will receive all peer add/remove events
    /// for the specified garage. Creates the channel if it doesn't exist.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn subscribe(&self, garage_id: &str) -> broadcast::Receiver<PeerEvent> {
        let sender = self
            .inner
            .lock()
            .unwrap()
            .channels
            .entry(garage_id.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
            .clone();
        sender.subscribe()
    }

    /// Broadcast a peer add event.
    ///
    /// Called when a new session is created and the garage should add
    /// the client as a `WireGuard` peer.
    pub fn broadcast_add(&self, garage_id: &str, public_key: WgPublicKey, allowed_ip: OverlayIp) {
        let event = PeerEvent::add(public_key, allowed_ip);
        self.broadcast(garage_id, event);
    }

    /// Broadcast a peer remove event.
    ///
    /// Called when a session is closed and the garage should remove
    /// the client from `WireGuard` peers.
    pub fn broadcast_remove(&self, garage_id: &str, public_key: WgPublicKey) {
        let event = PeerEvent::remove(public_key);
        self.broadcast(garage_id, event);
    }

    /// Broadcast an event to all subscribers of a garage.
    fn broadcast(&self, garage_id: &str, event: PeerEvent) {
        let inner = self.inner.lock().unwrap();
        if let Some(sender) = inner.channels.get(garage_id) {
            // Ignore send errors (no subscribers)
            let _ = sender.send(event);
        }
    }

    /// Remove a garage's broadcast channel.
    ///
    /// Called when a garage disconnects. Any remaining subscribers
    /// will receive an error on their next recv.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn remove_garage(&self, garage_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.channels.remove(garage_id);
    }

    /// Get the number of active subscribers for a garage.
    ///
    /// Returns 0 if the garage has no channel.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn subscriber_count(&self, garage_id: &str) -> usize {
        let inner = self.inner.lock().unwrap();
        inner
            .channels
            .get(garage_id)
            .map_or(0, tokio::sync::broadcast::Sender::receiver_count)
    }
}

impl Default for PeerBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[tokio::test]
    async fn subscribe_and_receive_add() {
        let broadcaster = PeerBroadcaster::new();
        let mut rx = broadcaster.subscribe("garage-1");

        let public_key = generate_public_key();
        let allowed_ip = OverlayIp::client(1);
        broadcaster.broadcast_add("garage-1", public_key.clone(), allowed_ip);

        let event = rx.recv().await.unwrap();
        assert_eq!(event.action, PeerAction::Add);
        assert_eq!(event.public_key, public_key);
        assert!(event.allowed_ip.is_some());
        assert!(event.allowed_ip.unwrap().ends_with("/128"));
    }

    #[tokio::test]
    async fn subscribe_and_receive_remove() {
        let broadcaster = PeerBroadcaster::new();
        let mut rx = broadcaster.subscribe("garage-1");

        let public_key = generate_public_key();
        broadcaster.broadcast_remove("garage-1", public_key.clone());

        let event = rx.recv().await.unwrap();
        assert_eq!(event.action, PeerAction::Remove);
        assert_eq!(event.public_key, public_key);
        assert!(event.allowed_ip.is_none());
    }

    #[tokio::test]
    async fn multiple_subscribers() {
        let broadcaster = PeerBroadcaster::new();
        let mut rx1 = broadcaster.subscribe("garage-1");
        let mut rx2 = broadcaster.subscribe("garage-1");

        let public_key = generate_public_key();
        let allowed_ip = OverlayIp::client(1);
        broadcaster.broadcast_add("garage-1", public_key.clone(), allowed_ip);

        // Both subscribers receive the event
        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();
        assert_eq!(event1.public_key, public_key);
        assert_eq!(event2.public_key, public_key);
    }

    #[test]
    fn subscriber_count() {
        let broadcaster = PeerBroadcaster::new();
        assert_eq!(broadcaster.subscriber_count("garage-1"), 0);

        let _rx1 = broadcaster.subscribe("garage-1");
        assert_eq!(broadcaster.subscriber_count("garage-1"), 1);

        let _rx2 = broadcaster.subscribe("garage-1");
        assert_eq!(broadcaster.subscriber_count("garage-1"), 2);
    }

    #[test]
    fn subscriber_count_after_drop() {
        let broadcaster = PeerBroadcaster::new();
        {
            let _rx = broadcaster.subscribe("garage-1");
            assert_eq!(broadcaster.subscriber_count("garage-1"), 1);
        }
        // After rx is dropped, count should be 0
        assert_eq!(broadcaster.subscriber_count("garage-1"), 0);
    }

    #[test]
    fn remove_garage() {
        let broadcaster = PeerBroadcaster::new();
        let _rx = broadcaster.subscribe("garage-1");
        assert_eq!(broadcaster.subscriber_count("garage-1"), 1);

        broadcaster.remove_garage("garage-1");
        // Channel is removed
        assert_eq!(broadcaster.subscriber_count("garage-1"), 0);
    }

    #[tokio::test]
    async fn broadcast_to_nonexistent_garage() {
        let broadcaster = PeerBroadcaster::new();

        // Broadcasting to a garage with no subscribers is a no-op
        let public_key = generate_public_key();
        broadcaster.broadcast_add("nonexistent", public_key, OverlayIp::client(1));
        // No panic, no error
    }

    #[tokio::test]
    async fn isolated_garages() {
        let broadcaster = PeerBroadcaster::new();
        let mut rx1 = broadcaster.subscribe("garage-1");
        let mut rx2 = broadcaster.subscribe("garage-2");

        let public_key = generate_public_key();
        broadcaster.broadcast_add("garage-1", public_key.clone(), OverlayIp::client(1));

        // Only garage-1 receives the event
        let event = rx1.recv().await.unwrap();
        assert_eq!(event.public_key, public_key);

        // garage-2's channel is empty
        let result = rx2.try_recv();
        assert!(result.is_err());
    }

    #[test]
    fn peer_event_add_serialization() {
        let public_key = generate_public_key();
        let event = PeerEvent::add(public_key.clone(), OverlayIp::client(1));

        let json = event.to_json().unwrap();
        assert!(json.contains(r#""action":"add""#));
        assert!(json.contains(r#""allowed_ip""#));

        // Deserialize back
        let parsed: PeerEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.action, PeerAction::Add);
        assert_eq!(parsed.public_key, public_key);
    }

    #[test]
    fn peer_event_remove_serialization() {
        let public_key = generate_public_key();
        let event = PeerEvent::remove(public_key.clone());

        let json = event.to_json().unwrap();
        assert!(json.contains(r#""action":"remove""#));
        assert!(!json.contains(r#""allowed_ip""#)); // Skipped when None

        // Deserialize back
        let parsed: PeerEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.action, PeerAction::Remove);
        assert_eq!(parsed.public_key, public_key);
    }
}
