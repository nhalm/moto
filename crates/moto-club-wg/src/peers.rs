//! Peer registration for `WireGuard` coordination.
//!
//! This module handles registration of devices and garages as `WireGuard` peers:
//!
//! - **Devices:** User devices register their public key and get an overlay IP
//! - **Garages:** Garage pods register their ephemeral public key and endpoints
//!
//! # Architecture
//!
//! The `WireGuard` public key IS the device identity (Cloudflare WARP model).
//! No separate device ID is needed. Peer registration is the first step in establishing a tunnel:
//!
//! ```text
//! Device Registration:
//!   POST /api/v1/wg/devices { public_key, device_name }
//!   → { public_key, assigned_ip }
//!
//! Garage Registration:
//!   POST /api/v1/wg/garages { garage_id, public_key, endpoints }
//!   → { assigned_ip, derp_map }
//! ```
//!
//! # Storage
//!
//! The [`PeerStore`] trait defines the storage interface. For production,
//! use `PostgresPeerStore` from `moto-club-api`.

use chrono::{DateTime, Utc};
use moto_wgtunnel_types::{OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::ipam::{Ipam, IpamStore};

/// Error type for peer operations.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    /// IPAM operation failed.
    #[error("IPAM error: {0}")]
    Ipam(#[from] crate::ipam::IpamError),

    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Device not found (identified by `WireGuard` public key).
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    /// Device not owned by requesting user.
    #[error("device not owned by requesting user")]
    DeviceNotOwned,

    /// Garage not found.
    #[error("garage not found: {0}")]
    GarageNotFound(String),
}

/// Result type for peer operations.
pub type Result<T> = std::result::Result<T, PeerError>;

/// Request to register a device.
///
/// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
/// No separate device ID is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistration {
    /// Device's `WireGuard` public key (IS the device identity).
    pub public_key: WgPublicKey,

    /// Owner of the device (from Bearer token).
    pub owner: String,

    /// Optional human-readable device name (e.g., "macbook-pro").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

/// Registered device information.
///
/// The `WireGuard` public key IS the device identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredDevice {
    /// Device's `WireGuard` public key (IS the device identity).
    pub public_key: WgPublicKey,

    /// Owner of the device (from Bearer token).
    pub owner: String,

    /// Assigned overlay IP address.
    pub overlay_ip: OverlayIp,

    /// Optional human-readable device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,

    /// When the device was registered.
    pub created_at: DateTime<Utc>,
}

/// Request to register a garage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarageRegistration {
    /// Garage identifier (e.g., "feature-foo").
    pub garage_id: String,

    /// Garage's ephemeral `WireGuard` public key.
    pub public_key: WgPublicKey,

    /// Direct UDP endpoints for P2P connections.
    #[serde(default)]
    pub endpoints: Vec<SocketAddr>,
}

/// Registered garage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredGarage {
    /// Garage identifier.
    pub garage_id: String,

    /// Garage's `WireGuard` public key.
    pub public_key: WgPublicKey,

    /// Assigned overlay IP address.
    pub overlay_ip: OverlayIp,

    /// Direct UDP endpoints for P2P connections.
    pub endpoints: Vec<SocketAddr>,

    /// Peer version, incremented on session create/close.
    pub peer_version: i32,

    /// When the garage registered.
    pub registered_at: DateTime<Utc>,
}

/// Storage backend for peer registry.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait PeerStore: Send + Sync {
    /// Get a registered device by public key.
    ///
    /// The `WireGuard` public key IS the device identity.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_device(&self, public_key: &WgPublicKey) -> Result<Option<RegisteredDevice>>;

    /// Store a device registration.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_device(&self, device: RegisteredDevice) -> Result<()>;

    /// Get a registered garage by ID.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>>;

    /// Store a garage registration.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_garage(&self, garage: RegisteredGarage) -> Result<()>;

    /// Remove a garage registration.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>>;

    /// List all registered garages.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn list_garages(&self) -> Result<Vec<RegisteredGarage>>;
}

/// Peer registry for managing device and garage registrations.
///
/// Coordinates between peer storage and IP allocation.
pub struct PeerRegistry<P, I> {
    store: P,
    ipam: Ipam<I>,
}

impl<P: PeerStore, I: IpamStore> PeerRegistry<P, I> {
    /// Create a new peer registry.
    #[must_use]
    pub const fn new(store: P, ipam: Ipam<I>) -> Self {
        Self { store, ipam }
    }

