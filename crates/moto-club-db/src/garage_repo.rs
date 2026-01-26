//! Garage repository for database operations.
//!
//! Provides CRUD operations for the `garages` table.

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::{DbError, DbPool, DbResult, Garage, GarageStatus, TerminationReason};

/// Input for creating a new garage.
#[derive(Debug, Clone)]
pub struct CreateGarage {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// Human-friendly name (unique, immutable).
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Git branch being worked on.
    pub branch: String,
    /// Time-to-live in seconds.
    pub ttl_seconds: i32,
    /// Kubernetes namespace name.
    pub namespace: String,
    /// Kubernetes pod name.
    pub pod_name: String,
}

/// Get a garage by its ID.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn get_by_id(pool: &DbPool, id: Uuid) -> DbResult<Garage> {
    sqlx::query_as::<_, Garage>("SELECT * FROM garages WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "garage",
            id: id.to_string(),
        })
}

/// Get a garage by its name.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn get_by_name(pool: &DbPool, name: &str) -> DbResult<Garage> {
    sqlx::query_as::<_, Garage>("SELECT * FROM garages WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "garage",
            id: name.to_string(),
        })
}

/// Create a new garage.
///
/// The garage is created with `Pending` status and `expires_at` calculated
/// from the current time plus `ttl_seconds`.
///
/// # Errors
///
/// Returns `DbError::AlreadyExists` if a garage with the same name exists.
pub async fn create(pool: &DbPool, input: CreateGarage) -> DbResult<Garage> {
    let now = Utc::now();
    let expires_at = now + Duration::seconds(i64::from(input.ttl_seconds));

    let result = sqlx::query_as::<_, Garage>(
        r"
        INSERT INTO garages (id, name, owner, branch, status, ttl_seconds, expires_at, namespace, pod_name, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING *
        ",
    )
    .bind(input.id)
    .bind(&input.name)
    .bind(&input.owner)
    .bind(&input.branch)
    .bind(GarageStatus::Pending)
    .bind(input.ttl_seconds)
    .bind(expires_at)
    .bind(&input.namespace)
    .bind(&input.pod_name)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await;

    match result {
        Ok(garage) => Ok(garage),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(DbError::AlreadyExists {
                entity: "garage",
                id: input.name,
            })
        }
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

/// List garages for an owner.
///
/// Returns garages ordered by `created_at` descending (newest first).
/// If `include_terminated` is false, terminated garages are excluded.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_owner(
    pool: &DbPool,
    owner: &str,
    include_terminated: bool,
) -> DbResult<Vec<Garage>> {
    let garages = if include_terminated {
        sqlx::query_as::<_, Garage>(
            "SELECT * FROM garages WHERE owner = $1 ORDER BY created_at DESC",
        )
        .bind(owner)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Garage>(
            "SELECT * FROM garages WHERE owner = $1 AND status != $2 ORDER BY created_at DESC",
        )
        .bind(owner)
        .bind(GarageStatus::Terminated)
        .fetch_all(pool)
        .await?
    };

    Ok(garages)
}

/// List all garages (for reconciliation).
///
/// Returns garages ordered by `created_at` descending.
/// If `include_terminated` is false, terminated garages are excluded.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_all(pool: &DbPool, include_terminated: bool) -> DbResult<Vec<Garage>> {
    let garages = if include_terminated {
        sqlx::query_as::<_, Garage>("SELECT * FROM garages ORDER BY created_at DESC")
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_as::<_, Garage>(
            "SELECT * FROM garages WHERE status != $1 ORDER BY created_at DESC",
        )
        .bind(GarageStatus::Terminated)
        .fetch_all(pool)
        .await?
    };

    Ok(garages)
}

/// List garages by status.
///
/// Returns garages with the given status, ordered by `created_at` descending.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_status(pool: &DbPool, status: GarageStatus) -> DbResult<Vec<Garage>> {
    let garages = sqlx::query_as::<_, Garage>(
        "SELECT * FROM garages WHERE status = $1 ORDER BY created_at DESC",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;

    Ok(garages)
}

/// List garages that have expired but are not yet terminated.
///
/// Returns garages where `expires_at < now` and `status != terminated`.
/// Used by TTL enforcement (moto-cron).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_expired(pool: &DbPool) -> DbResult<Vec<Garage>> {
    let now = Utc::now();
    let garages = sqlx::query_as::<_, Garage>(
        "SELECT * FROM garages WHERE expires_at < $1 AND status != $2 ORDER BY expires_at ASC",
    )
    .bind(now)
    .bind(GarageStatus::Terminated)
    .fetch_all(pool)
    .await?;

    Ok(garages)
}

/// Update a garage's status.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn update_status(pool: &DbPool, id: Uuid, status: GarageStatus) -> DbResult<Garage> {
    let now = Utc::now();

    sqlx::query_as::<_, Garage>(
        r"
        UPDATE garages
        SET status = $1, updated_at = $2
        WHERE id = $3
        RETURNING *
        ",
    )
    .bind(status)
    .bind(now)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "garage",
        id: id.to_string(),
    })
}

/// Terminate a garage.
///
/// Sets status to `Terminated`, records `terminated_at` and `termination_reason`.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn terminate(pool: &DbPool, id: Uuid, reason: TerminationReason) -> DbResult<Garage> {
    let now = Utc::now();

    sqlx::query_as::<_, Garage>(
        r"
        UPDATE garages
        SET status = $1, updated_at = $2, terminated_at = $3, termination_reason = $4
        WHERE id = $5
        RETURNING *
        ",
    )
    .bind(GarageStatus::Terminated)
    .bind(now)
    .bind(now)
    .bind(reason)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "garage",
        id: id.to_string(),
    })
}

/// Extend a garage's TTL.
///
/// Adds `additional_seconds` to the current `expires_at`.
/// Also updates `ttl_seconds` to reflect the total TTL.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn extend_ttl(pool: &DbPool, id: Uuid, additional_seconds: i32) -> DbResult<Garage> {
    let now = Utc::now();

    sqlx::query_as::<_, Garage>(
        r"
        UPDATE garages
        SET expires_at = expires_at + ($1 * interval '1 second'),
            ttl_seconds = ttl_seconds + $1,
            updated_at = $2
        WHERE id = $3
        RETURNING *
        ",
    )
    .bind(additional_seconds)
    .bind(now)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "garage",
        id: id.to_string(),
    })
}

/// Check if a garage name is available.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn is_name_available(pool: &DbPool, name: &str) -> DbResult<bool> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM garages WHERE name = $1")
        .bind(name)
        .fetch_one(pool)
        .await?;

    Ok(count.0 == 0)
}

/// Delete a garage record (for testing/cleanup only).
///
/// In production, garages should be terminated, not deleted.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the garage doesn't exist.
pub async fn delete(pool: &DbPool, id: Uuid) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM garages WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "garage",
            id: id.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_garage_input() {
        let input = CreateGarage {
            id: Uuid::now_v7(),
            name: "bold-mongoose".to_string(),
            owner: "nick".to_string(),
            branch: "main".to_string(),
            ttl_seconds: 14400,
            namespace: "moto-garage-abc123".to_string(),
            pod_name: "dev-container".to_string(),
        };

        assert_eq!(input.name, "bold-mongoose");
        assert_eq!(input.ttl_seconds, 14400);
    }
}
