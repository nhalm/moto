//! CLI `WireGuard` tunnel integration for moto.
//!
//! This crate provides the client-side tunnel management for moto's
//! `WireGuard`-based connectivity to garages. It handles:
//!
//! - [`enter`]: Garage enter command (establish tunnel and SSH session)
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
//! use moto_cli_wgtunnel::{TunnelManager, enter::{enter_garage, EnterConfig, ConsoleProgress}};
//!
//! // Initialize tunnel manager (loads or generates device keys)
//! let manager = TunnelManager::new().await?;
//!
//! // Enter a garage
//! let config = EnterConfig::default();
//! let progress = ConsoleProgress::new(false);
//! let session = enter_garage(&manager, "my-garage", config, &progress).await?;
//!
//! // SSH target is available
//! println!("SSH target: {}", session.ssh_target());
//! ```
//!
//! # Configuration
//!
//! Environment variables:
//! - `MOTO_WG_KEY_FILE`: Override `WireGuard` key location
//! - `MOTO_WGTUNNEL_DERP_ONLY`: Force DERP-only mode (skip direct connection attempts)
//!
//! Config file (`~/.config/moto/config.toml`):
//! ```toml
//! [wgtunnel]
//! prefer_direct = true
//! direct_timeout_secs = 3
//! derp_timeout_secs = 10
//! keepalive_secs = 25
//! ```

pub mod enter;
pub mod status;
pub mod tunnel;

pub use enter::{
    ConsoleProgress, EnterConfig, EnterError, EnterProgress, EnterResult, GarageSession,
    GarageWgInfo, SessionResponse, SilentProgress, enter_garage, get_existing_session,
};
pub use status::{TunnelStatusInfo, TunnelStatusResponse, format_status_table, get_tunnel_status};
pub use tunnel::{
    DeviceIdentity, ENV_WG_KEY_FILE, KEY_DIR_PERMISSIONS, KEY_FILE_PERMISSIONS, TunnelError,
    TunnelManager, TunnelSession, TunnelStatus,
};
