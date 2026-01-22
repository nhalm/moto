//! Shared types for the moto `WireGuard` tunnel system.
//!
//! This crate provides common types used across all wgtunnel crates:
//! - [`keys`]: `WireGuard` keypair types ([`WgPrivateKey`], [`WgPublicKey`])
//! - [`ip`]: Overlay network IP types ([`OverlayIp`], [`GARAGE_SUBNET`], [`CLIENT_SUBNET`])
//! - [`peer`]: Peer information types ([`PeerInfo`], [`PeerAction`])
//! - [`derp`]: DERP relay map types ([`DerpMap`], [`DerpRegion`], [`DerpNode`])
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_types::keys::{WgPrivateKey, WgPublicKey};
//!
//! // Generate a new keypair
//! let private_key = WgPrivateKey::generate();
//! let public_key = private_key.public_key();
//!
//! // Serialize public key for transmission
//! let base64 = public_key.to_base64();
//! println!("Public key: {}", base64);
//! ```
//!
//! ```
//! use moto_wgtunnel_types::ip::{OverlayIp, GARAGE_SUBNET, CLIENT_SUBNET};
//!
//! // Create overlay IPs for garages and clients
//! let garage_ip = OverlayIp::garage(1);
//! let client_ip = OverlayIp::client(42);
//!
//! assert!(garage_ip.is_garage());
//! assert!(client_ip.is_client());
//! ```
//!
//! ```
//! use moto_wgtunnel_types::derp::{DerpMap, DerpRegion, DerpNode};
//!
//! // Create a DERP map for relay fallback
//! let derp_map = DerpMap::new()
//!     .with_region(
//!         DerpRegion::new(1, "primary")
//!             .with_node(DerpNode::with_defaults("derp.example.com"))
//!     );
//!
//! assert_eq!(derp_map.len(), 1);
//! ```

pub mod derp;
pub mod ip;
pub mod keys;
pub mod peer;

pub use derp::{DerpMap, DerpNode, DerpRegion, DEFAULT_DERP_PORT, DEFAULT_STUN_PORT};
pub use ip::{IpError, OverlayIp, Subnet, CLIENT_SUBNET, GARAGE_SUBNET};
pub use keys::{KeyError, WgPrivateKey, WgPublicKey};
pub use peer::{PeerAction, PeerInfo};
