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

// Re-export main types for convenience
pub use events::{
    EventBroadcaster, EventStreamQuery, EventStreamingContext, GarageEvent, handle_event_socket,
};
pub use logs::{
    GarageInfo, LogMessage, LogStreamError, LogStreamQuery, LogStreamingContext, handle_log_socket,
};
pub use peers::{PeerStreamingContext, handle_peers_socket};
