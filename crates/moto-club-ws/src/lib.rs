//! WebSocket handlers for moto-club.
//!
//! This crate provides WebSocket endpoints for real-time streaming:
//! - Peer streaming (`/internal/wg/garages/{id}/peers`) - real-time peer updates for garages
//! - Log streaming (`/ws/v1/garages/{name}/logs`) - real-time log streaming for garages
//!
//! # Example
//!
//! ```ignore
//! use moto_club_ws::peers::handle_peers_socket;
//! use moto_club_ws::logs::handle_log_socket;
//!
//! // The WebSocket handlers are used with moto-club-api's AppState
//! // See moto-club-api for full integration example
//! ```

pub mod logs;
pub mod peers;

// Re-export main types for convenience
pub use logs::{
    GarageInfo, LogMessage, LogStreamError, LogStreamQuery, LogStreamingContext, handle_log_socket,
};
pub use peers::{PeerStreamingContext, handle_peers_socket};
