//! Kubernetes interactions for moto-club.
//!
//! This crate provides moto-club-specific K8s operations built on top of `moto-k8s`:
//! - Garage namespace management (create, delete, list)
//! - Garage pod lifecycle (deploy, get status, delete)
//! - Resource quotas and network policies
//!
//! # Example
//!
//! ```ignore
//! use moto_club_k8s::{GarageK8s, GarageNamespaceInput};
//! use moto_club_types::GarageId;
//! use moto_k8s::K8sClient;
//!
//! let client = K8sClient::new().await?;
//! let garage_k8s = GarageK8s::new(client);
//!
//! // Create a garage namespace
//! let input = GarageNamespaceInput {
//!     id: GarageId::new(),
//!     name: "bold-mongoose".to_string(),
//!     owner: "nick".to_string(),
//!     expires_at: None,
//! };
//! let namespace = garage_k8s.create_namespace(&input).await?;
//!
//! // Deploy a garage pod
//! let pod = garage_k8s.deploy_pod(&input, "ghcr.io/moto/dev:latest").await?;
//! ```

mod namespace;
mod network_policy;
mod pods;
mod pvc;
mod resource_quota;

pub use namespace::{GarageNamespaceInput, GarageNamespaceOps};
pub use network_policy::{GARAGE_ISOLATION_POLICY_NAME, GarageNetworkPolicyOps};
pub use pods::{DEV_CONTAINER_POD_NAME, GaragePodInput, GaragePodOps, GaragePodStatus, RepoConfig};
pub use pvc::{GarageWorkspacePvcOps, WORKSPACE_PVC_NAME};
pub use resource_quota::{GARAGE_QUOTA_NAME, GarageResourceQuotaOps};

use moto_k8s::K8sClient;

/// High-level Kubernetes operations for garages.
///
/// Wraps `K8sClient` and provides moto-club-specific operations
/// for garage namespace and pod management.
#[derive(Clone)]
pub struct GarageK8s {
    client: K8sClient,
    dev_container_image: String,
}

impl GarageK8s {
    /// Creates a new `GarageK8s` with the given client and default image.
    #[must_use]
    pub fn new(client: K8sClient) -> Self {
        Self {
            client,
            dev_container_image: "ghcr.io/moto-dev/moto-garage:latest".to_string(),
        }
    }

    /// Creates a new `GarageK8s` with a custom dev container image.
    #[must_use]
    pub fn with_image(client: K8sClient, image: impl Into<String>) -> Self {
        Self {
            client,
            dev_container_image: image.into(),
        }
    }

    /// Returns a reference to the underlying `K8sClient`.
    #[must_use]
    pub const fn client(&self) -> &K8sClient {
        &self.client
    }

    /// Returns the dev container image.
    #[must_use]
    pub fn dev_container_image(&self) -> &str {
        &self.dev_container_image
    }
}
