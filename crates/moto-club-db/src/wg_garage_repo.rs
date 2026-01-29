//! `WireGuard` garage repository for database operations.
//!
//! Provides CRUD operations for the `wg_garages` table.
//! Garage `WireGuard` registrations are created when a garage pod starts
//! and registers its `WireGuard` endpoint with moto-club.

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{DbError, DbPool, DbResult, WgGarage};

/// Input for registering/updating a garage's `WireGuard` endpoint.
#[derive(Debug, Clone)]
pub struct RegisterWgGarage {
    /// Garage ID (FK to garages table).
    pub garage_id: Uuid,
    /// Garage's `WireGuard` public key.
    pub public_key: String,
    /// Pod's reachable endpoints (e.g., `["10.42.0.5:51820"]`).
    pub endpoints: Vec<String>,
}

/// Get a garage `WireGuard` registration by garage ID.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage hasn't registered yet.
pub async fn get_by_garage_id(pool: &DbPool, garage_id: Uuid) -> DbResult<WgGarage> {
    sqlx::query_as::<_, WgGarage>("SELECT * FROM wg_garages WHERE garage_id = $1")
        .bind(garage_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "wg_garage",
            id: garage_id.to_string(),
        })
}

/// Check if a garage has registered its `WireGuard` endpoint.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn exists(pool: &DbPool, garage_id: Uuid) -> DbResult<bool> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM wg_garages WHERE garage_id = $1")
        .bind(garage_id)
        .fetch_one(pool)
        .await?;

    Ok(count.0 > 0)
}

/// Calculate the deterministic garage IP from the garage ID.
///
/// Per spec: Garage IPs use first 8 bytes of `SHA256(garage_id)` as host part.
/// Subnet: `fd00:moto:1::/64`
///
/// The same garage always gets the same IP, even if re-registered.
fn calculate_garage_ip(garage_id: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(garage_id.as_bytes());
    let hash = hasher.finalize();

    // Take first 8 bytes of hash for the host part
    // IPv6 /64 means we have 64 bits for the host part
    let host_bytes: [u8; 8] = hash[..8].try_into().expect("hash is at least 8 bytes");
    let host_part = u64::from_be_bytes(host_bytes);

    // Format as IPv6 in fd00:moto:1:: subnet
    // fd00:moto:1::{64-bit host}
    // The 64-bit host is split into 4 groups of 16 bits
    let a = (host_part >> 48) as u16;
    let b = ((host_part >> 32) & 0xFFFF) as u16;
    let c = ((host_part >> 16) & 0xFFFF) as u16;
    let d = (host_part & 0xFFFF) as u16;

    format!("fd00:moto:1::{a:x}:{b:x}:{c:x}:{d:x}")
}

/// Register a garage's `WireGuard` endpoint (upsert).
///
/// If the garage already has a registration, this updates the `public_key` and endpoints.
/// The `assigned_ip` is deterministic (derived from `garage_id` hash) and stays the same.
/// `peer_version` is preserved on update.
///
/// Per spec: Re-registration with different pubkey is allowed because namespace
/// validation ensures only pods in the correct namespace can register.
///
/// # Errors
///
/// Returns a database error if the garage FK constraint fails (garage doesn't exist).
pub async fn register(pool: &DbPool, input: RegisterWgGarage) -> DbResult<WgGarage> {
    let assigned_ip = calculate_garage_ip(input.garage_id);

    let garage = sqlx::query_as::<_, WgGarage>(
        r"
        INSERT INTO wg_garages (garage_id, public_key, assigned_ip, endpoints)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (garage_id) DO UPDATE SET
            public_key = EXCLUDED.public_key,
            endpoints = EXCLUDED.endpoints,
            registered_at = now()
        RETURNING *
        ",
    )
    .bind(input.garage_id)
    .bind(&input.public_key)
    .bind(&assigned_ip)
    .bind(&input.endpoints)
    .fetch_one(pool)
    .await?;

    Ok(garage)
}

