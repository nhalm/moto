//! Peer registration for `WireGuard` coordination.
//!
//! This module handles registration of devices and garages as `WireGuard` peers:
//!
//! - **Devices:** User devices register their public key and get an overlay IP
//! - **Garages:** Garage pods register their ephemeral public key and endpoints
//!
//! # Architecture
//!
//! The WireGuard public key IS the device identity (Cloudflare WARP model).
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
//! # Example
//!
//! ```
//! use moto_club_wg::peers::{PeerRegistry, InMemoryPeerStore, DeviceRegistration, GarageRegistration};
//! use moto_club_wg::ipam::{Ipam, InMemoryStore};
//! use moto_wgtunnel_types::keys::WgPrivateKey;
//!
//! # tokio_test::block_on(async {
//! // Create stores
//! let ipam_store = InMemoryStore::new();
//! let peer_store = InMemoryPeerStore::new();
//!
//! // Create registry with IPAM
//! let ipam = Ipam::new(ipam_store);
//! let registry = PeerRegistry::new(peer_store, ipam);
//!
//! // Register a device - public key IS the device identity
//! let private_key = WgPrivateKey::generate();
//! let public_key = private_key.public_key();
//!
//! let registration = DeviceRegistration {
//!     public_key: public_key.clone(),
//!     device_name: Some("macbook-pro".to_string()),
//! };
//!
//! let device = registry.register_device(registration).await.unwrap();
//! assert!(device.overlay_ip.is_client());
//! # });
//! ```

use moto_wgtunnel_types::{OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

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

    /// Device not found (identified by WireGuard public key).
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    /// Garage not found.
    #[error("garage not found: {0}")]
    GarageNotFound(String),
}

/// Result type for peer operations.
pub type Result<T> = std::result::Result<T, PeerError>;

/// Request to register a device.
///
/// The WireGuard public key IS the device identity (Cloudflare WARP model).
/// No separate device ID is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistration {
    /// Device's `WireGuard` public key (IS the device identity).
    pub public_key: WgPublicKey,

    /// Optional human-readable device name (e.g., "macbook-pro").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

/// Registered device information.
///
/// The WireGuard public key IS the device identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredDevice {
    /// Device's `WireGuard` public key (IS the device identity).
    pub public_key: WgPublicKey,

    /// Assigned overlay IP address.
    pub overlay_ip: OverlayIp,

    /// Optional human-readable device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
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
}

/// Storage backend for peer registry.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait PeerStore: Send + Sync {
    /// Get a registered device by public key.
    ///
    /// The WireGuard public key IS the device identity.
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
    /// The WireGuard public key IS the device identity (Cloudflare WARP model).
    /// If the device (public key) is already registered, returns the existing registration.
    /// Re-registration is idempotent.
    ///
    /// # Errors
    ///
    /// Returns error if storage or IPAM operations fail.
    pub async fn register_device(&self, req: DeviceRegistration) -> Result<RegisteredDevice> {
        // Check if device is already registered (by public key)
        if let Some(mut existing) = self.store.get_device(&req.public_key)? {
            // Update device name if it changed (idempotent)
            if existing.device_name != req.device_name {
                existing.device_name = req.device_name;
                self.store.set_device(existing.clone())?;
            }
            return Ok(existing);
        }

        // Allocate IP for new device (keyed by public key)
        let overlay_ip = self.ipam.allocate_client(&req.public_key).await?;

        let device = RegisteredDevice {
            public_key: req.public_key,
            overlay_ip,
            device_name: req.device_name,
        };

        self.store.set_device(device.clone())?;
        Ok(device)
    }

    /// Get a registered device by public key.
    ///
    /// The WireGuard public key IS the device identity.
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

/// In-memory peer store for testing.
///
/// Registrations are lost when the store is dropped.
pub struct InMemoryPeerStore {
    inner: Mutex<InMemoryPeerStoreInner>,
}

struct InMemoryPeerStoreInner {
    /// Devices keyed by public key (base64 encoded).
    /// WireGuard public key IS the device identity.
    devices: HashMap<String, RegisteredDevice>,
    garages: HashMap<String, RegisteredGarage>,
}

impl InMemoryPeerStore {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemoryPeerStoreInner {
                devices: HashMap::new(),
                garages: HashMap::new(),
            }),
        }
    }
}

