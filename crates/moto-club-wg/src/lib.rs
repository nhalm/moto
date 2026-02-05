//! Server-side `WireGuard` coordination for moto-club.
//!
//! This crate provides the coordination layer for `WireGuard` tunnels in moto-club:
//!
//! - [`ipam`]: IP address allocation for garages and client devices
//! - [`peers`]: Peer registration for devices and garages
//! - [`sessions`]: Tunnel session management
//! - [`derp`]: DERP relay map management
//! - [`broadcaster`]: Real-time peer event broadcasting for garage `WebSockets`
//!
//! # Architecture
//!
//! moto-club coordinates `WireGuard` peer discovery and IP allocation but never
//! sees tunnel traffic. The traffic flows directly peer-to-peer (or via DERP relay
//! when NAT blocks direct connections).
//!
//! The `WireGuard` public key IS the device identity (Cloudflare WARP model).
//! No separate device ID is needed.
//!
//! Terminal access uses ttyd over the `WireGuard` tunnel - the tunnel is the sole
//! authentication boundary.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                           moto-club                                  │
//! │  ┌───────────────────────────────────────────────────────────────┐  │
//! │  │  Coordination APIs                                             │  │
//! │  │  ├── IP Allocator (fd00:moto::/48)  ← this crate              │  │
//! │  │  ├── Peer Registration              ← this crate              │  │
//! │  │  ├── Session Management             ← this crate              │  │
//! │  │  └── DERP Map Provider              ← this crate              │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Storage
//!
//! This crate defines traits for storage backends ([`IpamStore`], [`PeerStore`],
//! [`SessionStore`]). For production use, use the `PostgreSQL` implementations from
//! `moto-club-api` (`PostgresIpamStore`, `PostgresPeerStore`, `PostgresSessionStore`).

pub mod broadcaster;
pub mod derp;
pub mod ipam;
pub mod peers;
pub mod sessions;

pub use broadcaster::{PeerAction, PeerBroadcaster, PeerEvent};
pub use derp::{
    DERP_SERVERS_ENV_VAR, DerpConfig, DerpError, DerpServerEntry, parse_derp_servers_env,
    parse_derp_servers_json,
};
pub use ipam::{Ipam, IpamError, IpamStore};
pub use peers::{
    DeviceRegistration, GarageRegistration, PeerError, PeerRegistry, PeerStore, RegisteredDevice,
    RegisteredGarage,
};
pub use sessions::{
    CreateSessionRequest, CreateSessionResponse, DEFAULT_SESSION_TTL_SECS,
    DISCONNECT_GRACE_PERIOD_SECS, GarageConnectionInfo, Session, SessionError, SessionManager,
    SessionStore,
};
