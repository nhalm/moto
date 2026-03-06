//! WebSocket handler for event streaming.
//!
//! Streams real-time garage events (TTL warnings, status changes, errors) to CLI clients.
//!
//! # Endpoint
//!
//! `WS /ws/v1/events?garages=bold-mongoose,quiet-falcon`
//!
//! # Protocol
//!
//! Server sends JSON messages with `type` discriminator:
//! - `ttl_warning`: TTL approaching expiry
//! - `status_change`: Garage state transition
//! - `error`: Garage error (pod failures, crash loops)

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tokio::sync::broadcast;

/// Channel capacity for event broadcasting.
const CHANNEL_CAPACITY: usize = 64;

/// Query parameters for event streaming.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventStreamQuery {
    /// Comma-separated garage names to watch (empty = all owned).
    #[serde(default)]
    pub garages: Option<String>,
}

/// Event types sent over the event WebSocket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum GarageEvent {
    /// TTL approaching expiry.
    #[serde(rename = "ttl_warning")]
    TtlWarning {
        /// Garage name.
        garage: String,
        /// Minutes remaining before expiry.
        minutes_remaining: u32,
        /// Expiry timestamp (ISO 8601).
        expires_at: String,
    },
    /// Garage state transition.
    #[serde(rename = "status_change")]
    StatusChange {
        /// Garage name.
        garage: String,
        /// Previous state.
        from: String,
        /// New state.
        to: String,
        /// Reason for transition (only on Terminated/Failed).
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Garage error.
    #[serde(rename = "error")]
    Error {
        /// Garage name.
        garage: String,
        /// Error description.
        message: String,
    },
}

impl GarageEvent {
    /// Get the garage name this event relates to.
    #[must_use]
    pub fn garage_name(&self) -> &str {
        match self {
            Self::TtlWarning { garage, .. }
            | Self::StatusChange { garage, .. }
            | Self::Error { garage, .. } => garage,
        }
    }
}

/// Event broadcaster for real-time garage event notifications.
///
/// Events are broadcast per-owner. Each owner has a single broadcast channel
/// that receives all events for their garages. The WebSocket handler filters
/// by the requested garage names.
pub struct EventBroadcaster {
    inner: Mutex<BroadcasterInner>,
}

struct BroadcasterInner {
    /// Per-owner broadcast channels.
    channels: HashMap<String, broadcast::Sender<GarageEvent>>,
}

impl EventBroadcaster {
    /// Create a new event broadcaster.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BroadcasterInner {
                channels: HashMap::new(),
            }),
        }
    }

    /// Subscribe to events for an owner.
    ///
    /// Returns a receiver that will receive all garage events for the owner.
    /// Creates the channel if it doesn't exist.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn subscribe(&self, owner: &str) -> broadcast::Receiver<GarageEvent> {
        let sender = self
            .inner
            .lock()
            .unwrap()
            .channels
            .entry(owner.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
            .clone();
        sender.subscribe()
    }

    /// Broadcast an event to all subscribers of an owner.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn broadcast(&self, owner: &str, event: GarageEvent) {
        let inner = self.inner.lock().unwrap();
        if let Some(sender) = inner.channels.get(owner) {
            let _ = sender.send(event);
        }
    }

    /// Get the number of active subscribers for an owner.
    ///
    /// Returns 0 if the owner has no channel.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn subscriber_count(&self, owner: &str) -> usize {
        let inner = self.inner.lock().unwrap();
        inner
            .channels
            .get(owner)
            .map_or(0, tokio::sync::broadcast::Sender::receiver_count)
    }

    /// Remove an owner's broadcast channel.
    ///
    /// Called when the last subscriber disconnects.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn remove_owner(&self, owner: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.channels.remove(owner);
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for providing event streaming context.
///
/// Abstracts the application state needed by the event WebSocket handler.
pub trait EventStreamingContext: Clone + Send + Sync + 'static {
    /// List garage names owned by the given owner.
    fn list_owned_garage_names(
        &self,
        owner: &str,
    ) -> impl std::future::Future<Output = Result<Vec<String>, String>> + Send;

    /// Get the event broadcaster.
    fn event_broadcaster(&self) -> std::sync::Arc<EventBroadcaster>;
}

