//! Server-side `WireGuard` coordination for moto-club.
//!
//! This crate provides the coordination layer for `WireGuard` tunnels in moto-club:
//!
//! - [`ipam`]: IP address allocation for garages and client devices
//! - [`peers`]: Peer registration for devices and garages
//! - [`sessions`]: Tunnel session management
//! - [`ssh_keys`]: User SSH key management for garage access
//!
//! # Architecture
//!
//! moto-club coordinates `WireGuard` peer discovery and IP allocation but never
//! sees tunnel traffic. The traffic flows directly peer-to-peer (or via DERP relay
//! when NAT blocks direct connections).
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                           moto-club                                  │
//! │  ┌───────────────────────────────────────────────────────────────┐  │
//! │  │  Coordination APIs                                             │  │
//! │  │  ├── IP Allocator (fd00:moto::/48)  ← this crate              │  │
//! │  │  ├── Peer Registration              ← this crate              │  │
//! │  │  ├── Session Management             ← this crate              │  │
//! │  │  ├── SSH Key Management             ← this crate              │  │
//! │  │  └── DERP Map Provider                                        │  │
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
//! use uuid::Uuid;
//!
//! # tokio_test::block_on(async {
//! // Create IPAM and peer registry
//! let ipam_store = InMemoryStore::new();
//! let peer_store = InMemoryPeerStore::new();
//! let ipam = Ipam::new(ipam_store);
//! let registry = PeerRegistry::new(peer_store, ipam);
//!
//! // Register a device
//! let device_id = Uuid::now_v7();
//! let private_key = WgPrivateKey::generate();
//!
//! let registration = DeviceRegistration {
//!     device_id,
//!     public_key: private_key.public_key(),
//!     device_name: Some("my-laptop".to_string()),
//! };
//!
//! let device = registry.register_device(registration).await.unwrap();
//! assert!(device.overlay_ip.is_client());
//! # });
//! ```

pub mod ipam;
pub mod peers;
pub mod sessions;
pub mod ssh_keys;

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
pub use ssh_keys::{
    InMemorySshKeyStore, RegisteredSshKey, SshKeyError, SshKeyManager, SshKeyRegistration,
    SshKeyResponse, SshKeyStore,
};
