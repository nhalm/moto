//! Shared types for the moto `WireGuard` tunnel system.
//!
//! This crate provides common types used across all wgtunnel crates:
//! - [`keys`]: `WireGuard` keypair types ([`WgPrivateKey`], [`WgPublicKey`])
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_types::keys::{WgPrivateKey, WgPublicKey};
//!
//! // Generate a new keypair
//! let private_key = WgPrivateKey::generate();
//! let public_key = private_key.public_key();
//!
//! // Serialize public key for transmission
//! let base64 = public_key.to_base64();
//! println!("Public key: {}", base64);
//! ```

pub mod keys;

pub use keys::{KeyError, WgPrivateKey, WgPublicKey};
