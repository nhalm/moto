//! Database layer for moto-keybox.
//!
//! This module provides:
//! - Data models that map to the PostgreSQL schema
//! - Repository functions for secrets, DEKs, and audit logs

use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

/// Database connection pool type alias.
pub type DbPool = sqlx::PgPool;

/// Database errors.
#[derive(Debug, Error)]
pub enum DbError {
    /// SQLx database error.
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// Record not found.
    #[error("{entity} not found: {id}")]
    NotFound {
        /// Entity type (e.g., "secret", "dek").
        entity: &'static str,
        /// Entity identifier.
        id: String,
    },

    /// Duplicate record.
    #[error("{entity} already exists: {id}")]
    AlreadyExists {
        /// Entity type.
        entity: &'static str,
        /// Entity identifier.
        id: String,
    },
}

/// Result type for database operations.
pub type DbResult<T> = Result<T, DbError>;

/// Create a database connection pool.
///
/// # Errors
///
/// Returns an error if the connection fails.
pub async fn connect(database_url: &str) -> DbResult<DbPool> {
    let pool = sqlx::PgPool::connect(database_url).await?;
    Ok(pool)
}

// ============================================================================
// Models
// ============================================================================

/// Secret metadata (not the encrypted value).
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Secret {
    /// Unique identifier.
    pub id: Uuid,
    /// Scope: global, service, or instance.
    pub scope: String,
    /// Service name (null for global).
    pub service: Option<String>,
    /// Instance ID: garage-id or bike-id (null for global/service).
    pub instance_id: Option<String>,
    /// Secret name (e.g., "ai/anthropic").
    pub name: String,
    /// Current version number.
    pub current_version: i32,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Soft delete timestamp.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Secret version with encrypted value.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SecretVersion {
    /// Unique identifier.
    pub id: Uuid,
    /// Parent secret ID.
    pub secret_id: Uuid,
    /// Version number.
    pub version: i32,
    /// Encrypted secret value.
    pub ciphertext: Vec<u8>,
    /// Nonce used for encryption.
    pub nonce: Vec<u8>,
    /// Reference to DEK used for encryption.
    pub dek_id: Uuid,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Encrypted Data Encryption Key (DEK).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EncryptedDek {
    /// Unique identifier.
    pub id: Uuid,
    /// DEK encrypted with master key (KEK).
    pub encrypted_key: Vec<u8>,
    /// Nonce used for DEK encryption.
    pub nonce: Vec<u8>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Audit log entry.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct AuditLogEntry {
    /// Unique identifier.
    pub id: Uuid,
    /// Event type: accessed, created, deleted, etc.
    pub event_type: String,
    /// Principal type: garage, bike, service.
    pub principal_type: Option<String>,
    /// Principal ID.
    pub principal_id: Option<String>,
    /// SPIFFE ID of the accessor.
    pub spiffe_id: Option<String>,
    /// Secret scope.
    pub secret_scope: Option<String>,
    /// Secret name.
    pub secret_name: Option<String>,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Secret Repository
// ============================================================================

/// Input for creating a new secret.
pub struct CreateSecret {
    /// Scope: global, service, or instance.
    pub scope: String,
    /// Service name (required for service/instance scope).
    pub service: Option<String>,
    /// Instance ID (required for instance scope).
    pub instance_id: Option<String>,
    /// Secret name.
    pub name: String,
}

/// Get a secret by scope and name.
pub async fn get_secret(
    pool: &DbPool,
    scope: &str,
    service: Option<&str>,
    instance_id: Option<&str>,
    name: &str,
) -> DbResult<Secret> {
    let secret = sqlx::query_as::<_, Secret>(
        r"SELECT id, scope, service, instance_id, name, current_version,
                 created_at, updated_at, deleted_at
         FROM secrets
         WHERE scope = $1
           AND (service = $2 OR (service IS NULL AND $2 IS NULL))
           AND (instance_id = $3 OR (instance_id IS NULL AND $3 IS NULL))
           AND name = $4
           AND deleted_at IS NULL",
    )
    .bind(scope)
    .bind(service)
    .bind(instance_id)
    .bind(name)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "secret",
        id: name.to_string(),
    })?;

    Ok(secret)
}

/// Create a new secret with its first version.
pub async fn create_secret(
    pool: &DbPool,
    input: CreateSecret,
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    dek_id: Uuid,
) -> DbResult<Secret> {
    let id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let now = Utc::now();

    // Start transaction
    let mut tx = pool.begin().await?;

    // Insert secret metadata
    sqlx::query(
        r"INSERT INTO secrets (id, scope, service, instance_id, name, current_version, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, 1, $6, $6)",
    )
    .bind(id)
    .bind(&input.scope)
    .bind(&input.service)
    .bind(&input.instance_id)
    .bind(&input.name)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.is_unique_violation() {
                return DbError::AlreadyExists {
                    entity: "secret",
                    id: input.name.clone(),
                };
            }
        }
        DbError::Sqlx(e)
    })?;

    // Insert first version
    sqlx::query(
        r"INSERT INTO secret_versions (id, secret_id, version, ciphertext, nonce, dek_id, created_at)
         VALUES ($1, $2, 1, $3, $4, $5, $6)",
    )
    .bind(version_id)
    .bind(id)
    .bind(&ciphertext)
    .bind(&nonce)
    .bind(dek_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Secret {
        id,
        scope: input.scope,
        service: input.service,
        instance_id: input.instance_id,
        name: input.name,
        current_version: 1,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    })
}