impl Default for InMemoryPeerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerStore for InMemoryPeerStore {
    fn get_device(&self, public_key: &WgPublicKey) -> Result<Option<RegisteredDevice>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.devices.get(&public_key.to_base64()).cloned())
    }

    fn set_device(&self, device: RegisteredDevice) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .devices
            .insert(device.public_key.to_base64(), device);
        Ok(())
    }

    fn get_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.garages.get(garage_id).cloned())
    }

    fn set_garage(&self, garage: RegisteredGarage) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .garages
            .insert(garage.garage_id.clone(), garage);
        Ok(())
    }

    fn remove_garage(&self, garage_id: &str) -> Result<Option<RegisteredGarage>> {
        Ok(self.inner.lock().unwrap().garages.remove(garage_id))
    }

    fn list_garages(&self) -> Result<Vec<RegisteredGarage>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.garages.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipam::InMemoryStore;
    use moto_wgtunnel_types::WgPrivateKey;

    fn create_registry() -> PeerRegistry<InMemoryPeerStore, InMemoryStore> {
        let ipam_store = InMemoryStore::new();
        let peer_store = InMemoryPeerStore::new();
        let ipam = Ipam::new(ipam_store);
        PeerRegistry::new(peer_store, ipam)
    }

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[tokio::test]
    async fn register_device() {
        let registry = create_registry();
        let public_key = generate_public_key();

        let registration = DeviceRegistration {
            public_key: public_key.clone(),
            device_name: Some("test-device".to_string()),
        };

        let device = registry.register_device(registration).await.unwrap();

        assert_eq!(device.public_key, public_key);
        assert!(device.overlay_ip.is_client());
        assert_eq!(device.device_name, Some("test-device".to_string()));
    }

    #[tokio::test]
    async fn device_registration_is_idempotent() {
        let registry = create_registry();
        let public_key = generate_public_key();

        let registration = DeviceRegistration {
            public_key: public_key.clone(),
            device_name: None,
        };

        let device1 = registry
            .register_device(registration.clone())
            .await
            .unwrap();
        let device2 = registry.register_device(registration).await.unwrap();

        // Same public key gets same IP (idempotent)
        assert_eq!(device1.overlay_ip, device2.overlay_ip);
        assert_eq!(device1.public_key, device2.public_key);
    }

    #[tokio::test]
    async fn new_key_is_new_device() {
        // Per spec: "Re-keying (new WG keypair) = new device registration, new IP"
        let registry = create_registry();
        let key1 = generate_public_key();
        let key2 = generate_public_key();

        // Register with first key
        let registration1 = DeviceRegistration {
            public_key: key1.clone(),
            device_name: None,
        };
        let device1 = registry.register_device(registration1).await.unwrap();

        // Register with new key (this is a NEW device)
        let registration2 = DeviceRegistration {
            public_key: key2.clone(),
            device_name: None,
        };
        let device2 = registry.register_device(registration2).await.unwrap();

        // Different keys = different devices = different IPs
        assert_ne!(device1.overlay_ip, device2.overlay_ip);
        assert_ne!(device1.public_key, device2.public_key);
    }

    #[tokio::test]
    async fn get_device() {
        let registry = create_registry();
        let public_key = generate_public_key();

        // Not registered yet
        assert!(registry.get_device(&public_key).unwrap().is_none());

        // Register
        let registration = DeviceRegistration {
            public_key: public_key.clone(),
            device_name: None,
        };
        registry.register_device(registration).await.unwrap();

        // Now found
        let device = registry.get_device(&public_key).unwrap();
        assert!(device.is_some());
        assert_eq!(device.unwrap().public_key, public_key);
    }

    #[tokio::test]
    async fn register_garage() {
        let registry = create_registry();
        let garage_id = "test-garage".to_string();
        let public_key = generate_public_key();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();

        let registration = GarageRegistration {
            garage_id: garage_id.clone(),
            public_key: public_key.clone(),
            endpoints: vec![endpoint],
        };

        let garage = registry.register_garage(registration).await.unwrap();

        assert_eq!(garage.garage_id, garage_id);
        assert_eq!(garage.public_key, public_key);
        assert!(garage.overlay_ip.is_garage());
        assert_eq!(garage.endpoints, vec![endpoint]);
    }

    #[tokio::test]
    async fn garage_ip_is_deterministic() {
        let registry = create_registry();
        let garage_id = "test-garage".to_string();

        let registration1 = GarageRegistration {
            garage_id: garage_id.clone(),
            public_key: generate_public_key(),
            endpoints: vec![],
        };
        let garage1 = registry.register_garage(registration1).await.unwrap();

        // Different key, same garage ID
        let registration2 = GarageRegistration {
            garage_id: garage_id.clone(),
            public_key: generate_public_key(),
            endpoints: vec![],
        };
        let garage2 = registry.register_garage(registration2).await.unwrap();

        // Same IP (deterministic from garage ID)
        assert_eq!(garage1.overlay_ip, garage2.overlay_ip);
    }

    #[tokio::test]
    async fn get_garage() {
        let registry = create_registry();
        let garage_id = "test-garage";

        // Not registered yet
        assert!(registry.get_garage(garage_id).unwrap().is_none());

        // Register
        let registration = GarageRegistration {
            garage_id: garage_id.to_string(),
            public_key: generate_public_key(),
            endpoints: vec![],
        };
        registry.register_garage(registration).await.unwrap();

        // Now found
        let garage = registry.get_garage(garage_id).unwrap();
        assert!(garage.is_some());
        assert_eq!(garage.unwrap().garage_id, garage_id);
    }

    #[tokio::test]
    async fn unregister_garage() {
        let registry = create_registry();
        let garage_id = "test-garage";

        // Register
        let registration = GarageRegistration {
            garage_id: garage_id.to_string(),
            public_key: generate_public_key(),
            endpoints: vec![],
        };
        registry.register_garage(registration).await.unwrap();

        // Unregister
        let removed = registry.unregister_garage(garage_id).unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().garage_id, garage_id);

        // No longer found
        assert!(registry.get_garage(garage_id).unwrap().is_none());

        // Unregister again is no-op
        let removed = registry.unregister_garage(garage_id).unwrap();
        assert!(removed.is_none());
    }

    #[tokio::test]
    async fn list_garages() {
        let registry = create_registry();

        // Register multiple garages
        for i in 0..3 {
            let registration = GarageRegistration {
                garage_id: format!("garage-{i}"),
                public_key: generate_public_key(),
                endpoints: vec![],
            };
            registry.register_garage(registration).await.unwrap();
        }

        let garages = registry.list_garages().unwrap();
        assert_eq!(garages.len(), 3);
    }

    #[test]
    fn device_registration_serde() {
        let registration = DeviceRegistration {
            public_key: generate_public_key(),
            device_name: Some("test".to_string()),
        };

        let json = serde_json::to_string(&registration).unwrap();
        let parsed: DeviceRegistration = serde_json::from_str(&json).unwrap();

        assert_eq!(registration.public_key, parsed.public_key);
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
