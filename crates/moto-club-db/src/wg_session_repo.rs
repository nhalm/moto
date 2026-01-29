//! `WireGuard` session repository for database operations.
//!
//! Provides CRUD operations for the `wg_sessions` table.
//! Sessions represent authorized tunnel connections from a client device to a garage.
//! Sessions have the `garage_id` FK with `ON DELETE CASCADE` to prevent orphaned records.

use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{DbError, DbPool, DbResult, WgSession};

/// Session with additional details from joined tables.
///
/// Used by `list_by_owner_with_details` to return enriched session data
/// including garage name and device name.
#[derive(Debug, Clone, FromRow)]
pub struct WgSessionWithDetails {
    /// Unique identifier.
    pub id: Uuid,
    /// Device public key this session belongs to.
    pub device_pubkey: String,
    /// Garage this session connects to.
    pub garage_id: Uuid,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// When the session was closed (if applicable).
    pub closed_at: Option<DateTime<Utc>>,
    /// Human-readable garage name.
    pub garage_name: String,
    /// Optional device name for display.
    pub device_name: Option<String>,
}

/// Input for creating a new `WireGuard` session.
#[derive(Debug, Clone)]
pub struct CreateWgSession {
    /// Device public key this session belongs to.
    pub device_pubkey: String,
    /// Garage this session connects to.
    pub garage_id: Uuid,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
}

/// Get a session by its ID.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the session doesn't exist.
pub async fn get_by_id(pool: &DbPool, id: Uuid) -> DbResult<WgSession> {
    sqlx::query_as::<_, WgSession>("SELECT * FROM wg_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "session",
            id: id.to_string(),
        })
}

