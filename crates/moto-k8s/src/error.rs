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

    /// Failed to list pods.
    #[error("failed to list pods: {0}")]
    PodList(#[source] kube::Error),

    /// Pod not found.
    #[error("pod not found: {0}")]
    PodNotFound(String),

    /// Failed to get pod logs.
    #[error("failed to get pod logs: {0}")]
    PodLogs(#[source] kube::Error),

    /// I/O error while streaming logs.
    #[error("I/O error: {0}")]
    IoError(#[source] std::io::Error),

    /// Failed to read kubeconfig.
    #[error("failed to read kubeconfig: {0}")]
    KubeconfigRead(#[source] kube::config::KubeconfigError),

    /// Context not found.
    #[error("context not found: {0}")]
    ContextNotFound(String),

    /// Failed to create deployment.
    #[error("failed to create deployment: {0}")]
    DeploymentCreate(#[source] kube::Error),

    /// Failed to get deployment.
    #[error("failed to get deployment: {0}")]
    DeploymentGet(#[source] kube::Error),

    /// Failed to update deployment.
    #[error("failed to update deployment: {0}")]
    DeploymentUpdate(#[source] kube::Error),

    /// Deployment not found.
    #[error("deployment not found: {0}")]
    DeploymentNotFound(String),

    /// Failed to create service.
    #[error("failed to create service: {0}")]
    ServiceCreate(#[source] kube::Error),

    /// Failed to get service.
    #[error("failed to get service: {0}")]
    ServiceGet(#[source] kube::Error),

    /// Deployment timed out waiting for readiness.
    #[error("deployment timed out waiting for readiness: {0}")]
    DeploymentTimeout(String),

    /// Failed to list deployments.
    #[error("failed to list deployments: {0}")]
    DeploymentList(#[source] kube::Error),

    /// Failed to perform token review.
    #[error("failed to perform token review: {0}")]
    TokenReview(#[source] kube::Error),

    /// Token not authenticated (invalid or expired).
    #[error("token not authenticated")]
    TokenNotAuthenticated,
}

/// Result type alias for K8s operations.
pub type Result<T> = std::result::Result<T, Error>;
