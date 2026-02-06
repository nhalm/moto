//! `WireGuard` device repository for database operations.
//!
//! Provides CRUD operations for the `wg_devices` table.
//! The `WireGuard` public key IS the device identity (Cloudflare WARP model).

use crate::{DbError, DbPool, DbResult, WgDevice};

/// Input for creating/registering a new `WireGuard` device.
#[derive(Debug, Clone)]
pub struct CreateWgDevice {
    /// `WireGuard` public key (device identity/primary key).
    pub public_key: String,
    /// Owner identifier.
    pub owner: String,
    /// Optional friendly name for the device.
    pub device_name: Option<String>,
    /// Assigned overlay IP address (`fd00:moto:2::xxx`).
    pub assigned_ip: String,
}

/// Get a device by its public key.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the device doesn't exist.
pub async fn get_by_public_key(pool: &DbPool, public_key: &str) -> DbResult<WgDevice> {
    sqlx::query_as::<_, WgDevice>("SELECT * FROM wg_devices WHERE public_key = $1")
        .bind(public_key)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "device",
            id: public_key.to_string(),
        })
}

/// Check if a device exists by its public key.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn exists(pool: &DbPool, public_key: &str) -> DbResult<bool> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM wg_devices WHERE public_key = $1")
        .bind(public_key)
        .fetch_one(pool)
        .await?;

    Ok(count.0 > 0)
}

/// Create a new `WireGuard` device.
///
/// # Errors
///
/// Returns `DbError::AlreadyExists` if a device with the same public key exists.
pub async fn create(pool: &DbPool, input: CreateWgDevice) -> DbResult<WgDevice> {
    let result = sqlx::query_as::<_, WgDevice>(
        r"
        INSERT INTO wg_devices (public_key, owner, device_name, assigned_ip)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        ",
    )
    .bind(&input.public_key)
    .bind(&input.owner)
    .bind(&input.device_name)
    .bind(&input.assigned_ip)
    .fetch_one(pool)
    .await;

    match result {
        Ok(device) => Ok(device),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(DbError::AlreadyExists {
                entity: "device",
                id: input.public_key,
            })
        }
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

/// Get or create a device (idempotent registration).
///
/// If the device already exists with the same public key:
/// - If same owner: returns the existing device
/// - If different owner: returns `DbError::NotOwned`
///
/// This implements the idempotent device registration behavior per the spec:
/// same key re-registering returns existing assignment (200 OK).
///
/// # Errors
///
/// Returns `DbError::NotOwned` if the public key is registered to a different user.
pub async fn get_or_create(pool: &DbPool, input: CreateWgDevice) -> DbResult<(WgDevice, bool)> {
    // Try to get existing device first
    match get_by_public_key(pool, &input.public_key).await {
        Ok(existing) => {
            if existing.owner != input.owner {
                return Err(DbError::NotOwned {
                    entity: "device",
                    id: input.public_key,
                });
            }
            // Same owner, return existing device (created = false)
            Ok((existing, false))
        }
        Err(DbError::NotFound { .. }) => {
            // Device doesn't exist, create it
            let device = create(pool, input).await?;
            Ok((device, true))
        }
        Err(e) => Err(e),
    }
}

/// List all devices for an owner.
///
/// Returns devices ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_owner(pool: &DbPool, owner: &str) -> DbResult<Vec<WgDevice>> {
    let devices = sqlx::query_as::<_, WgDevice>(
        "SELECT * FROM wg_devices WHERE owner = $1 ORDER BY created_at DESC",
    )
    .bind(owner)
    .fetch_all(pool)
    .await?;

    Ok(devices)
}

/// Get the next available IP address for client devices.
///
/// Client IPs are allocated sequentially from `fd00:moto:2::1` onward.
/// Returns the next available IP based on the highest currently allocated.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn get_next_client_ip(pool: &DbPool) -> DbResult<String> {
    // Get the maximum assigned IP suffix
    // IPs are in format fd00:moto:2::N where N is the host part
    let result: Option<(String,)> = sqlx::query_as(
        r"
        SELECT assigned_ip FROM wg_devices
        ORDER BY created_at DESC
        LIMIT 1
        ",
    )
    .fetch_optional(pool)
    .await?;

    let next_suffix = match result {
        Some((last_ip,)) => {
            // Extract the suffix from fd00:moto:2::N
            last_ip
                .strip_prefix("fd00:moto:2::")
                .map_or(1, |suffix_str| {
                    // Parse as hex (IPv6 addresses use hex)
                    let suffix = u64::from_str_radix(suffix_str, 16).unwrap_or(0);
                    suffix + 1
                })
        }
        None => 1, // First device
    };

    Ok(format!("fd00:moto:2::{next_suffix:x}"))
}

/// Delete a device record (for testing/cleanup only).
///
/// In production, devices are typically not deleted.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the device doesn't exist.
pub async fn delete(pool: &DbPool, public_key: &str) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM wg_devices WHERE public_key = $1")
        .bind(public_key)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "device",
            id: public_key.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_wg_device_input() {
        let input = CreateWgDevice {
            public_key: "base64-wg-public-key".to_string(),
            owner: "nick".to_string(),
            device_name: Some("macbook-pro".to_string()),
            assigned_ip: "fd00:moto:2::1".to_string(),
        };

        assert_eq!(input.public_key, "base64-wg-public-key");
        assert_eq!(input.owner, "nick");
        assert_eq!(input.device_name, Some("macbook-pro".to_string()));
        assert_eq!(input.assigned_ip, "fd00:moto:2::1");
    }

    #[test]
    fn create_wg_device_input_no_name() {
        let input = CreateWgDevice {
            public_key: "base64-wg-public-key".to_string(),
            owner: "nick".to_string(),
            device_name: None,
            assigned_ip: "fd00:moto:2::2".to_string(),
        };

        assert_eq!(input.device_name, None);
    }
}

#[cfg(test)]
#[path = "wg_device_repo_test.rs"]
mod wg_device_repo_test;
