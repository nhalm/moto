//! Garage-side `WireGuard` tunnel daemon.
//!
//! This crate provides the `WireGuard` tunnel functionality that runs inside
//! garage pods, enabling secure terminal access (ttyd) from user devices.
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
//! │  │  └── Health Endpoint (liveness/readiness probes)              │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! │                                                                      │
//! │  ┌───────────────────────────────────────────────────────────────┐  │
//! │  │  Terminal daemon (ttyd + tmux, no auth - tunnel is auth)       │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`daemon`]: Main daemon loop coordinating all components
//! - [`register`]: Registration with moto-club coordination server
//! - [`health`]: Health endpoint for Kubernetes probes and monitoring

pub mod daemon;
pub mod health;
pub mod register;

pub use daemon::{Daemon, DaemonConfig, DaemonError, PeerState};
pub use health::{HealthCheck, HealthStatus, OverallStatus, WireGuardState, health_router};
pub use register::{GarageRegistrar, RegistrationConfig, RegistrationError, RegistrationResponse};
