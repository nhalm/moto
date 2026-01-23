//! Garage namespace management.
//!
//! Provides operations for creating, listing, and deleting garage namespaces
//! with moto-specific labels and naming conventions.

use std::future::Future;

use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::Namespace;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{Labels, NamespaceOps, Result};

use crate::GarageK8s;

/// Input for creating a garage namespace.
#[derive(Debug, Clone)]
pub struct GarageNamespaceInput {
    /// Unique garage identifier.
    pub id: GarageId,
    /// Human-friendly garage name.
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Optional expiration time.
    pub expires_at: Option<DateTime<Utc>>,
    /// Optional engine name.
    pub engine: Option<String>,
}

impl GarageNamespaceInput {
    /// Returns the K8s namespace name for this garage.
    ///
    /// Format: `moto-garage-{short_id}` (e.g., `moto-garage-abc12345`).
    #[must_use]
    pub fn namespace_name(&self) -> String {
        format!("moto-garage-{}", self.id.short())
    }
}

/// Trait for garage namespace operations.
pub trait GarageNamespaceOps {
    /// Creates a namespace for a garage.
    ///
    /// The namespace is named `moto-garage-{short_id}` and includes labels:
    /// - `moto.dev/type: garage`
    /// - `moto.dev/id: {id}`
    /// - `moto.dev/name: {name}`
    /// - `moto.dev/owner: {owner}`
    /// - `moto.dev/expires-at: {expires_at}` (optional)
    /// - `moto.dev/engine: {engine}` (optional)
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace already exists or creation fails.
    fn create_garage_namespace(
        &self,
        input: &GarageNamespaceInput,
    ) -> impl Future<Output = Result<Namespace>> + Send;

    /// Deletes a garage namespace by ID.
    ///
    /// This cascades to delete all resources in the namespace (pods, services, etc.).
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or deletion fails.
    fn delete_garage_namespace(&self, id: &GarageId) -> impl Future<Output = Result<()>> + Send;

    /// Gets a garage namespace by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or the operation fails.
    fn get_garage_namespace(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<Namespace>> + Send;

    /// Lists all garage namespaces.
    ///
    /// Returns namespaces with label `moto.dev/type=garage`.
    ///
    /// # Errors
    ///
    /// Returns an error if the list operation fails.
    fn list_garage_namespaces(&self) -> impl Future<Output = Result<Vec<Namespace>>> + Send;

    /// Lists garage namespaces for a specific owner.
    ///
    /// # Errors
    ///
    /// Returns an error if the list operation fails.
    fn list_garage_namespaces_by_owner(
        &self,
        owner: &str,
    ) -> impl Future<Output = Result<Vec<Namespace>>> + Send;

    /// Checks if a garage namespace exists.
    fn garage_namespace_exists(&self, id: &GarageId) -> impl Future<Output = Result<bool>> + Send;
}

impl GarageNamespaceOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %input.id, garage_name = %input.name))]
    async fn create_garage_namespace(&self, input: &GarageNamespaceInput) -> Result<Namespace> {
        let namespace_name = input.namespace_name();

        let expires_at_str = input.expires_at.map(|dt| dt.to_rfc3339());
        let labels = Labels::for_garage(
            &input.id.to_string(),
            &input.name,
            Some(&input.owner),
            expires_at_str.as_deref(),
            input.engine.as_deref(),
        );

        debug!(namespace = %namespace_name, "creating garage namespace");
        self.client.create_namespace(&namespace_name, labels).await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn delete_garage_namespace(&self, id: &GarageId) -> Result<()> {
        let namespace_name = format!("moto-garage-{}", id.short());
        debug!(namespace = %namespace_name, "deleting garage namespace");
        self.client.delete_namespace(&namespace_name).await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn get_garage_namespace(&self, id: &GarageId) -> Result<Namespace> {
        let namespace_name = format!("moto-garage-{}", id.short());
        debug!(namespace = %namespace_name, "getting garage namespace");
        self.client.get_namespace(&namespace_name).await
    }

    #[instrument(skip(self))]
    async fn list_garage_namespaces(&self) -> Result<Vec<Namespace>> {
        let selector = Labels::garage_selector();
        debug!(selector = %selector, "listing garage namespaces");
        self.client.list_namespaces(Some(&selector)).await
    }

    #[instrument(skip(self), fields(owner = %owner))]
    async fn list_garage_namespaces_by_owner(&self, owner: &str) -> Result<Vec<Namespace>> {
        let selector = format!("{},{}={}", Labels::garage_selector(), Labels::OWNER, owner);
        debug!(selector = %selector, "listing garage namespaces by owner");
        self.client.list_namespaces(Some(&selector)).await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn garage_namespace_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace_name = format!("moto-garage-{}", id.short());
        debug!(namespace = %namespace_name, "checking garage namespace exists");
        self.client.namespace_exists(&namespace_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn namespace_name_format() {
        let input = GarageNamespaceInput {
            id: GarageId::new(),
            name: "my-project".to_string(),
            owner: "alice".to_string(),
            expires_at: None,
            engine: None,
        };

        let ns_name = input.namespace_name();
        assert!(ns_name.starts_with("moto-garage-"));
        assert_eq!(ns_name.len(), "moto-garage-".len() + 8);
    }

    #[test]
    fn namespace_input_with_expires_at() {
        let expires = Utc.with_ymd_and_hms(2026, 1, 23, 14, 0, 0).unwrap();
        let input = GarageNamespaceInput {
            id: GarageId::new(),
            name: "test".to_string(),
            owner: "bob".to_string(),
            expires_at: Some(expires),
            engine: Some("moto-club".to_string()),
        };

        assert_eq!(input.expires_at, Some(expires));
        assert_eq!(input.engine, Some("moto-club".to_string()));
    }
}
