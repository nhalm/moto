//! Namespace operations trait.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Namespace;

use crate::Result;

/// Trait for Kubernetes namespace operations.
///
/// This trait abstracts namespace CRUD operations, allowing for different
/// implementations (real K8s client, mock for testing, etc.).
pub trait NamespaceOps {
    /// Creates a namespace with the given name and labels.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace already exists or creation fails.
    fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> impl Future<Output = Result<Namespace>> + Send;

    /// Deletes a namespace by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or deletion fails.
    fn delete_namespace(&self, name: &str) -> impl Future<Output = Result<()>> + Send;

    /// Gets a namespace by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or the operation fails.
    fn get_namespace(&self, name: &str) -> impl Future<Output = Result<Namespace>> + Send;

    /// Lists namespaces matching the given label selector.
    ///
    /// # Errors
    ///
    /// Returns an error if the list operation fails.
    fn list_namespaces(
        &self,
        label_selector: Option<&str>,
    ) -> impl Future<Output = Result<Vec<Namespace>>> + Send;

    /// Checks if a namespace exists.
    fn namespace_exists(&self, name: &str) -> impl Future<Output = Result<bool>> + Send;
}

use std::future::Future;
