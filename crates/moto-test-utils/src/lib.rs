//! Shared test utilities for moto integration tests.
//!
//! This crate provides test helpers for integration tests that require
//! a `PostgreSQL` database. It connects to the test database on port 5433.
//!
//! # Example
//!
//! ```ignore
//! use moto_test_utils::{test_pool, unique_garage_name, unique_owner, fake_wg_pubkey};
//!
//! #[tokio::test]
//! async fn test_garage_creation() {
//!     let pool = test_pool().await;
//!     let name = unique_garage_name();
//!     let owner = unique_owner();
//!     let pubkey = fake_wg_pubkey();
//!     // ... use pool, name, owner, pubkey in tests
//! }
//! ```
//!
//! # Test Isolation
//!
//! Tests run in parallel across crates. Isolation comes from unique identifiers
//! (UUIDs), not separate databases or transactions. All `unique_*()` functions
//! generate values that are guaranteed not to collide.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::RngCore;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

/// Test database URL.
const TEST_DATABASE_URL: &str = "postgres://moto_test:moto_test@localhost:5433/moto_test";

/// Returns a connection to the test database (port 5433).
///
/// Creates a fresh pool for each call. While this means each test
/// has its own pool, `PostgreSQL` handles connection pooling efficiently
/// and this avoids cross-runtime issues with `#[tokio::test]`.
///
/// # Panics
///
/// Panics with a helpful message if:
/// - The test database is not running
/// - `TEST_DATABASE_URL` environment variable is set but invalid
///
/// # Example
///
/// ```ignore
/// let pool = test_pool().await;
/// let result = sqlx::query("SELECT 1").fetch_one(&pool).await;
/// ```
pub async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| TEST_DATABASE_URL.to_string());

    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&url)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "\n\nFailed to connect to test database.\n\n\
                Connection URL: {url}\n\
                Error: {e}\n\n\
                Make sure the test database is running:\n\
                \n\
                    make test-db-up\n\
                    make test-db-migrate\n\n"
            );
        })
}

/// Generates a unique garage name for test isolation.
///
/// Returns a name in the format `test-garage-{uuid}` where uuid is a
/// randomly generated `UUIDv7`.
///
/// # Example
///
/// ```
/// use moto_test_utils::unique_garage_name;
///
/// let name = unique_garage_name();
/// assert!(name.starts_with("test-garage-"));
/// ```
#[must_use]
pub fn unique_garage_name() -> String {
    format!("test-garage-{}", Uuid::now_v7())
}

/// Generates a unique owner name for test isolation.
///
/// Returns a name in the format `test-owner-{uuid}` where uuid is a
/// randomly generated `UUIDv7`.
///
/// # Example
///
/// ```
/// use moto_test_utils::unique_owner;
///
/// let owner = unique_owner();
/// assert!(owner.starts_with("test-owner-"));
/// ```
#[must_use]
pub fn unique_owner() -> String {
    format!("test-owner-{}", Uuid::now_v7())
}

/// Generates a fake `WireGuard` public key for testing.
///
/// Returns a base64-encoded 32-byte key that looks like a real
/// `WireGuard` public key but is randomly generated.
///
/// # Example
///
/// ```
/// use moto_test_utils::fake_wg_pubkey;
///
/// let pubkey = fake_wg_pubkey();
/// // Base64-encoded 32 bytes = 44 characters (with padding)
/// assert_eq!(pubkey.len(), 44);
/// assert!(pubkey.ends_with("="));
/// ```
#[must_use]
pub fn fake_wg_pubkey() -> String {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    STANDARD.encode(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_garage_name_format() {
        let name = unique_garage_name();
        assert!(name.starts_with("test-garage-"));
        // UUIDv7 is 36 chars, prefix is 12, total = 48
        assert_eq!(name.len(), 48);
    }

    #[test]
    fn unique_garage_names_are_unique() {
        let name1 = unique_garage_name();
        let name2 = unique_garage_name();
        assert_ne!(name1, name2);
    }

    #[test]
    fn unique_owner_format() {
        let owner = unique_owner();
        assert!(owner.starts_with("test-owner-"));
        assert_eq!(owner.len(), 47);
    }

    #[test]
    fn unique_owners_are_unique() {
        let owner1 = unique_owner();
        let owner2 = unique_owner();
        assert_ne!(owner1, owner2);
    }

    #[test]
    fn fake_wg_pubkey_format() {
        let pubkey = fake_wg_pubkey();
        // Base64 encoding of 32 bytes = ceil(32*8/6) = 43 chars + 1 padding = 44
        assert_eq!(pubkey.len(), 44);
        assert!(pubkey.ends_with('='));

        // Verify it's valid base64
        let decoded = STANDARD.decode(&pubkey).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn fake_wg_pubkeys_are_unique() {
        let key1 = fake_wg_pubkey();
        let key2 = fake_wg_pubkey();
        assert_ne!(key1, key2);
    }
}
