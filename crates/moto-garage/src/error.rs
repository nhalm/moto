//! Error types for garage operations.

use thiserror::Error;

/// Errors that can occur during garage operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Garage not found by ID or name.
    #[error("garage not found: {0}")]
    GarageNotFound(String),

    /// Garage already exists with this name.
    #[error("garage already exists: {0}")]
    GarageExists(String),

    /// K8s operation failed.
    #[error("k8s error: {0}")]
    K8s(#[from] moto_k8s::Error),

    /// Remote mode is not yet implemented.
    #[error("remote mode not implemented")]
    RemoteNotImplemented,
}

/// Result type alias for garage operations.
pub type Result<T> = std::result::Result<T, Error>;