/// Update a garage's endpoints.
///
/// Used when the pod's network endpoints change (e.g., after pod restart).
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage hasn't registered yet.
pub async fn update_endpoints(
    pool: &DbPool,
    garage_id: Uuid,
    endpoints: &[String],
) -> DbResult<WgGarage> {
    let garage = sqlx::query_as::<_, WgGarage>(
        r"
        UPDATE wg_garages
        SET endpoints = $2
        WHERE garage_id = $1
        RETURNING *
        ",
    )
    .bind(garage_id)
    .bind(endpoints)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "wg_garage",
        id: garage_id.to_string(),
    })?;

    Ok(garage)
}

/// Increment the `peer_version` for a garage.
///
/// Called when sessions are created or closed. Garages poll for peers
/// and use the version to detect changes (conditional GET with 304).
///
/// Returns the new `peer_version`.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage hasn't registered yet.
pub async fn increment_peer_version(pool: &DbPool, garage_id: Uuid) -> DbResult<i32> {
    let result: Option<(i32,)> = sqlx::query_as(
        r"
        UPDATE wg_garages
        SET peer_version = peer_version + 1
        WHERE garage_id = $1
        RETURNING peer_version
        ",
    )
    .bind(garage_id)
    .fetch_optional(pool)
    .await?;

    match result {
        Some((version,)) => Ok(version),
        None => Err(DbError::NotFound {
            entity: "wg_garage",
            id: garage_id.to_string(),
        }),
    }
}

/// Get the current `peer_version` for a garage.
///
/// Used for conditional GET requests (`?version=N` param).
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage hasn't registered yet.
pub async fn get_peer_version(pool: &DbPool, garage_id: Uuid) -> DbResult<i32> {
    let result: Option<(i32,)> =
        sqlx::query_as("SELECT peer_version FROM wg_garages WHERE garage_id = $1")
            .bind(garage_id)
            .fetch_optional(pool)
            .await?;

    match result {
        Some((version,)) => Ok(version),
        None => Err(DbError::NotFound {
            entity: "wg_garage",
            id: garage_id.to_string(),
        }),
    }
}

/// Delete a garage `WireGuard` registration.
///
/// This is typically not called directly - registrations are deleted via
/// ON DELETE CASCADE when the garage is deleted.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage hasn't registered yet.
pub async fn delete(pool: &DbPool, garage_id: Uuid) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM wg_garages WHERE garage_id = $1")
        .bind(garage_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "wg_garage",
            id: garage_id.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_wg_garage_input() {
        let garage_id = Uuid::now_v7();
        let input = RegisterWgGarage {
            garage_id,
            public_key: "base64-garage-wg-public-key".to_string(),
            endpoints: vec!["10.42.0.5:51820".to_string()],
        };

        assert_eq!(input.garage_id, garage_id);
        assert_eq!(input.public_key, "base64-garage-wg-public-key");
        assert_eq!(input.endpoints, vec!["10.42.0.5:51820"]);
    }

    #[test]
    fn calculate_garage_ip_deterministic() {
        // Same garage_id should always produce the same IP
        let garage_id = Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap();

        let ip1 = calculate_garage_ip(garage_id);
        let ip2 = calculate_garage_ip(garage_id);

        assert_eq!(ip1, ip2);
        assert!(ip1.starts_with("fd00:moto:1::"));
    }

    #[test]
    fn calculate_garage_ip_different_ids() {
        // Different garage_ids should produce different IPs (with high probability)
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let ip1 = calculate_garage_ip(id1);
        let ip2 = calculate_garage_ip(id2);

        assert_ne!(ip1, ip2);
    }

    #[test]
    fn calculate_garage_ip_format() {
        let garage_id = Uuid::now_v7();
        let ip = calculate_garage_ip(garage_id);

        // Should be a valid IPv6 in our subnet
        assert!(ip.starts_with("fd00:moto:1::"));

        // Should have the format fd00:moto:1::xxxx:xxxx:xxxx:xxxx
        let parts: Vec<&str> = ip.split("::").collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "fd00:moto:1");

        // Host part should have 4 groups separated by colons
        let host_parts: Vec<&str> = parts[1].split(':').collect();
        assert_eq!(host_parts.len(), 4);
    }
}
