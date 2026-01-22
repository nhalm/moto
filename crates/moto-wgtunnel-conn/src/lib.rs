//! Connection multiplexer for `WireGuard` tunnels.
//!
//! This crate provides the connection management layer for moto's `WireGuard` tunnel
//! system. It handles:
//!
//! - [`stun`]: STUN client for NAT discovery
//! - [`endpoint`]: Endpoint selection logic
//! - [`path`]: Path status tracking (Direct/DERP)
//! - `magic` (future): `MagicConn` UDP + DERP multiplexer
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                           MagicConn                                      │
//! │  ┌─────────────────────────────┐  ┌─────────────────────────────────┐   │
//! │  │      Direct UDP Path        │  │        DERP Relay Path          │   │
//! │  │  - STUN for NAT discovery   │  │  - Fallback when direct fails   │   │
//! │  │  - 3 second timeout         │  │  - Self-hosted DERP servers     │   │
//! │  └─────────────────────────────┘  └─────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # STUN for NAT Discovery
//!
//! STUN (Session Traversal Utilities for NAT) is used to discover the public
//! IP address and port mapping of a client behind NAT. This information is
//! used to establish direct `WireGuard` connections when possible.
//!
//! ```no_run
//! use moto_wgtunnel_conn::stun::{StunClient, StunResult};
//! use std::net::SocketAddr;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = StunClient::new();
//!
//! // Discover public endpoint via STUN server
//! let stun_server: SocketAddr = "stun.example.com:3478".parse()?;
//! let result = client.discover(stun_server, Duration::from_secs(3)).await?;
//!
//! println!("My public endpoint: {}", result.reflexive_addr);
//! # Ok(())
//! # }
//! ```
//!
//! # Endpoint Selection
//!
//! The [`endpoint`] module provides [`EndpointSelector`] for choosing the best
//! endpoint to connect to a peer:
//!
//! ```
//! use moto_wgtunnel_conn::endpoint::{Endpoint, EndpointSelector, EndpointConfig};
//! use std::net::SocketAddr;
//!
//! let mut selector = EndpointSelector::with_defaults();
//!
//! // Add direct endpoint from peer info
//! selector.add_direct("203.0.113.5:51820".parse().unwrap());
//!
//! // Add DERP regions as fallback
//! selector.add_derp(1, "primary");
//!
//! // Get endpoints in priority order (direct first, then DERP)
//! while let Some(endpoint) = selector.next_endpoint() {
//!     println!("Trying: {}", endpoint);
//! }
//! ```
//!
//! # Path Selection (Future)
//!
//! The connection multiplexer will automatically select the best path:
//! 1. Try direct UDP connection (3 second timeout)
//! 2. If direct fails, use DERP relay
//! 3. No upgrade attempts once on DERP (simplicity for v1)

pub mod endpoint;
pub mod path;
pub mod stun;

pub use endpoint::{
    DEFAULT_DERP_TIMEOUT, DEFAULT_DIRECT_TIMEOUT, Endpoint, EndpointConfig, EndpointSelector,
};
pub use path::{PathQuality, PathState, PathType};
pub use stun::{StunClient, StunError, StunResult};