/// Update a secret with a new version.
pub async fn update_secret(
    pool: &DbPool,
    secret_id: Uuid,
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    dek_id: Uuid,
) -> DbResult<Secret> {
    let version_id = Uuid::now_v7();
    let now = Utc::now();

    // Start transaction
    let mut tx = pool.begin().await?;

    // Get current version and increment
    let secret = sqlx::query_as::<_, Secret>(
        r"UPDATE secrets
         SET current_version = current_version + 1, updated_at = $2
         WHERE id = $1 AND deleted_at IS NULL
         RETURNING id, scope, service, instance_id, name, current_version, created_at, updated_at, deleted_at",
    )
    .bind(secret_id)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "secret",
        id: secret_id.to_string(),
    })?;

    // Insert new version
    sqlx::query(
        r"INSERT INTO secret_versions (id, secret_id, version, ciphertext, nonce, dek_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(version_id)
    .bind(secret_id)
    .bind(secret.current_version)
    .bind(&ciphertext)
    .bind(&nonce)
    .bind(dek_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(secret)
}

/// Soft delete a secret.
pub async fn delete_secret(pool: &DbPool, secret_id: Uuid) -> DbResult<()> {
    let result =
        sqlx::query(r"UPDATE secrets SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL")
            .bind(secret_id)
            .bind(Utc::now())
            .execute(pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound {
            entity: "secret",
            id: secret_id.to_string(),
        });
    }

    Ok(())
}

/// List secrets in a scope (names only, no values).
pub async fn list_secrets(
    pool: &DbPool,
    scope: &str,
    service: Option<&str>,
    instance_id: Option<&str>,
) -> DbResult<Vec<Secret>> {
    let secrets = sqlx::query_as::<_, Secret>(
        r"SELECT id, scope, service, instance_id, name, current_version,
                 created_at, updated_at, deleted_at
         FROM secrets
         WHERE scope = $1
           AND (service = $2 OR (service IS NULL AND $2 IS NULL))
           AND (instance_id = $3 OR (instance_id IS NULL AND $3 IS NULL))
           AND deleted_at IS NULL
         ORDER BY name",
    )
    .bind(scope)
    .bind(service)
    .bind(instance_id)
    .fetch_all(pool)
    .await?;

    Ok(secrets)
}

