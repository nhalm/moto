// Allow simple setters to not be const fn - keeping them non-const is fine
#![allow(clippy::missing_const_for_fn)]

//! `WireGuard` engine for moto tunnels.
//!
//! This crate provides the `WireGuard` tunnel implementation using boringtun
//! (a userspace `WireGuard` implementation). It handles:
//!
//! - [`config`]: Tunnel configuration (keys, peers, timing)
//! - `tunnel`: Tunnel management with boringtun (future)
//! - `platform`: Platform-specific TUN abstractions (future)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                         moto-wgtunnel-engine                                 │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │                        TunnelConfig                                  │    │
//! │  │  - InterfaceConfig (private key, local IP)                          │    │
//! │  │  - PeerConfig (public key, allowed IPs, endpoint)                   │    │
//! │  │  - TimingConfig (keepalive, timeouts)                               │    │
//! │  │  - DerpMap (relay fallback)                                         │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! │                                  │                                           │
//! │                                  ▼                                           │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │                       Tunnel (future)                                │    │
//! │  │  - boringtun WireGuard state machine                                │    │
//! │  │  - Packet encryption/decryption                                     │    │
//! │  │  - Handshake management                                             │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! │                                  │                                           │
//! │                                  ▼                                           │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │                  Platform TUN (future)                               │    │
//! │  │  - Linux: /dev/net/tun                                              │    │
//! │  │  - macOS: utun device                                               │    │
//! │  │  - In-process virtual TUN (no kernel device)                        │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Connection Flow
//!
//! 1. Create [`TunnelConfig`] with interface and peer settings
//! 2. Create `Tunnel` (future) with the config
//! 3. Tunnel uses [`MagicConn`](moto_wgtunnel_conn::MagicConn) for transport
//! 4. Packets are encrypted/decrypted by boringtun
//! 5. Platform TUN handles IP packet routing
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_engine::config::{
//!     TunnelConfig, InterfaceConfig, PeerConfig, TimingConfig, ConnectionMode,
//! };
//! use moto_wgtunnel_types::{WgPrivateKey, OverlayIp, DerpMap, DerpRegion, DerpNode};
//!
//! // Generate or load keys
//! let private_key = WgPrivateKey::generate();
//! let peer_public_key = WgPrivateKey::generate().public_key();
//!
//! // Configure the local interface
//! let interface = InterfaceConfig::new(
//!     private_key,
//!     OverlayIp::client(1),  // fd00:moto:2::1
//! );
//!
//! // Configure the peer (garage)
//! let peer = PeerConfig::with_endpoint(
//!     peer_public_key,
//!     OverlayIp::garage(0xabc1),  // fd00:moto:1::abc1
//!     "203.0.113.5:51820".parse().unwrap(),
//! );
//!
//! // Configure DERP fallback
//! let derp_map = DerpMap::new()
//!     .with_region(
//!         DerpRegion::new(1, "primary")
//!             .with_node(DerpNode::with_defaults("derp.example.com"))
//!     );
//!
//! // Build the tunnel config
//! let config = TunnelConfig::builder()
//!     .interface(interface)
//!     .peer(peer)
//!     .timing(TimingConfig::default())
//!     .derp_map(derp_map)
//!     .connection_mode(ConnectionMode::PreferDirect)
//!     .build();
//!
//! // Tunnel creation will be added in future:
//! // let tunnel = Tunnel::new(config).await?;
//! // tunnel.connect(&peer_public_key).await?;
//! ```
//!
//! # Platform Support
//!
//! | Platform | Support | Notes |
//! |----------|---------|-------|
//! | Linux | Full | `/dev/net/tun` or in-process virtual TUN |
//! | macOS | Full | utun device or in-process virtual TUN |
//! | Windows | Not supported | May be added in future |
//!
//! # Dependencies
//!
//! - [`boringtun`]: Userspace `WireGuard` implementation
//! - [`moto_wgtunnel_types`]: Shared types (keys, IPs, DERP maps)
//! - [`moto_wgtunnel_conn`]: Connection multiplexer (UDP + DERP)

pub mod config;

pub use config::{
    ConfigError, ConnectionMode, InterfaceConfig, PeerConfig, TimingConfig, TunnelConfig,
    TunnelConfigBuilder, DEFAULT_DERP_TIMEOUT_SECS, DEFAULT_DIRECT_TIMEOUT_SECS,
    DEFAULT_KEEPALIVE_SECS, DEFAULT_MTU, ENV_DERP_ONLY, ENV_LOG_LEVEL,
};
