//! DERP server repository for database operations.
//!
//! Provides CRUD operations for the `derp_servers` table.
//! Used for syncing DERP configuration from config file to database.

use crate::{DbPool, DbResult, DerpServer};
use uuid::Uuid;

/// Input for creating/upserting a DERP server.
#[derive(Debug, Clone)]
pub struct UpsertDerpServer {
    /// Region ID.
    pub region_id: i32,
    /// Region name.
    pub region_name: String,
    /// Server hostname.
    pub host: String,
    /// DERP port.
    pub port: i32,
    /// STUN port.
    pub stun_port: i32,
}

/// Get all DERP servers.
///
/// Returns servers ordered by `region_id`, then by host.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_all(pool: &DbPool) -> DbResult<Vec<DerpServer>> {
    let servers =
        sqlx::query_as::<_, DerpServer>("SELECT * FROM derp_servers ORDER BY region_id, host")
            .fetch_all(pool)
            .await?;

    Ok(servers)
}

/// Get healthy DERP servers only.
///
/// Returns healthy servers ordered by `region_id`, then by host.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_healthy(pool: &DbPool) -> DbResult<Vec<DerpServer>> {
    let servers = sqlx::query_as::<_, DerpServer>(
        "SELECT * FROM derp_servers WHERE healthy = true ORDER BY region_id, host",
    )
    .fetch_all(pool)
    .await?;

    Ok(servers)
}

/// Insert or update a DERP server.
///
/// Upserts based on (`region_id`, host) - if a server with the same region and host
/// exists, it will be updated. Otherwise, a new record is created.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn upsert(pool: &DbPool, input: UpsertDerpServer) -> DbResult<DerpServer> {
    let server = sqlx::query_as::<_, DerpServer>(
        r"
        INSERT INTO derp_servers (id, region_id, region_name, host, port, stun_port, healthy, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, true, now())
        ON CONFLICT (region_id, host) DO UPDATE SET
            region_name = EXCLUDED.region_name,
            port = EXCLUDED.port,
            stun_port = EXCLUDED.stun_port
        RETURNING *
        ",
    )
    .bind(Uuid::now_v7())
    .bind(input.region_id)
    .bind(&input.region_name)
    .bind(&input.host)
    .bind(input.port)
    .bind(input.stun_port)
    .fetch_one(pool)
    .await?;

    Ok(server)
}

/// Delete a DERP server by `region_id` and host.
///
/// # Returns
///
/// Returns `true` if a server was deleted, `false` if it didn't exist.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn delete_by_region_and_host(
    pool: &DbPool,
    region_id: i32,
    host: &str,
) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM derp_servers WHERE region_id = $1 AND host = $2")
        .bind(region_id)
        .bind(host)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete all DERP servers in a region.
///
/// # Returns
///
/// Returns the number of servers deleted.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn delete_by_region(pool: &DbPool, region_id: i32) -> DbResult<u64> {
    let result = sqlx::query("DELETE FROM derp_servers WHERE region_id = $1")
        .bind(region_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

/// Delete all DERP servers.
///
/// # Returns
///
/// Returns the number of servers deleted.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn delete_all(pool: &DbPool) -> DbResult<u64> {
    let result = sqlx::query("DELETE FROM derp_servers")
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

/// Sync DERP servers from config.
///
/// This performs a full sync: inserts/updates servers from the config,
/// and deletes any servers not present in the config.
///
/// The config is expected to provide `(region_id, host)` as the unique key
/// for each server.
///
/// # Returns
///
/// Returns a tuple of (inserted, updated, deleted) counts.
///
/// # Errors
///
/// Returns a database error if any query fails.
pub async fn sync_from_config(
    pool: &DbPool,
    servers: Vec<UpsertDerpServer>,
) -> DbResult<SyncResult> {
    // Get existing servers
    let existing = list_all(pool).await?;
    let existing_keys: std::collections::HashSet<(i32, String)> = existing
        .iter()
        .map(|s| (s.region_id, s.host.clone()))
        .collect();

    // Track which servers are in the config
    let mut config_keys: std::collections::HashSet<(i32, String)> =
        std::collections::HashSet::new();
    let mut inserted = 0u64;
    let mut updated = 0u64;

    // Upsert all servers from config
    for server in servers {
        let key = (server.region_id, server.host.clone());
        config_keys.insert(key.clone());

        let is_new = !existing_keys.contains(&key);
        upsert(pool, server).await?;

        if is_new {
            inserted += 1;
        } else {
            updated += 1;
        }
    }

    // Delete servers not in config
    let mut deleted = 0u64;
    for (region_id, host) in existing_keys {
        if !config_keys.contains(&(region_id, host.clone()))
            && delete_by_region_and_host(pool, region_id, &host).await?
        {
            deleted += 1;
        }
    }

    Ok(SyncResult {
        inserted,
        updated,
        deleted,
    })
}

/// Result of a sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncResult {
    /// Number of new servers inserted.
    pub inserted: u64,
    /// Number of existing servers updated.
    pub updated: u64,
    /// Number of servers deleted (not in config).
    pub deleted: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_derp_server_input() {
        let input = UpsertDerpServer {
            region_id: 1,
            region_name: "primary".to_string(),
            host: "derp.example.com".to_string(),
            port: 443,
            stun_port: 3478,
        };

        assert_eq!(input.region_id, 1);
        assert_eq!(input.region_name, "primary");
        assert_eq!(input.host, "derp.example.com");
        assert_eq!(input.port, 443);
        assert_eq!(input.stun_port, 3478);
    }

    #[test]
    fn sync_result_display() {
        let result = SyncResult {
            inserted: 2,
            updated: 1,
            deleted: 3,
        };

        assert_eq!(result.inserted, 2);
        assert_eq!(result.updated, 1);
        assert_eq!(result.deleted, 3);
    }
}
