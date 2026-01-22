//! Garage-side `WireGuard` tunnel daemon.
//!
//! This crate provides the `WireGuard` tunnel functionality that runs inside
//! garage pods, enabling secure SSH access from user devices.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                          Garage Pod                                  │
//! │  ┌───────────────────────────────────────────────────────────────┐  │
//! │  │  moto-garage-wgtunnel daemon                                   │  │
//! │  │  ├── Registration (register with moto-club on startup)        │  │
//! │  │  ├── Peer Streaming (receive peer updates via WebSocket)      │  │
//! │  │  ├── WireGuard Engine (handle encrypted packets)              │  │
//! │  │  ├── Health Endpoint (liveness/readiness probes)              │  │
//! │  │  └── SSH Integration (accept connections from tunnel)         │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`daemon`]: Main daemon loop coordinating all components
//! - [`register`]: Registration with moto-club coordination server
//! - [`health`]: Health endpoint for Kubernetes probes and monitoring
//! - [`ssh`]: SSH server configuration and authorized key management

pub mod daemon;
pub mod health;
pub mod register;
pub mod ssh;

pub use daemon::{Daemon, DaemonConfig, DaemonError, PeerState};
pub use health::{HealthCheck, HealthStatus, OverallStatus, WireGuardState};
pub use register::{
    GarageRegistrar, RegistrationConfig, RegistrationError, RegistrationResponse,
};
pub use ssh::{AuthorizedKeys, KeyType, SshConfig, SshConfigBuilder, SshError, SshPublicKey};
