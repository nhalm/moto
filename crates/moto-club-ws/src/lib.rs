//! WebSocket handlers for moto-club.
//!
//! This crate provides WebSocket endpoints for real-time streaming:
//! - Peer streaming (`/internal/wg/garages/{id}/peers`) - real-time peer updates for garages
//! - Log streaming (`/ws/v1/garages/{name}/logs`) - real-time log streaming for garages
//! - Event streaming (`/ws/v1/events`) - real-time garage event notifications
//!
//! # Example
//!
//! ```ignore
//! use moto_club_ws::peers::handle_peers_socket;
//! use moto_club_ws::logs::handle_log_socket;
//! use moto_club_ws::events::handle_event_socket;
//!
//! // The WebSocket handlers are used with moto-club-api's AppState
//! // See moto-club-api for full integration example
//! ```

pub mod events;
pub mod logs;
pub mod peers;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Tracks concurrent connections per key with a configurable maximum.
///
/// Used to enforce connection limits on WebSocket endpoints:
/// - Max 5 log streaming connections per garage
/// - Max 3 event streaming connections per user
pub struct ConnectionTracker {
    counts: Mutex<HashMap<String, usize>>,
}

impl ConnectionTracker {
    /// Create a new connection tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
        }
    }

    /// Try to acquire a connection slot. Returns a guard that releases the slot on drop.
    /// Returns `None` if the limit for the given key has been reached.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn try_acquire(self: &Arc<Self>, key: &str, max: usize) -> Option<ConnectionGuard> {
        let acquired = self.increment_if_below(key, max);
        if acquired {
            Some(ConnectionGuard {
                tracker: Arc::clone(self),
                key: key.to_string(),
            })
        } else {
            None
        }
    }

    /// Increment the count for a key if it is below the maximum. Returns whether it was incremented.
    fn increment_if_below(&self, key: &str, max: usize) -> bool {
        let mut counts = self.counts.lock().unwrap();
        let count = counts.entry(key.to_string()).or_insert(0);
        if *count >= max {
            return false;
        }
        *count += 1;
        drop(counts);
        true
    }

    /// Get the current connection count for a key.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn count(&self, key: &str) -> usize {
        self.counts.lock().unwrap().get(key).copied().unwrap_or(0)
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that decrements the connection count when dropped.
///
/// Created by [`ConnectionTracker::try_acquire`]. When this guard is dropped
/// (e.g., when the WebSocket handler returns), the connection slot is released.
pub struct ConnectionGuard {
    tracker: Arc<ConnectionTracker>,
    key: String,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut counts = self.tracker.counts.lock().unwrap();
        if let Some(count) = counts.get_mut(&self.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.key);
            }
        }
    }
}

// Re-export main types for convenience
pub use events::{
    EventBroadcaster, EventStreamQuery, EventStreamingContext, GarageEvent, handle_event_socket,
};
pub use logs::{
    GarageInfo, LogMessage, LogStreamError, LogStreamQuery, LogStreamingContext, handle_log_socket,
};
pub use peers::{PeerStreamingContext, handle_peers_socket};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_acquire_within_limit() {
        let tracker = Arc::new(ConnectionTracker::new());
        let g1 = tracker.try_acquire("garage-1", 3);
        assert!(g1.is_some());
        assert_eq!(tracker.count("garage-1"), 1);
    }

    #[test]
    fn tracker_acquire_up_to_limit() {
        let tracker = Arc::new(ConnectionTracker::new());
        let _g1 = tracker.try_acquire("garage-1", 2).unwrap();
        let _g2 = tracker.try_acquire("garage-1", 2).unwrap();
        assert_eq!(tracker.count("garage-1"), 2);

        // Third should fail
        assert!(tracker.try_acquire("garage-1", 2).is_none());
    }

    #[test]
    fn tracker_guard_releases_on_drop() {
        let tracker = Arc::new(ConnectionTracker::new());
        let guard = tracker.try_acquire("garage-1", 1).unwrap();
        assert_eq!(tracker.count("garage-1"), 1);

        drop(guard);
        assert_eq!(tracker.count("garage-1"), 0);

        // Can acquire again after release
        assert!(tracker.try_acquire("garage-1", 1).is_some());
    }

    #[test]
    fn tracker_isolated_keys() {
        let tracker = Arc::new(ConnectionTracker::new());
        let _g1 = tracker.try_acquire("garage-1", 1).unwrap();
        let _g2 = tracker.try_acquire("garage-2", 1).unwrap();
        assert_eq!(tracker.count("garage-1"), 1);
        assert_eq!(tracker.count("garage-2"), 1);

        // Each key has its own limit
        assert!(tracker.try_acquire("garage-1", 1).is_none());
        assert!(tracker.try_acquire("garage-2", 1).is_none());
    }

    #[test]
    fn tracker_count_unknown_key() {
        let tracker = ConnectionTracker::new();
        assert_eq!(tracker.count("nonexistent"), 0);
    }
}
