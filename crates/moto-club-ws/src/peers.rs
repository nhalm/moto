//! WebSocket handler for peer streaming.
//!
//! Garages maintain a persistent WebSocket connection to receive real-time
//! peer updates when sessions are created or closed.
//!
//! # Endpoint
//!
//! `WS /internal/wg/garages/{id}/peers`
//!
//! Authorization: Bearer <k8s-service-account-token>
//!
//! # Protocol
//!
//! After connecting, the server sends the current peer list as `PeerEvent::Add` messages.
//! Then it streams `PeerEvent::Add` and `PeerEvent::Remove` as sessions change.
//!
//! # Example Message
//!
//! ```json
//! {"type": "add", "public_key": "base64...", "allowed_ip": "fd00:moto:2::1/128"}
//! ```

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use moto_club_wg::{PeerBroadcaster, PeerEvent, RegisteredDevice, Session};
use moto_wgtunnel_types::WgPublicKey;

/// Trait for providing peer streaming context.
///
/// This trait abstracts the application state needed by the peer WebSocket handler.
/// Implement this trait for your application state type to enable peer streaming.
pub trait PeerStreamingContext: Clone + Send + Sync + 'static {
    /// List active (non-expired) sessions for a garage.
    ///
    /// # Errors
    ///
    /// Returns an error if the session lookup fails.
    fn list_sessions_for_garage(&self, garage_id: &str) -> Result<Vec<Session>, String>;

    /// Get device info by public key.
    ///
    /// # Errors
    ///
    /// Returns an error if the device lookup fails.
    fn get_device(&self, pubkey: &WgPublicKey) -> Result<Option<RegisteredDevice>, String>;

    /// Get the peer broadcaster for subscribing to peer events.
    fn peer_broadcaster(&self) -> Arc<PeerBroadcaster>;
}

/// Handle a WebSocket connection for peer streaming.
///
/// This function is the main entry point for handling peer WebSocket connections.
/// It sends the current peer list on connect, then streams peer events as they occur.
///
/// # Type Parameters
///
/// * `C` - Context type that provides session listing, device lookup, and broadcaster access
///
/// # Arguments
///
/// * `socket` - The WebSocket connection
/// * `garage_id` - The garage ID to stream peers for
/// * `context` - Application context providing peer streaming dependencies
pub async fn handle_peers_socket<C: PeerStreamingContext>(
    socket: WebSocket,
    garage_id: String,
    context: C,
) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to peer events for this garage
    let peer_broadcaster = context.peer_broadcaster();
    let mut peer_rx = peer_broadcaster.subscribe(&garage_id);

    tracing::info!(garage_id = %garage_id, "Peer WebSocket connected");

    // Send current peers (sessions) to the garage on connect
    if let Ok(sessions) = context.list_sessions_for_garage(&garage_id) {
        for session in sessions {
            if let Ok(Some(device)) = context.get_device(&session.device_pubkey) {
                let event = PeerEvent::add(device.public_key, device.overlay_ip);
                if let Ok(json) = event.to_json() {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        tracing::debug!(garage_id = %garage_id, "Failed to send initial peer");
                        return;
                    }
                }
            }
        }
    }

    loop {
        tokio::select! {
            // Forward peer events to the WebSocket
            result = peer_rx.recv() => {
                match result {
                    Ok(event) => {
                        match event.to_json() {
                            Ok(json) => {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    tracing::debug!(garage_id = %garage_id, "WebSocket send failed, closing");
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!(garage_id = %garage_id, error = %e, "Failed to serialize peer event");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(garage_id = %garage_id, lagged = n, "Peer events lagged, some events dropped");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::debug!(garage_id = %garage_id, "Peer broadcast channel closed");
                        break;
                    }
                }
            }
            // Handle incoming WebSocket messages (pings, close, etc.)
            result = receiver.next() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!(garage_id = %garage_id, "Peer WebSocket closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!(garage_id = %garage_id, error = %e, "WebSocket error");
                        break;
                    }
                    _ => {
                        // Ignore text/binary messages from garage
                    }
                }
            }
        }
    }

    // Cleanup when WebSocket closes
    peer_broadcaster.remove_garage(&garage_id);
    tracing::info!(garage_id = %garage_id, "Peer WebSocket disconnected");
}

#[cfg(test)]
mod tests {
    use moto_wgtunnel_types::WgPrivateKey;

    #[test]
    fn test_pubkey_generation() {
        let pubkey = WgPrivateKey::generate().public_key();
        // Just verify it can be created
        assert!(!pubkey.to_base64().is_empty());
    }
}
