//! Garage client for managing dev environments.

use chrono::Utc;
use moto_club_types::{GarageId, GarageInfo, GarageState};
use moto_k8s::{K8sClient, Labels, NamespaceOps};
use tracing::{debug, instrument};

use crate::{Error, GarageMode, Result};

/// Client for garage operations.
///
/// Abstracts the difference between local (direct K8s) and remote (via club)
/// modes, providing the same interface for both.
pub struct GarageClient {
    mode: GarageMode,
    k8s: Option<K8sClient>,
}

impl GarageClient {
    /// Creates a new garage client in local mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the K8s client cannot be created.
    pub async fn local() -> Result<Self> {
        let k8s = K8sClient::new().await?;
        Ok(Self {
            mode: GarageMode::Local,
            k8s: Some(k8s),
        })
    }

    /// Creates a new garage client in remote mode.
    ///
    /// Note: Remote mode is not yet implemented.
    #[must_use]
    pub fn remote(endpoint: impl Into<String>) -> Self {
        Self {
            mode: GarageMode::remote(endpoint),
            k8s: None,
        }
    }

    /// Creates a garage client from an existing K8s client (for testing).
    #[must_use]
    pub fn from_k8s(k8s: K8sClient) -> Self {
        Self {
            mode: GarageMode::Local,
            k8s: Some(k8s),
        }
    }

    /// Returns the operating mode.
    #[must_use]
    pub fn mode(&self) -> &GarageMode {
        &self.mode
    }

    /// Lists all garages.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    #[instrument(skip(self))]
    pub async fn list(&self) -> Result<Vec<GarageInfo>> {
        match &self.mode {
            GarageMode::Local => self.list_local().await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Opens (creates) a new garage with the given name.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-friendly name for the garage.
    /// * `owner` - Optional owner identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if a garage with this name already exists or creation fails.
    #[instrument(skip(self, name), fields(garage.name = %name))]
    pub async fn open(&self, name: &str, owner: Option<&str>) -> Result<GarageInfo> {
        match &self.mode {
            GarageMode::Local => self.open_local(name, owner).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Closes (deletes) a garage by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist or deletion fails.
    #[instrument(skip(self), fields(garage.id = %id))]
    pub async fn close(&self, id: &GarageId) -> Result<()> {
        match &self.mode {
            GarageMode::Local => self.close_local(id).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Lists garages in local mode.
    async fn list_local(&self) -> Result<Vec<GarageInfo>> {
        let k8s = self.k8s.as_ref().expect("k8s client required for local mode");

        debug!("listing garages from K8s");
        let namespaces = k8s.list_namespaces(Some(&Labels::garage_selector())).await?;

        let mut garages = Vec::new();
        for ns in namespaces {
            if let Some(info) = namespace_to_garage_info(&ns) {
                garages.push(info);
            }
        }

        // Sort by creation time (newest first)
        garages.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(garages)
    }

    /// Opens a garage in local mode.
    async fn open_local(&self, name: &str, owner: Option<&str>) -> Result<GarageInfo> {
        let k8s = self.k8s.as_ref().expect("k8s client required for local mode");

        // Check if garage with this name already exists
        let existing = self.list_local().await?;
        if existing.iter().any(|g| g.name == name) {
            return Err(Error::GarageExists(name.to_string()));
        }

        // Create new garage info
        let mut info = GarageInfo::new(name);
        if let Some(owner) = owner {
            info = info.with_owner(owner);
        }

        // Create namespace with labels
        let labels = Labels::for_garage(
            &info.id.to_string(),
            &info.name,
            info.owner.as_deref(),
        );

        debug!(namespace = %info.namespace, "creating garage namespace");
        k8s.create_namespace(&info.namespace, labels).await?;

        Ok(info)
    }

    /// Closes a garage in local mode.
    async fn close_local(&self, id: &GarageId) -> Result<()> {
        let k8s = self.k8s.as_ref().expect("k8s client required for local mode");

        // Find the garage by ID
        let garages = self.list_local().await?;
        let garage = garages
            .iter()
            .find(|g| &g.id == id)
            .ok_or_else(|| Error::GarageNotFound(id.to_string()))?;

        debug!(namespace = %garage.namespace, "deleting garage namespace");
        k8s.delete_namespace(&garage.namespace).await?;

        Ok(())
    }
}

/// Converts a K8s namespace to `GarageInfo`.
///
/// Returns `None` if the namespace doesn't have the required moto labels.
fn namespace_to_garage_info(ns: &k8s_openapi::api::core::v1::Namespace) -> Option<GarageInfo> {
    let metadata = &ns.metadata;
    let labels = metadata.labels.as_ref()?;

    // Required labels
    let id_str = labels.get(Labels::ID)?;
    let name = labels.get(Labels::NAME)?;

    let id: GarageId = id_str.parse().ok()?;
    let namespace = metadata.name.clone()?;

    // Optional owner
    let owner = labels.get(Labels::OWNER).cloned();

    // Creation time from K8s metadata
    let created_at = metadata
        .creation_timestamp
        .as_ref()
        .map(|ts| ts.0)
        .unwrap_or_else(Utc::now);

    // Determine state based on namespace status
    let state = match &ns.status {
        Some(status) => match status.phase.as_deref() {
            Some("Active") => GarageState::Ready,
            Some("Terminating") => GarageState::Terminating,
            _ => GarageState::Pending,
        },
        None => GarageState::Pending,
    };

    Some(GarageInfo {
        id,
        name: name.clone(),
        namespace,
        state,
        created_at,
        expires_at: None, // TTL not implemented yet
        owner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_client_mode() {
        let client = GarageClient::remote("https://club.example.com");
        assert!(client.mode().is_remote());
        assert_eq!(client.mode().endpoint(), Some("https://club.example.com"));
    }

    // Integration tests require a running K8s cluster
    // They are marked #[ignore] and can be run with:
    // cargo test --package moto-garage -- --ignored

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn local_client_creation() {
        let client = GarageClient::local().await;
        assert!(client.is_ok(), "Failed to create client: {:?}", client.err());
        assert!(client.unwrap().mode().is_local());
    }

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn list_empty() {
        let client = GarageClient::local().await.expect("client creation failed");
        let garages = client.list().await;
        assert!(garages.is_ok(), "Failed to list garages: {:?}", garages.err());
    }
}
