//! WebSocket handlers for moto-club.
//!
//! This crate provides WebSocket endpoints for real-time streaming:
//! - Peer streaming (`/internal/wg/garages/{id}/peers`) - real-time peer updates for garages
//!
//! Future endpoints (deferred):
//! - Log streaming - real-time log streaming for garages
//! - Events - real-time events for garage lifecycle changes
//!
//! # Example
//!
//! ```ignore
//! use moto_club_ws::peers::handle_peers_socket;
//!
//! // The WebSocket handler is used with moto-club-api's AppState
//! // See moto-club-api for full integration example
//! ```

pub mod peers;

// Re-export main types for convenience
pub use peers::{PeerStreamingContext, handle_peers_socket};
