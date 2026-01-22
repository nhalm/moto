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
//! │  │  └── SSH Integration (accept connections from tunnel)         │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`register`]: Registration with moto-club coordination server

pub mod register;

pub use register::{
    GarageRegistrar, RegistrationConfig, RegistrationError, RegistrationResponse,
};
