//! IP address allocation for the `WireGuard` overlay network.
//!
//! This module provides IP address management (IPAM) for both garages and clients:
//!
//! - **Garages:** IP derived deterministically from garage ID (hash-based)
//! - **Clients:** IP allocated sequentially and persisted per device (keyed by public key)
//!
//! # Architecture
//!
//! The IPAM is designed to be backed by a database for persistence. The [`IpamStore`]
//! trait defines the storage interface, allowing different backends (in-memory for tests,
//! Postgres for production).
//!
//! The WireGuard public key IS the device identity (Cloudflare WARP model).
//!
//! # Example
//!
//! ```
//! use moto_club_wg::ipam::{Ipam, InMemoryStore};
//! use moto_wgtunnel_types::WgPrivateKey;
//!
//! # tokio_test::block_on(async {
//! let store = InMemoryStore::new();
//! let ipam = Ipam::new(store);
//!
//! // Allocate IP for a garage (deterministic)
//! let garage_id = "my-garage";
//! let garage_ip = ipam.allocate_garage(garage_id).await.unwrap();
//!
//! // Same garage ID always gets same IP
//! let garage_ip2 = ipam.allocate_garage(garage_id).await.unwrap();
//! assert_eq!(garage_ip, garage_ip2);
//!
//! // Allocate IP for a client device (keyed by public key)
//! let device_key = WgPrivateKey::generate().public_key();
//! let client_ip = ipam.allocate_client(&device_key).await.unwrap();
//!
//! // Same public key always gets same IP
//! let client_ip2 = ipam.allocate_client(&device_key).await.unwrap();
//! assert_eq!(client_ip, client_ip2);
//! # });
//! ```

use moto_wgtunnel_types::{OverlayIp, WgPublicKey};
use std::collections::HashMap;
use std::sync::Mutex;

/// Error type for IPAM operations.
#[derive(Debug, thiserror::Error)]
pub enum IpamError {
    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// IP address pool exhausted.
    #[error("IP address pool exhausted for {pool}")]
    Exhausted {
        /// Name of the exhausted pool.
        pool: &'static str,
    },
}

/// Result type for IPAM operations.
pub type Result<T> = std::result::Result<T, IpamError>;

/// Storage backend for IPAM.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait IpamStore: Send + Sync {
    /// Get the IP allocated to a client device by public key, if any.
    ///
    /// The WireGuard public key IS the device identity (Cloudflare WARP model).
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_client_ip(&self, public_key: &WgPublicKey) -> Result<Option<OverlayIp>>;

    /// Store a client device IP allocation.
    ///
    /// The WireGuard public key IS the device identity.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_client_ip(&self, public_key: &WgPublicKey, ip: OverlayIp) -> Result<()>;

    /// Get the next available client host ID for allocation.
    ///
    /// This should return a value that hasn't been allocated yet.
    /// The store is responsible for ensuring uniqueness.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn next_client_host_id(&self) -> Result<u64>;
}

/// IP address manager for the `WireGuard` overlay network.
///
/// Handles IP allocation for both garages (deterministic) and clients (sequential).
pub struct Ipam<S> {
    store: S,
}

impl<S: IpamStore> Ipam<S> {
    /// Create a new IPAM with the given storage backend.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Allocate an overlay IP for a garage.
    ///
    /// The IP is derived deterministically from the garage ID using a hash.
    /// Calling this multiple times with the same garage ID returns the same IP.
    ///
    /// # Errors
    ///
    /// This function is infallible for valid garage IDs.
    #[allow(clippy::unused_async)] // Async for API consistency with clients
    pub async fn allocate_garage(&self, garage_id: &str) -> Result<OverlayIp> {
        let host_id = hash_garage_id(garage_id);
        Ok(OverlayIp::garage(host_id))
    }

    /// Allocate an overlay IP for a client device.
    ///
    /// The WireGuard public key IS the device identity (Cloudflare WARP model).
    /// If the device (public key) already has an allocated IP, returns the existing one.
    /// Otherwise, allocates a new IP and persists it.
    ///
    /// # Errors
    ///
    /// Returns error if storage operations fail or the IP pool is exhausted.
    #[allow(clippy::unused_async)] // Async for future database operations
    pub async fn allocate_client(&self, public_key: &WgPublicKey) -> Result<OverlayIp> {
        // Check if device already has an IP
        if let Some(ip) = self.store.get_client_ip(public_key)? {
            return Ok(ip);
        }

        // Allocate new IP
        let host_id = self.store.next_client_host_id()?;

        // Host ID 0 is reserved (network address)
        let host_id = if host_id == 0 { 1 } else { host_id };

        let ip = OverlayIp::client(host_id);
        self.store.set_client_ip(public_key, ip)?;

        Ok(ip)
    }

    /// Get the IP allocated to a client device without allocating.
    ///
    /// The WireGuard public key IS the device identity.
    /// Returns `None` if the device has no allocated IP.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    #[allow(clippy::unused_async)] // Async for future database operations
    pub async fn get_client_ip(&self, public_key: &WgPublicKey) -> Result<Option<OverlayIp>> {
        self.store.get_client_ip(public_key)
    }
}

