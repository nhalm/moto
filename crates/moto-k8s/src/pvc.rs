//! `PersistentVolumeClaim` operations trait.

use std::future::Future;

use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use crate::Result;

/// Trait for Kubernetes `PersistentVolumeClaim` operations.
///
/// This trait abstracts PVC CRUD operations, allowing for different
/// implementations (real K8s client, mock for testing, etc.).
pub trait PvcOps {
    /// Creates a `PersistentVolumeClaim` in the given namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the PVC already exists or creation fails.
    fn create_pvc(
        &self,
        namespace: &str,
        pvc: &PersistentVolumeClaim,
    ) -> impl Future<Output = Result<PersistentVolumeClaim>> + Send;

    /// Gets a `PersistentVolumeClaim` by name in the given namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the PVC doesn't exist or the operation fails.
    fn get_pvc(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<PersistentVolumeClaim>> + Send;

    /// Deletes a `PersistentVolumeClaim` by name in the given namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the PVC doesn't exist or deletion fails.
    fn delete_pvc(&self, namespace: &str, name: &str) -> impl Future<Output = Result<()>> + Send;

    /// Checks if a `PersistentVolumeClaim` exists in the given namespace.
    fn pvc_exists(&self, namespace: &str, name: &str) -> impl Future<Output = Result<bool>> + Send;
}