/// Create a new `WireGuard` session.
///
/// # Errors
///
/// Returns a database error if the device or garage FK constraint fails.
pub async fn create(pool: &DbPool, input: CreateWgSession) -> DbResult<WgSession> {
    let session = sqlx::query_as::<_, WgSession>(
        r"
        INSERT INTO wg_sessions (device_pubkey, garage_id, expires_at)
        VALUES ($1, $2, $3)
        RETURNING *
        ",
    )
    .bind(&input.device_pubkey)
    .bind(input.garage_id)
    .bind(input.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(session)
}

/// List active sessions for a device.
///
/// Active sessions are those where `closed_at IS NULL` and `expires_at > now()`.
/// Returns sessions ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_active_by_device(pool: &DbPool, device_pubkey: &str) -> DbResult<Vec<WgSession>> {
    let sessions = sqlx::query_as::<_, WgSession>(
        r"
        SELECT * FROM wg_sessions
        WHERE device_pubkey = $1
          AND closed_at IS NULL
          AND expires_at > now()
        ORDER BY created_at DESC
        ",
    )
    .bind(device_pubkey)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// List active sessions for a garage.
///
/// Active sessions are those where `closed_at IS NULL` and `expires_at > now()`.
/// Returns sessions ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_active_by_garage(pool: &DbPool, garage_id: Uuid) -> DbResult<Vec<WgSession>> {
    let sessions = sqlx::query_as::<_, WgSession>(
        r"
        SELECT * FROM wg_sessions
        WHERE garage_id = $1
          AND closed_at IS NULL
          AND expires_at > now()
        ORDER BY created_at DESC
        ",
    )
    .bind(garage_id)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// List all sessions for a device (including expired/closed).
///
/// Returns sessions ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_all_by_device(pool: &DbPool, device_pubkey: &str) -> DbResult<Vec<WgSession>> {
    let sessions = sqlx::query_as::<_, WgSession>(
        r"
        SELECT * FROM wg_sessions
        WHERE device_pubkey = $1
        ORDER BY created_at DESC
        ",
    )
    .bind(device_pubkey)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// Filter options for listing sessions.
#[derive(Debug, Clone, Default)]
pub struct ListSessionsFilter {
    /// Filter by garage ID.
    pub garage_id: Option<Uuid>,
    /// Include expired/closed sessions (default: false).
    pub include_all: bool,
}

/// List sessions for an owner (via device ownership).
///
/// Sessions are filtered to those belonging to devices owned by the user.
/// By default, only active sessions (not expired, not closed) are returned.
/// Use `filter.include_all = true` to include expired/closed sessions.
///
/// Returns sessions ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_owner(
    pool: &DbPool,
    owner: &str,
    filter: ListSessionsFilter,
) -> DbResult<Vec<WgSession>> {
    let sessions = if filter.include_all {
        if let Some(garage_id) = filter.garage_id {
            sqlx::query_as::<_, WgSession>(
                r"
                SELECT s.* FROM wg_sessions s
                JOIN wg_devices d ON s.device_pubkey = d.public_key
                WHERE d.owner = $1 AND s.garage_id = $2
                ORDER BY s.created_at DESC
                ",
            )
            .bind(owner)
            .bind(garage_id)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, WgSession>(
                r"
                SELECT s.* FROM wg_sessions s
                JOIN wg_devices d ON s.device_pubkey = d.public_key
                WHERE d.owner = $1
                ORDER BY s.created_at DESC
                ",
            )
            .bind(owner)
            .fetch_all(pool)
            .await?
        }
    } else if let Some(garage_id) = filter.garage_id {
        sqlx::query_as::<_, WgSession>(
            r"
            SELECT s.* FROM wg_sessions s
            JOIN wg_devices d ON s.device_pubkey = d.public_key
            WHERE d.owner = $1
              AND s.garage_id = $2
              AND s.closed_at IS NULL
              AND s.expires_at > now()
            ORDER BY s.created_at DESC
            ",
        )
        .bind(owner)
        .bind(garage_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, WgSession>(
            r"
            SELECT s.* FROM wg_sessions s
            JOIN wg_devices d ON s.device_pubkey = d.public_key
            WHERE d.owner = $1
              AND s.closed_at IS NULL
              AND s.expires_at > now()
            ORDER BY s.created_at DESC
            ",
        )
        .bind(owner)
        .fetch_all(pool)
        .await?
    };

    Ok(sessions)
}

/// List sessions for an owner with enriched details (garage name, device name).
///
/// Sessions are filtered to those belonging to devices owned by the user.
/// By default, only active sessions (not expired, not closed) are returned.
/// Use `filter.include_all = true` to include expired/closed sessions.
///
/// Returns sessions ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_owner_with_details(
    pool: &DbPool,
    owner: &str,
    filter: ListSessionsFilter,
) -> DbResult<Vec<WgSessionWithDetails>> {
    let sessions = if filter.include_all {
        if let Some(garage_id) = filter.garage_id {
            sqlx::query_as::<_, WgSessionWithDetails>(
                r"
                SELECT s.id, s.device_pubkey, s.garage_id, s.expires_at, s.created_at, s.closed_at,
                       g.name AS garage_name, d.device_name
                FROM wg_sessions s
                JOIN wg_devices d ON s.device_pubkey = d.public_key
                JOIN garages g ON s.garage_id = g.id
                WHERE d.owner = $1 AND s.garage_id = $2
                ORDER BY s.created_at DESC
                ",
            )
            .bind(owner)
            .bind(garage_id)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, WgSessionWithDetails>(
                r"
                SELECT s.id, s.device_pubkey, s.garage_id, s.expires_at, s.created_at, s.closed_at,
                       g.name AS garage_name, d.device_name
                FROM wg_sessions s
                JOIN wg_devices d ON s.device_pubkey = d.public_key
                JOIN garages g ON s.garage_id = g.id
                WHERE d.owner = $1
                ORDER BY s.created_at DESC
                ",
            )
            .bind(owner)
            .fetch_all(pool)
            .await?
        }
    } else if let Some(garage_id) = filter.garage_id {
        sqlx::query_as::<_, WgSessionWithDetails>(
            r"
            SELECT s.id, s.device_pubkey, s.garage_id, s.expires_at, s.created_at, s.closed_at,
                   g.name AS garage_name, d.device_name
            FROM wg_sessions s
            JOIN wg_devices d ON s.device_pubkey = d.public_key
            JOIN garages g ON s.garage_id = g.id
            WHERE d.owner = $1
              AND s.garage_id = $2
              AND s.closed_at IS NULL
              AND s.expires_at > now()
            ORDER BY s.created_at DESC
            ",
        )
        .bind(owner)
        .bind(garage_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, WgSessionWithDetails>(
            r"
            SELECT s.id, s.device_pubkey, s.garage_id, s.expires_at, s.created_at, s.closed_at,
                   g.name AS garage_name, d.device_name
            FROM wg_sessions s
            JOIN wg_devices d ON s.device_pubkey = d.public_key
            JOIN garages g ON s.garage_id = g.id
            WHERE d.owner = $1
              AND s.closed_at IS NULL
              AND s.expires_at > now()
            ORDER BY s.created_at DESC
            ",
        )
        .bind(owner)
        .fetch_all(pool)
        .await?
    };

    Ok(sessions)
}

/// Close a session (soft delete).
///
/// Sets `closed_at` to the current time. The session record is preserved for audit.
/// This is idempotent: closing an already-closed session succeeds.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the session doesn't exist.
pub async fn close(pool: &DbPool, id: Uuid) -> DbResult<WgSession> {
    let session = sqlx::query_as::<_, WgSession>(
        r"
        UPDATE wg_sessions
        SET closed_at = COALESCE(closed_at, now())
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "session",
        id: id.to_string(),
    })?;

    Ok(session)
}

