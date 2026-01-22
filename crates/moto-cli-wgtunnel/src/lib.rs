//! CLI `WireGuard` tunnel integration for moto.
//!
//! This crate provides the client-side tunnel management for moto's
//! `WireGuard`-based connectivity to garages. It handles:
//!
//! - [`tunnel`]: Tunnel lifecycle management (create, connect, close)
//! - [`status`]: Connection status display
//! - Key management (device keypair, device ID)
//! - Configuration file handling
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                          moto-cli-wgtunnel                                   │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │  TunnelManager                                                       │    │
//! │  │  - Device identity (WG keypair, device ID)                          │    │
//! │  │  - Active tunnel sessions                                           │    │
//! │  │  - Key file management (~/.config/moto/)                            │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! │                                  │                                           │
//! │                                  ▼                                           │
//! │  ┌─────────────────────────────────────────────────────────────────────┐    │
//! │  │  TunnelSession                                                       │    │
//! │  │  - Connection to a single garage                                    │    │
//! │  │  - `WireGuard` tunnel state                                         │    │
//! │  │  - Path status (direct/DERP)                                        │    │
//! │  └─────────────────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Files
//!
//! ```text
//! ~/.config/moto/
//! ├── wg-private.key      # WireGuard private key (generated once)
//! ├── wg-public.key       # WireGuard public key
//! └── device-id           # UUID, unique device identifier
//! ```
//!
//! # Example
//!
//! ```ignore
//! use moto_cli_wgtunnel::tunnel::TunnelManager;
//!
//! // Initialize tunnel manager (loads or generates device keys)
//! let manager = TunnelManager::new().await?;
//!
//! // Get device info for registration with moto-club
//! let device_info = manager.device_info();
//! println!("Device ID: {}", device_info.device_id);
//! println!("Public Key: {}", device_info.public_key);
//! ```
//!
//! # Configuration
//!
//! Environment variables:
//! - `MOTO_WG_KEY_FILE`: Override `WireGuard` key location
//!
//! Config file (`~/.config/moto/config.toml`):
//! ```toml
//! [wgtunnel]
//! prefer_direct = true
//! direct_timeout_secs = 3
//! derp_timeout_secs = 10
//! keepalive_secs = 25
//! ```

pub mod status;
pub mod tunnel;

pub use status::{TunnelStatusInfo, TunnelStatusResponse, format_status_table, get_tunnel_status};
pub use tunnel::{
    DeviceIdentity, ENV_WG_KEY_FILE, KEY_DIR_PERMISSIONS, KEY_FILE_PERMISSIONS, TunnelError,
    TunnelManager, TunnelSession, TunnelStatus,
};