/// Get the current version of a secret.
pub async fn get_secret_version(pool: &DbPool, secret_id: Uuid) -> DbResult<SecretVersion> {
    let version = sqlx::query_as::<_, SecretVersion>(
        r"SELECT sv.id, sv.secret_id, sv.version, sv.ciphertext, sv.nonce, sv.dek_id, sv.created_at
         FROM secret_versions sv
         JOIN secrets s ON sv.secret_id = s.id
         WHERE sv.secret_id = $1 AND sv.version = s.current_version",
    )
    .bind(secret_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "secret_version",
        id: secret_id.to_string(),
    })?;

    Ok(version)
}

// ============================================================================
// DEK Repository
// ============================================================================

/// Create a new encrypted DEK.
pub async fn create_dek(
    pool: &DbPool,
    encrypted_key: Vec<u8>,
    nonce: Vec<u8>,
) -> DbResult<EncryptedDek> {
    let id = Uuid::now_v7();
    let now = Utc::now();

    sqlx::query(
        r"INSERT INTO encrypted_deks (id, encrypted_key, nonce, created_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(&encrypted_key)
    .bind(&nonce)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(EncryptedDek {
        id,
        encrypted_key,
        nonce,
        created_at: now,
    })
}

/// Get an encrypted DEK by ID.
pub async fn get_dek(pool: &DbPool, id: Uuid) -> DbResult<EncryptedDek> {
    let dek = sqlx::query_as::<_, EncryptedDek>(
        r"SELECT id, encrypted_key, nonce, created_at FROM encrypted_deks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound {
        entity: "dek",
        id: id.to_string(),
    })?;

    Ok(dek)
}

// ============================================================================
// Audit Log Repository
// ============================================================================

/// Input for creating an audit log entry.
pub struct CreateAuditEntry {
    /// Event type.
    pub event_type: String,
    /// Principal type.
    pub principal_type: Option<String>,
    /// Principal ID.
    pub principal_id: Option<String>,
    /// SPIFFE ID.
    pub spiffe_id: Option<String>,
    /// Secret scope.
    pub secret_scope: Option<String>,
    /// Secret name.
    pub secret_name: Option<String>,
}

/// Create an audit log entry.
pub async fn create_audit_entry(pool: &DbPool, entry: CreateAuditEntry) -> DbResult<()> {
    let id = Uuid::now_v7();

    sqlx::query(
        r"INSERT INTO audit_log (id, event_type, principal_type, principal_id, spiffe_id, secret_scope, secret_name, timestamp)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(&entry.event_type)
    .bind(&entry.principal_type)
    .bind(&entry.principal_id)
    .bind(&entry.spiffe_id)
    .bind(&entry.secret_scope)
    .bind(&entry.secret_name)
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(())
}

/// Query audit logs with optional filters.
pub async fn query_audit_logs(
    pool: &DbPool,
    spiffe_id: Option<&str>,
    secret_name: Option<&str>,
    limit: i64,
) -> DbResult<Vec<AuditLogEntry>> {
    let entries = sqlx::query_as::<_, AuditLogEntry>(
        r"SELECT id, event_type, principal_type, principal_id, spiffe_id,
                 secret_scope, secret_name, timestamp
         FROM audit_log
         WHERE ($1::TEXT IS NULL OR spiffe_id = $1)
           AND ($2::TEXT IS NULL OR secret_name = $2)
         ORDER BY timestamp DESC
         LIMIT $3",
    )
    .bind(spiffe_id)
    .bind(secret_name)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_error_display() {
        let err = DbError::NotFound {
            entity: "secret",
            id: "ai/anthropic".to_string(),
        };
        assert_eq!(err.to_string(), "secret not found: ai/anthropic");

        let err = DbError::AlreadyExists {
            entity: "secret",
            id: "ai/anthropic".to_string(),
        };
        assert_eq!(err.to_string(), "secret already exists: ai/anthropic");
    }
}