    /// Register a device.
    ///
    /// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
    /// If the device (public key) is already registered, returns the existing registration.
    /// Re-registration is idempotent.
    ///
    /// # Errors
    ///
    /// Returns error if storage or IPAM operations fail.
    pub async fn register_device(
        &self,
        req: DeviceRegistration,
    ) -> Result<(RegisteredDevice, bool)> {
        // Check if device is already registered (by public key)
        if let Some(mut existing) = self.store.get_device(&req.public_key)? {
            // Check ownership - same key registered to different user is forbidden
            if existing.owner != req.owner {
                return Err(PeerError::DeviceNotOwned);
            }

            // Update device name if it changed (idempotent)
            if existing.device_name != req.device_name {
                existing.device_name = req.device_name;
                self.store.set_device(existing.clone())?;
            }
            return Ok((existing, false));
        }

        // Allocate IP for new device (keyed by public key)
        let overlay_ip = self.ipam.allocate_client(&req.public_key).await?;

        let device = RegisteredDevice {
            public_key: req.public_key,
            owner: req.owner,
            overlay_ip,
            device_name: req.device_name,
            created_at: Utc::now(),
        };

        self.store.set_device(device.clone())?;
        Ok((device, true))
    }

    /// Get a registered device by public key.
    ///
    /// The `WireGuard` public key IS the device identity.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_device(&self, public_key: &WgPublicKey) -> Result<Option<RegisteredDevice>> {
        self.store.get_device(public_key)
    }

    /// Register a garage.
    ///
    /// If the garage is already registered, updates the public key and endpoints.
    ///
    /// # Errors
    ///
    /// Returns error if storage or IPAM operations fail.
    pub async fn register_garage(&self, req: GarageRegistration) -> Result<RegisteredGarage> {
        // Allocate IP (deterministic for garages)
        let overlay_ip = self.ipam.allocate_garage(&req.garage_id).await?;

        let garage = RegisteredGarage {
            garage_id: req.garage_id,
            public_key: req.public_key,
            overlay_ip,
            endpoints: req.endpoints,
            peer_version: 0,
            registered_at: Utc::now(),
        };

        self.store.set_garage(garage.clone())?;
        Ok(garage)
    }

    /// Get a registered garage.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>> {
        self.store.get_garage(garage_id)
    }

    /// Unregister a garage.
    ///
    /// Called when a garage terminates.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn unregister_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>> {
        self.store.remove_garage(garage_id)
    }

    /// List all registered garages.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn list_garages(&self) -> Result<Vec<RegisteredGarage>> {
        self.store.list_garages()
    }
}

// Unit tests for pure functions (no database needed)
#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[test]
    fn device_registration_serde() {
        let registration = DeviceRegistration {
            public_key: generate_public_key(),
            owner: "testuser".to_string(),
            device_name: Some("test".to_string()),
        };

        let json = serde_json::to_string(&registration).unwrap();
        let parsed: DeviceRegistration = serde_json::from_str(&json).unwrap();

        assert_eq!(registration.public_key, parsed.public_key);
        assert_eq!(registration.owner, parsed.owner);
        assert_eq!(registration.device_name, parsed.device_name);
    }

    #[test]
    fn garage_registration_serde() {
        let registration = GarageRegistration {
            garage_id: "test-garage".to_string(),
            public_key: generate_public_key(),
            endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
        };

        let json = serde_json::to_string(&registration).unwrap();
        let parsed: GarageRegistration = serde_json::from_str(&json).unwrap();

        assert_eq!(registration.garage_id, parsed.garage_id);
        assert_eq!(registration.public_key, parsed.public_key);
        assert_eq!(registration.endpoints, parsed.endpoints);
    }
}

// Integration tests that require PostgreSQL
// Note: These tests require PostgresIpamStore and PostgresPeerStore from moto-club-api.
// See moto-club-api/src/wg_test.rs for integration tests with PostgreSQL storage.
//
// Tests to implement:
// - register_device: Register a device and verify overlay IP is assigned
// - device_registration_is_idempotent: Same public key gets same IP
// - new_key_is_new_device: Different public key = different device
// - get_device: Lookup device by public key
// - register_garage: Register a garage with endpoints
// - garage_ip_is_deterministic: Same garage ID = same IP
// - get_garage: Lookup garage by ID
// - unregister_garage: Remove garage registration
// - list_garages: List all registered garages
