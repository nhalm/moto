//! Connection multiplexer for `WireGuard` tunnels.
//!
//! This crate provides the connection management layer for moto's `WireGuard` tunnel
//! system. It handles:
//!
//! - [`stun`]: STUN client for NAT discovery
//! - [`endpoint`]: Endpoint selection logic
//! - [`path`]: Path status tracking (Direct/DERP)
//! - [`magic`]: `MagicConn` UDP + DERP multiplexer
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
//! # `MagicConn` - Connection Multiplexer
//!
//! `MagicConn` handles direct UDP vs DERP relay connections transparently.
//! It attempts direct connections first, falling back to DERP when NAT blocks
//! direct communication.
//!
//! ```ignore
//! use moto_wgtunnel_conn::magic::{MagicConn, MagicConnConfig};
//! use moto_wgtunnel_types::{WgPrivateKey, DerpMap};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let private_key = WgPrivateKey::generate();
//! let derp_map = DerpMap::new();
//!
//! let config = MagicConnConfig::new(private_key, derp_map);
//! let conn = MagicConn::new(config).await?;
//!
//! // Add a peer
//! let peer_key = WgPrivateKey::generate().public_key();
//! conn.add_peer(&peer_key, vec!["192.0.2.1:51820".parse()?]).await;
//!
//! // Connect to peer (tries direct first, falls back to DERP)
//! conn.connect(&peer_key).await?;
//!
//! // Send packets
//! conn.send(&peer_key, b"encrypted wireguard packet").await?;
//!
//! // Receive packets
//! let packet = conn.recv().await?;
//! # Ok(())
//! # }
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
//! # Path Selection
//!
//! The connection multiplexer automatically selects the best path:
//! 1. Try direct UDP connection (3 second timeout)
//! 2. If direct fails, use DERP relay
//! 3. No upgrade attempts once on DERP (simplicity for v1)

pub mod endpoint;
pub mod magic;
pub mod path;
pub mod stun;

pub use endpoint::{
    DEFAULT_DERP_TIMEOUT, DEFAULT_DIRECT_TIMEOUT, Endpoint, EndpointConfig, EndpointSelector,
};
pub use magic::{MagicConn, MagicConnConfig, MagicConnError, ReceivedPacket};
pub use path::{PathQuality, PathState, PathType};
pub use stun::{StunClient, StunError, StunResult};