/// Close all sessions for a garage.
///
/// Sets `closed_at` to the current time for all sessions belonging to the garage.
/// Used when a garage is terminated.
///
/// Returns the number of sessions closed.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn close_all_for_garage(pool: &DbPool, garage_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query(
        r"
        UPDATE wg_sessions
        SET closed_at = COALESCE(closed_at, now())
        WHERE garage_id = $1 AND closed_at IS NULL
        ",
    )
    .bind(garage_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Check if the requesting user owns a session (via device ownership).
///
/// # Errors
///
/// Returns `DbError::NotFound` if the session doesn't exist.
/// Returns `DbError::NotOwned` if the session belongs to a different user.
pub async fn verify_ownership(pool: &DbPool, session_id: Uuid, owner: &str) -> DbResult<WgSession> {
    let session = get_by_id(pool, session_id).await?;

    // Check device ownership
    let device_owner: Option<(String,)> =
        sqlx::query_as("SELECT owner FROM wg_devices WHERE public_key = $1")
            .bind(&session.device_pubkey)
            .fetch_optional(pool)
            .await?;

    match device_owner {
        Some((device_owner,)) if device_owner == owner => Ok(session),
        Some(_) => Err(DbError::NotOwned {
            entity: "session",
            id: session_id.to_string(),
        }),
        None => {
            // Device was deleted but session remains (shouldn't happen with FK constraints)
            Err(DbError::NotFound {
                entity: "session",
                id: session_id.to_string(),
            })
        }
    }
}

/// Delete a session record (for testing/cleanup only).
///
/// In production, sessions are soft-deleted via `close()`.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the session doesn't exist.
pub async fn delete(pool: &DbPool, id: Uuid) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM wg_sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_wg_session_input() {
        let garage_id = Uuid::now_v7();
        let expires_at = Utc::now() + chrono::Duration::hours(4);

        let input = CreateWgSession {
            device_pubkey: "base64-wg-public-key".to_string(),
            garage_id,
            expires_at,
        };

        assert_eq!(input.device_pubkey, "base64-wg-public-key");
        assert_eq!(input.garage_id, garage_id);
        assert_eq!(input.expires_at, expires_at);
    }

    #[test]
    fn list_sessions_filter_default() {
        let filter = ListSessionsFilter::default();
        assert!(filter.garage_id.is_none());
        assert!(!filter.include_all);
    }

    #[test]
    fn list_sessions_filter_with_garage() {
        let garage_id = Uuid::now_v7();
        let filter = ListSessionsFilter {
            garage_id: Some(garage_id),
            include_all: false,
        };
        assert_eq!(filter.garage_id, Some(garage_id));
        assert!(!filter.include_all);
    }

    #[test]
    fn list_sessions_filter_include_all() {
        let filter = ListSessionsFilter {
            garage_id: None,
            include_all: true,
        };
        assert!(filter.garage_id.is_none());
        assert!(filter.include_all);
    }
}