/// Handle a WebSocket connection for event streaming.
///
/// Subscribes to the owner's event broadcast channel and forwards events
/// that match the requested garage filter.
pub async fn handle_event_socket<C: EventStreamingContext>(
    socket: WebSocket,
    owner: String,
    query: EventStreamQuery,
    context: C,
) {
    let (mut sender, mut receiver) = socket.split();

    // Determine garage filter
    let garage_filter: Option<HashSet<String>> = match query.garages {
        Some(ref names) if !names.is_empty() => {
            Some(names.split(',').map(|s| s.trim().to_string()).collect())
        }
        _ => None,
    };

    // If a filter is provided, validate that the owner actually owns those garages
    if let Some(ref filter) = garage_filter {
        match context.list_owned_garage_names(&owner).await {
            Ok(owned_names) => {
                let owned_set: HashSet<&str> = owned_names.iter().map(String::as_str).collect();
                for name in filter {
                    if !owned_set.contains(name.as_str()) {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "garage": name,
                            "message": format!("garage '{name}' not found or not owned")
                        });
                        if let Ok(json) = serde_json::to_string(&error_msg) {
                            let _ = sender.send(Message::Text(json.into())).await;
                        }
                        let _ = sender.close().await;
                        return;
                    }
                }
            }
            Err(e) => {
                let error_msg = serde_json::json!({
                    "type": "error",
                    "garage": "",
                    "message": format!("failed to validate garages: {e}")
                });
                if let Ok(json) = serde_json::to_string(&error_msg) {
                    let _ = sender.send(Message::Text(json.into())).await;
                }
                let _ = sender.close().await;
                return;
            }
        }
    }

    let broadcaster = context.event_broadcaster();
    let mut event_rx = broadcaster.subscribe(&owner);

    tracing::info!(
        owner = %owner,
        filter = ?garage_filter,
        "event WebSocket connected"
    );

    loop {
        tokio::select! {
            event_result = event_rx.recv() => {
                match event_result {
                    Ok(event) => {
                        // Apply garage filter
                        if let Some(ref filter) = garage_filter
                            && !filter.contains(event.garage_name())
                        {
                            continue;
                        }

                        if let Ok(json) = serde_json::to_string(&event)
                            && sender.send(Message::Text(json.into())).await.is_err()
                        {
                            tracing::debug!(owner = %owner, "event WebSocket send failed, closing");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(owner = %owner, skipped = n, "event subscriber lagged");
                        // Continue receiving — events are fire-and-forget
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!(owner = %owner, "event broadcast channel closed");
                        break;
                    }
                }
            }
            result = receiver.next() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!(owner = %owner, "event WebSocket closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!(owner = %owner, error = %e, "WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Clean up if no more subscribers
    if broadcaster.subscriber_count(&owner) <= 1 {
        broadcaster.remove_owner(&owner);
    }

    tracing::info!(owner = %owner, "event WebSocket disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garage_event_ttl_warning_serialization() {
        let event = GarageEvent::TtlWarning {
            garage: "bold-mongoose".to_string(),
            minutes_remaining: 15,
            expires_at: "2026-01-21T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"ttl_warning""#));
        assert!(json.contains(r#""garage":"bold-mongoose""#));
        assert!(json.contains(r#""minutes_remaining":15"#));
    }

    #[test]
    fn garage_event_status_change_serialization() {
        let event = GarageEvent::StatusChange {
            garage: "bold-mongoose".to_string(),
            from: "Pending".to_string(),
            to: "Initializing".to_string(),
            reason: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"status_change""#));
        assert!(json.contains(r#""from":"Pending""#));
        assert!(json.contains(r#""to":"Initializing""#));
        assert!(!json.contains("reason"));
    }

    #[test]
    fn garage_event_status_change_with_reason_serialization() {
        let event = GarageEvent::StatusChange {
            garage: "bold-mongoose".to_string(),
            from: "Ready".to_string(),
            to: "Terminated".to_string(),
            reason: Some("ttl_expired".to_string()),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""reason":"ttl_expired""#));
    }

    #[test]
    fn garage_event_error_serialization() {
        let event = GarageEvent::Error {
            garage: "bold-mongoose".to_string(),
            message: "Pod crash loop detected".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""message":"Pod crash loop detected""#));
    }

    #[test]
    fn garage_event_name() {
        let event = GarageEvent::TtlWarning {
            garage: "test-garage".to_string(),
            minutes_remaining: 5,
            expires_at: "2026-01-21T12:00:00Z".to_string(),
        };
        assert_eq!(event.garage_name(), "test-garage");
    }

    #[test]
    fn event_stream_query_defaults() {
        let query: EventStreamQuery = serde_json::from_str("{}").unwrap();
        assert!(query.garages.is_none());
    }

    #[test]
    fn event_stream_query_with_garages() {
        let query: EventStreamQuery =
            serde_json::from_str(r#"{"garages":"bold-mongoose,quiet-falcon"}"#).unwrap();
        assert_eq!(query.garages.as_deref(), Some("bold-mongoose,quiet-falcon"));
    }

    #[test]
    fn event_broadcaster_subscribe_and_broadcast() {
        let broadcaster = EventBroadcaster::new();
        let mut rx = broadcaster.subscribe("owner-1");

        let event = GarageEvent::StatusChange {
            garage: "test".to_string(),
            from: "Pending".to_string(),
            to: "Ready".to_string(),
            reason: None,
        };
        broadcaster.broadcast("owner-1", event);

        let received = rx.try_recv().unwrap();
        assert!(matches!(received, GarageEvent::StatusChange { .. }));
    }

    #[test]
    fn event_broadcaster_subscriber_count() {
        let broadcaster = EventBroadcaster::new();
        assert_eq!(broadcaster.subscriber_count("owner-1"), 0);

        let _rx1 = broadcaster.subscribe("owner-1");
        assert_eq!(broadcaster.subscriber_count("owner-1"), 1);

        let _rx2 = broadcaster.subscribe("owner-1");
        assert_eq!(broadcaster.subscriber_count("owner-1"), 2);
    }

    #[test]
    fn event_broadcaster_isolated_owners() {
        let broadcaster = EventBroadcaster::new();
        let _rx1 = broadcaster.subscribe("owner-1");
        let mut rx2 = broadcaster.subscribe("owner-2");

        let event = GarageEvent::Error {
            garage: "test".to_string(),
            message: "fail".to_string(),
        };
        broadcaster.broadcast("owner-1", event);

        // owner-2 should not receive owner-1's events
        assert!(rx2.try_recv().is_err());
    }

    #[test]
    fn event_broadcaster_remove_owner() {
        let broadcaster = EventBroadcaster::new();
        let _rx = broadcaster.subscribe("owner-1");
        assert_eq!(broadcaster.subscriber_count("owner-1"), 1);

        broadcaster.remove_owner("owner-1");
        assert_eq!(broadcaster.subscriber_count("owner-1"), 0);
    }

    #[test]
    fn garage_event_deserialization() {
        let json = r#"{"type":"ttl_warning","garage":"test","minutes_remaining":5,"expires_at":"2026-01-21T12:00:00Z"}"#;
        let event: GarageEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(
            event,
            GarageEvent::TtlWarning {
                minutes_remaining: 5,
                ..
            }
        ));
    }
}
