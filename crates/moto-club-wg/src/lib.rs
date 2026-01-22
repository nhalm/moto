//! Server-side `WireGuard` coordination for moto-club.
//!
//! This crate provides the coordination layer for `WireGuard` tunnels in moto-club:
//!
//! - [`ipam`]: IP address allocation for garages and client devices
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
//! │  │  ├── Peer Registration                                        │  │
//! │  │  ├── Session Management                                       │  │
//! │  │  └── DERP Map Provider                                        │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::ipam::{Ipam, InMemoryStore};
//! use uuid::Uuid;
//!
//! # tokio_test::block_on(async {
//! // Create IPAM with in-memory store (use PostgreSQL in production)
//! let store = InMemoryStore::new();
//! let ipam = Ipam::new(store);
//!
//! // Allocate garage IP (deterministic from garage ID)
//! let garage_ip = ipam.allocate_garage("my-garage").await.unwrap();
//! assert!(garage_ip.is_garage());
//!
//! // Allocate client IP (sequential, persisted per device)
//! let device_id = Uuid::now_v7();
//! let client_ip = ipam.allocate_client(device_id).await.unwrap();
//! assert!(client_ip.is_client());
//! # });
//! ```

pub mod ipam;

pub use ipam::{Ipam, IpamError, IpamStore, InMemoryStore};
