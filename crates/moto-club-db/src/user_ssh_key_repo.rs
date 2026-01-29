//! User SSH key repository for database operations.
//!
//! Provides CRUD operations for the `user_ssh_keys` table.
//! SSH keys are used for injection into garage pods at creation time.

use crate::{DbError, DbPool, DbResult, UserSshKey};
use uuid::Uuid;

/// Input for creating/registering a new user SSH key.
#[derive(Debug, Clone)]
pub struct CreateUserSshKey {
    /// Owner identifier.
    pub owner: String,
    /// SSH public key (e.g., "ssh-ed25519 AAAA... user@host").
    pub public_key: String,
    /// Key fingerprint (SHA256:...).
    pub fingerprint: String,
}

/// Get an SSH key by its ID.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the key doesn't exist.
pub async fn get_by_id(pool: &DbPool, id: Uuid) -> DbResult<UserSshKey> {
    sqlx::query_as::<_, UserSshKey>("SELECT * FROM user_ssh_keys WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DbError::NotFound {
            entity: "ssh_key",
            id: id.to_string(),
        })
}

/// Get an SSH key by owner and fingerprint.
///
/// # Errors
///
/// Returns `DbError::NotFound` if no key matches.
pub async fn get_by_owner_fingerprint(
    pool: &DbPool,
    owner: &str,
    fingerprint: &str,
) -> DbResult<UserSshKey> {
    sqlx::query_as::<_, UserSshKey>(
        "SELECT * FROM user_ssh_keys WHERE owner = $1 AND fingerprint = $2",
    )
    .bind(owner)
    .bind(fingerprint)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "ssh_key",
        id: fingerprint.to_string(),
    })
}

/// Create a new user SSH key.
///
/// # Errors
///
/// Returns `DbError::AlreadyExists` if a key with the same owner and fingerprint exists.
pub async fn create(pool: &DbPool, input: CreateUserSshKey) -> DbResult<UserSshKey> {
    let id = Uuid::now_v7();

    let result = sqlx::query_as::<_, UserSshKey>(
        r"
        INSERT INTO user_ssh_keys (id, owner, public_key, fingerprint)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        ",
    )
    .bind(id)
    .bind(&input.owner)
    .bind(&input.public_key)
    .bind(&input.fingerprint)
    .fetch_one(pool)
    .await;

    match result {
        Ok(key) => Ok(key),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(DbError::AlreadyExists {
                entity: "ssh_key",
                id: input.fingerprint,
            })
        }
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

/// Get or create an SSH key (idempotent registration).
///
/// If a key with the same owner and fingerprint already exists, returns the existing key.
/// This implements the idempotent SSH key registration behavior per the spec:
/// same key re-registered by same user returns 200 OK with existing record.
///
/// Returns a tuple of (key, created) where `created` is true if a new key was created.
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn get_or_create(pool: &DbPool, input: CreateUserSshKey) -> DbResult<(UserSshKey, bool)> {
    // Try to get existing key first
    match get_by_owner_fingerprint(pool, &input.owner, &input.fingerprint).await {
        Ok(existing) => {
            // Same owner and fingerprint, return existing key (created = false)
            Ok((existing, false))
        }
        Err(DbError::NotFound { .. }) => {
            // Key doesn't exist, create it
            let key = create(pool, input).await?;
            Ok((key, true))
        }
        Err(e) => Err(e),
    }
}

/// List all SSH keys for an owner.
///
/// Returns keys ordered by `created_at` descending (newest first).
///
/// # Errors
///
/// Returns a database error if the query fails.
pub async fn list_by_owner(pool: &DbPool, owner: &str) -> DbResult<Vec<UserSshKey>> {
    let keys = sqlx::query_as::<_, UserSshKey>(
        "SELECT * FROM user_ssh_keys WHERE owner = $1 ORDER BY created_at DESC",
    )
    .bind(owner)
    .fetch_all(pool)
    .await?;

    Ok(keys)
}

/// Delete an SSH key by ID.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the key doesn't exist.
pub async fn delete(pool: &DbPool, id: Uuid) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM user_ssh_keys WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "ssh_key",
            id: id.to_string(),
        });
    }

    Ok(())
}

/// Delete an SSH key by ID, verifying ownership.
///
/// # Errors
///
/// Returns `DbError::NotFound` if the key doesn't exist.
/// Returns `DbError::NotOwned` if the key belongs to a different owner.
pub async fn delete_owned(pool: &DbPool, id: Uuid, owner: &str) -> DbResult<()> {
    // First check if the key exists and verify ownership
    let key = get_by_id(pool, id).await?;

    if key.owner != owner {
        return Err(DbError::NotOwned {
            entity: "ssh_key",
            id: id.to_string(),
        });
    }

    // Now delete
    sqlx::query("DELETE FROM user_ssh_keys WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_user_ssh_key_input() {
        let input = CreateUserSshKey {
            owner: "nick".to_string(),
            public_key: "ssh-ed25519 AAAA... user@host".to_string(),
            fingerprint: "SHA256:abc123".to_string(),
        };

        assert_eq!(input.owner, "nick");
        assert_eq!(input.public_key, "ssh-ed25519 AAAA... user@host");
        assert_eq!(input.fingerprint, "SHA256:abc123");
    }
}
