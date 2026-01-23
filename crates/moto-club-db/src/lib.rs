//! Database layer for moto-club.
//!
//! This crate provides:
//! - Data models that map to the `PostgreSQL` schema (`models`)
//! - Repository traits and implementations for database operations
//!
//! # Example
//!
//! ```ignore
//! use moto_club_db::{models::Garage, DbPool};
//!
//! let pool = DbPool::connect("postgres://...").await?;
//! let garage = sqlx::query_as::<_, Garage>("SELECT * FROM garages WHERE id = $1")
//!     .bind(id)
//!     .fetch_one(&pool)
//!     .await?;
//! ```

pub mod models;

use thiserror::Error;

pub use models::{
    DerpServer, Garage, GarageStatus, ParseGarageStatusError, ParseTerminationReasonError,
    TerminationReason, UserSshKey, WgDevice, WgSession,
};

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

/// Create a database connection pool with options.
///
/// # Errors
///
/// Returns an error if the connection fails.
pub async fn connect_with_options(
    database_url: &str,
    max_connections: u32,
) -> DbResult<DbPool> {
    use sqlx::postgres::PgPoolOptions;

    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await?;
    Ok(pool)
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
    }
}
