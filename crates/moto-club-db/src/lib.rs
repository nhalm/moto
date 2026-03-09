//! Database layer for moto-club.
//!
//! This crate provides:
//! - Data models that map to the `PostgreSQL` schema (`models`)
//! - Repository functions for database operations (`garage_repo`)
//!
//! # Example
//!
//! ```ignore
//! use moto_club_db::{DbPool, garage_repo, garage_repo::CreateGarage};
//! use uuid::Uuid;
//!
//! let pool = DbPool::connect("postgres://...").await?;
//!
//! // Create a new garage
//! let input = CreateGarage {
//!     id: Uuid::now_v7(),
//!     name: "bold-mongoose".to_string(),
//!     owner: "nick".to_string(),
//!     branch: "main".to_string(),
//!     image: "ghcr.io/nhalm/moto-dev:latest".to_string(),
//!     ttl_seconds: 14400,
//!     namespace: "moto-garage-abc123".to_string(),
//!     pod_name: "dev-container".to_string(),
//! };
//! let garage = garage_repo::create(&pool, input).await?;
//!
//! // Get a garage by ID
//! let garage = garage_repo::get_by_id(&pool, garage.id).await?;
//! ```

pub mod audit_repo;
pub mod garage_repo;
pub mod models;
pub mod wg_device_repo;
pub mod wg_garage_repo;
pub mod wg_session_repo;

use thiserror::Error;

pub use models::{
    Garage, GarageStatus, ParseGarageStatusError, ParseTerminationReasonError, TerminationReason,
    WgDevice, WgGarage, WgSession,
};
pub use wg_session_repo::{ListSessionsFilter, WgSessionWithDetails};

// Re-export TerminationReason::ErrorReason as Error for convenience
// (the variant is named ErrorReason internally to avoid conflict with Error trait)

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
        /// Entity type (e.g., "garage", "device").
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

    /// Entity owned by different user.
    #[error("{entity} not owned: {id}")]
    NotOwned {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_error_display() {
        let err = DbError::NotFound {
            entity: "garage",
            id: "abc123".to_string(),
        };
        assert_eq!(err.to_string(), "garage not found: abc123");

        let err = DbError::AlreadyExists {
            entity: "device",
            id: "xyz789".to_string(),
        };
        assert_eq!(err.to_string(), "device already exists: xyz789");

        let err = DbError::NotOwned {
            entity: "device",
            id: "pubkey123".to_string(),
        };
        assert_eq!(err.to_string(), "device not owned: pubkey123");
    }

    #[test]
    fn migrations_are_embedded() {
        // Verify migrations are properly embedded at compile time
        assert!(
            MIGRATIONS.iter().next().is_some(),
            "migrations should be embedded"
        );

        let migrations: Vec<_> = MIGRATIONS.iter().collect();
        assert_eq!(migrations.len(), 4, "should have exactly four migrations");
        // sqlx converts underscores to spaces in descriptions
        assert!(
            migrations[0].description.contains("initial"),
            "first migration should be initial schema"
        );
    }
}
