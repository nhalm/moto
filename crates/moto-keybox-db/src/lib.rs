//! Database layer for moto-keybox.
//!
//! This crate provides:
//! - Data models that map to the `PostgreSQL` schema (`models`)
//! - Repository functions for database operations (`secret_repo`, `audit_repo`)
//!
//! # Schema
//!
//! The keybox database schema consists of four tables:
//!
//! - `secrets`: Secret metadata (scope, service, `instance_id`, name, version)
//! - `secret_versions`: Encrypted secret values (ciphertext, nonce, dek reference)
//! - `encrypted_deks`: Encrypted data encryption keys (wrapped by KEK)
//! - `audit_log`: Access and security events (no secret values)
//!
//! # Example
//!
//! ```ignore
//! use moto_keybox_db::{DbPool, Secret, Scope, secret_repo, audit_repo};
//! use uuid::Uuid;
//!
//! let pool = moto_keybox_db::connect("postgres://...").await?;
//!
//! // Create a secret
//! let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, "ai/anthropic").await?;
//!
//! // Log an audit entry
//! audit_repo::insert_audit_entry(
//!     &pool,
//!     &InsertAuditEntry {
//!         event_type: AuditEventType::SecretCreated,
//!         service: "keybox",
//!         principal_type: PrincipalType::Service,
//!         principal_id: "spiffe://moto.local/service/moto-club",
//!         action: "create",
//!         resource_type: "secret",
//!         resource_id: "global/ai/anthropic",
//!         outcome: "success",
//!         metadata: serde_json::json!({}),
//!         client_ip: None,
//!     },
//! ).await?;
//! ```

pub mod audit_repo;
pub mod models;
pub mod secret_repo;

use thiserror::Error;

pub use audit_repo::{AuditLogQuery, InsertAuditEntry};
pub use models::{
    AuditEventType, AuditLogEntry, EncryptedDek, ParseAuditEventTypeError, ParsePrincipalTypeError,
    ParseScopeError, PrincipalType, Scope, Secret, SecretVersion,
};

/// Database connection pool type alias.
pub type DbPool = sqlx::PgPool;

/// Database errors.
#[derive(Debug, Error)]
pub enum DbError {
    /// `SQLx` database error.
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// Migration error.
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

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

/// Embedded database migrations.
///
/// This uses sqlx's compile-time migration embedding to include all
/// SQL migration files from the `migrations/` directory.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!();

/// Create a database connection pool.
///
/// # Errors
///
/// Returns an error if the connection fails.
pub async fn connect(database_url: &str) -> DbResult<DbPool> {
    let pool = sqlx::PgPool::connect(database_url).await?;
    Ok(pool)
}

/// Create a database connection pool with options.
///
/// # Errors
///
/// Returns an error if the connection fails.
pub async fn connect_with_options(database_url: &str, max_connections: u32) -> DbResult<DbPool> {
    use sqlx::postgres::PgPoolOptions;

    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run database migrations.
///
/// This applies all pending migrations from the embedded `MIGRATIONS`.
/// Safe to call multiple times - already applied migrations are skipped.
///
/// # Errors
///
/// Returns an error if migration fails.
pub async fn run_migrations(pool: &DbPool) -> DbResult<()> {
    MIGRATIONS.run(pool).await?;
    Ok(())
}

/// SQL schema for the keybox database.
///
/// This is provided as a constant for reference. Use sqlx migrations
/// for actual schema management.
pub const SCHEMA_SQL: &str = r"
-- Secrets metadata
CREATE TABLE IF NOT EXISTS secrets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope TEXT NOT NULL,        -- global, service, instance
    service TEXT,               -- null for global
    instance_id TEXT,           -- null for global/service
    name TEXT NOT NULL,
    current_version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,     -- soft delete
    UNIQUE(scope, service, instance_id, name)
);

-- Encrypted DEKs (must be created before secret_versions due to FK)
CREATE TABLE IF NOT EXISTS encrypted_deks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    encrypted_key BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Secret versions (encrypted values)
CREATE TABLE IF NOT EXISTS secret_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    secret_id UUID NOT NULL REFERENCES secrets(id),
    version INTEGER NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    dek_id UUID NOT NULL REFERENCES encrypted_deks(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(secret_id, version)
);

-- Audit log (unified schema)
CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type TEXT NOT NULL,
    service TEXT NOT NULL DEFAULT 'keybox',
    principal_type TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    outcome TEXT NOT NULL DEFAULT 'success',
    metadata JSONB NOT NULL DEFAULT '{}',
    client_ip TEXT,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_secrets_scope ON secrets(scope);
CREATE INDEX IF NOT EXISTS idx_secrets_service ON secrets(service) WHERE service IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_log_principal ON audit_log(principal_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_event_type ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_log_resource ON audit_log(resource_type, resource_id);
";

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
            id: "ai/openai".to_string(),
        };
        assert_eq!(err.to_string(), "secret already exists: ai/openai");
    }

    #[test]
    fn schema_sql_is_valid() {
        // Basic sanity check that the schema SQL is non-empty and contains expected tables
        assert!(SCHEMA_SQL.contains("CREATE TABLE"));
        assert!(SCHEMA_SQL.contains("secrets"));
        assert!(SCHEMA_SQL.contains("secret_versions"));
        assert!(SCHEMA_SQL.contains("encrypted_deks"));
        assert!(SCHEMA_SQL.contains("audit_log"));
    }

    #[test]
    fn migrations_are_embedded() {
        // Verify migrations are properly embedded at compile time
        assert!(
            MIGRATIONS.iter().next().is_some(),
            "migrations should be embedded"
        );

        // Check the initial migration exists
        let migrations: Vec<_> = MIGRATIONS.iter().collect();
        assert_eq!(migrations.len(), 2, "should have two migrations");
        // sqlx converts underscores to spaces in descriptions
        assert!(
            migrations[0].description.contains("initial"),
            "first migration should be initial schema"
        );
        assert!(
            migrations[1].description.contains("audit"),
            "second migration should be audit log unified schema"
        );
    }
}
