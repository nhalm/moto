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
//! # Example
//!
//! ```
//! use moto_club_wg::ipam::{Ipam, InMemoryStore};
//! use moto_club_wg::peers::{PeerRegistry, InMemoryPeerStore, DeviceRegistration};
//! use moto_wgtunnel_types::keys::WgPrivateKey;
//!
//! # tokio_test::block_on(async {
//! // Create IPAM and peer registry
//! let ipam_store = InMemoryStore::new();
//! let peer_store = InMemoryPeerStore::new();
//! let ipam = Ipam::new(ipam_store);
//! let registry = PeerRegistry::new(peer_store, ipam);
//!
//! // Register a device - public key IS the device identity
//! let private_key = WgPrivateKey::generate();
//!
//! let registration = DeviceRegistration {
//!     public_key: private_key.public_key(),
//!     owner: "myuser".to_string(),
//!     device_name: Some("my-laptop".to_string()),
//! };
//!
//! let device = registry.register_device(registration).await.unwrap();
//! assert!(device.overlay_ip.is_client());
//! # });
//! ```

pub mod broadcaster;
pub mod derp;
pub mod ipam;
pub mod peers;
pub mod sessions;

pub use broadcaster::{PeerAction, PeerBroadcaster, PeerEvent};
pub use derp::{
    DEFAULT_DERP_CONFIG_PATH, DERP_CONFIG_ENV_VAR, DerpConfigFile, DerpConfigNode,
    DerpConfigRegion, DerpError, DerpMapManager, DerpStore, InMemoryDerpStore, load_derp_config,
    load_derp_config_from_path,
};
pub use ipam::{InMemoryStore, Ipam, IpamError, IpamStore};
pub use peers::{
    DeviceRegistration, GarageRegistration, InMemoryPeerStore, PeerError, PeerRegistry, PeerStore,
    RegisteredDevice, RegisteredGarage,
};
pub use sessions::{
    CreateSessionRequest, CreateSessionResponse, DEFAULT_SESSION_TTL_SECS,
    DISCONNECT_GRACE_PERIOD_SECS, GarageConnectionInfo, InMemorySessionStore, Session,
    SessionError, SessionManager, SessionStore,
};