/// Hash a garage ID to a host identifier for IP allocation.
///
/// Uses a simple hash to derive a deterministic host ID from the garage ID.
/// The same garage ID always produces the same host ID.
fn hash_garage_id(garage_id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    // Use a stable hasher for deterministic results
    let mut hasher = StableHasher::new();
    garage_id.hash(&mut hasher);
    let hash = hasher.finish();

    // Use lower 48 bits to fit in IPv6 host portion
    // Avoid 0 (network address) by ensuring at least 1
    let host_id = hash & 0x0000_FFFF_FFFF_FFFF;
    if host_id == 0 { 1 } else { host_id }
}

/// A simple stable hasher that produces consistent results.
///
/// Note: This is intentionally not cryptographic - it's just for
/// deterministic IP derivation, not security.
struct StableHasher {
    state: u64,
}

impl StableHasher {
    const fn new() -> Self {
        Self {
            state: 0x517c_c1b7_2722_0a95, // FNV-1a offset basis (64-bit)
        }
    }
}

impl std::hash::Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        // FNV-1a hash for stability across runs
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        for &byte in bytes {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// In-memory IPAM store for testing.
///
/// Allocations are lost when the store is dropped.
pub struct InMemoryStore {
    inner: Mutex<InMemoryStoreInner>,
}

struct InMemoryStoreInner {
    /// Device public key (base64) -> allocated IP
    /// WireGuard public key IS the device identity
    client_ips: HashMap<String, OverlayIp>,
    /// Next host ID to allocate
    next_host_id: u64,
}

impl InMemoryStore {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemoryStoreInner {
                client_ips: HashMap::new(),
                next_host_id: 1, // Start at 1, 0 is reserved
            }),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IpamStore for InMemoryStore {
    fn get_client_ip(&self, public_key: &WgPublicKey) -> Result<Option<OverlayIp>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.client_ips.get(&public_key.to_base64()).copied())
    }

    fn set_client_ip(&self, public_key: &WgPublicKey, ip: OverlayIp) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .client_ips
            .insert(public_key.to_base64(), ip);
        Ok(())
    }

    fn next_client_host_id(&self) -> Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        let host_id = inner.next_host_id;
        inner.next_host_id += 1;
        drop(inner);
        Ok(host_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[tokio::test]
    async fn garage_allocation_is_deterministic() {
        let store = InMemoryStore::new();
        let ipam = Ipam::new(store);

        let ip1 = ipam.allocate_garage("garage-1").await.unwrap();
        let ip2 = ipam.allocate_garage("garage-1").await.unwrap();
        let ip3 = ipam.allocate_garage("garage-2").await.unwrap();

        // Same garage ID = same IP
        assert_eq!(ip1, ip2);

        // Different garage ID = different IP
        assert_ne!(ip1, ip3);

        // All are garage IPs
        assert!(ip1.is_garage());
        assert!(ip3.is_garage());
    }

    #[tokio::test]
    async fn client_allocation_is_sequential() {
        let store = InMemoryStore::new();
        let ipam = Ipam::new(store);

        let key1 = generate_public_key();
        let key2 = generate_public_key();

        let ip1 = ipam.allocate_client(&key1).await.unwrap();
        let ip2 = ipam.allocate_client(&key2).await.unwrap();

        // Different devices (public keys) get different IPs
        assert_ne!(ip1, ip2);

        // All are client IPs
        assert!(ip1.is_client());
        assert!(ip2.is_client());
    }

    #[tokio::test]
    async fn client_allocation_is_persistent() {
        let store = InMemoryStore::new();
        let ipam = Ipam::new(store);

        let key = generate_public_key();

        let ip1 = ipam.allocate_client(&key).await.unwrap();
        let ip2 = ipam.allocate_client(&key).await.unwrap();

        // Same public key always gets same IP
        assert_eq!(ip1, ip2);
    }

    #[tokio::test]
    async fn get_client_ip_without_allocation() {
        let store = InMemoryStore::new();
        let ipam = Ipam::new(store);

        let key = generate_public_key();

        // No allocation yet
        let ip = ipam.get_client_ip(&key).await.unwrap();
        assert!(ip.is_none());

        // After allocation
        let allocated = ipam.allocate_client(&key).await.unwrap();
        let ip = ipam.get_client_ip(&key).await.unwrap();
        assert_eq!(ip, Some(allocated));
    }

    #[test]
    fn hash_garage_id_is_stable() {
        // These hashes must be stable across runs
        let hash1 = hash_garage_id("test-garage");
        let hash2 = hash_garage_id("test-garage");
        assert_eq!(hash1, hash2);

        // Different IDs produce different hashes
        let hash3 = hash_garage_id("other-garage");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn hash_garage_id_avoids_zero() {
        // Test many garage IDs to ensure none hash to 0
        for i in 0..1000 {
            let garage_id = format!("garage-{i}");
            let host_id = hash_garage_id(&garage_id);
            assert_ne!(host_id, 0, "garage ID '{garage_id}' hashed to 0");
        }
    }
}
