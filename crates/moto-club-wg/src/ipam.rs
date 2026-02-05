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
//! trait defines the storage interface. Use `PostgresIpamStore` from `moto-club-api`
//! for production.
//!
//! The `WireGuard` public key IS the device identity (Cloudflare WARP model).

use moto_wgtunnel_types::{OverlayIp, WgPublicKey};

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
    /// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_client_ip(&self, public_key: &WgPublicKey) -> Result<Option<OverlayIp>>;

    /// Store a client device IP allocation.
    ///
    /// The `WireGuard` public key IS the device identity.
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
    /// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
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
    /// The `WireGuard` public key IS the device identity.
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

// Unit tests for pure hash functions (no database needed)
#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn garage_ip_is_deterministic() {
        // Test that the same garage ID always produces the same host_id
        // This tests the pure function without needing an Ipam instance
        let host_id1 = hash_garage_id("garage-1");
        let host_id2 = hash_garage_id("garage-1");
        let host_id3 = hash_garage_id("garage-2");

        assert_eq!(host_id1, host_id2);
        assert_ne!(host_id1, host_id3);

        // Verify the IPs are in the garage subnet
        let ip1 = OverlayIp::garage(host_id1);
        let ip3 = OverlayIp::garage(host_id3);

        assert!(ip1.is_garage());
        assert!(ip3.is_garage());
        assert_ne!(ip1, ip3);
    }
}

// Integration tests that require PostgreSQL
// Run with: cargo test --features integration
#[cfg(all(test, feature = "integration"))]
mod integration_tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    fn generate_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    // Note: These tests require PostgresIpamStore from moto-club-api.
    // See moto-club-api/src/wg_test.rs for integration tests with PostgreSQL storage.
    // The IpamStore trait is tested through PostgresIpamStore in that crate.

    // The following tests document the expected behavior that integration tests should verify:
    //
    // 1. garage_allocation_is_deterministic:
    //    - Same garage ID returns same IP
    //    - Different garage IDs return different IPs
    //    - All allocated IPs are in the garage subnet
    //
    // 2. client_allocation_is_sequential:
    //    - Different public keys get different IPs
    //    - All allocated IPs are in the client subnet
    //
    // 3. client_allocation_is_persistent:
    //    - Same public key always returns the same IP
    //
    // 4. get_client_ip_without_allocation:
    //    - Returns None for unallocated public keys
    //    - Returns Some(ip) after allocation
}
