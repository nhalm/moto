//! Common error types for the moto monorepo.

use thiserror::Error;

/// Common error type for moto operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serde(String),

    /// Generic internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result type alias using the common Error.
pub type Result<T> = std::result::Result<T, Error>;
