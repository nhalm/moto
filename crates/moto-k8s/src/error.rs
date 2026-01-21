//! Error types for K8s operations.

use thiserror::Error;

/// Errors that can occur during K8s operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Failed to create K8s client from kubeconfig.
    #[error("failed to create K8s client: {0}")]
    ClientCreate(#[source] kube::Error),

    /// Namespace already exists.
    #[error("namespace already exists: {0}")]
    NamespaceExists(String),

    /// Namespace not found.
    #[error("namespace not found: {0}")]
    NamespaceNotFound(String),

    /// Failed to create namespace.
    #[error("failed to create namespace: {0}")]
    NamespaceCreate(#[source] kube::Error),

    /// Failed to delete namespace.
    #[error("failed to delete namespace: {0}")]
    NamespaceDelete(#[source] kube::Error),

    /// Failed to get namespace.
    #[error("failed to get namespace: {0}")]
    NamespaceGet(#[source] kube::Error),

    /// Failed to list namespaces.
    #[error("failed to list namespaces: {0}")]
    NamespaceList(#[source] kube::Error),
}

/// Result type alias for K8s operations.
pub type Result<T> = std::result::Result<T, Error>;
