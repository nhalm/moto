//! Garage `ResourceQuota` management.
//!
//! Creates the garage-quota `ResourceQuota` per garage-isolation.md spec:
//! - requests.cpu: "4"
//! - requests.memory: 8Gi
//! - limits.cpu: "4"
//! - limits.memory: 8Gi
//! - pods: "10" (garage + supporting services)
//! - persistentvolumeclaims: "1"
//! - services: "10"

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{ResourceQuota, ResourceQuotaSpec};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::ObjectMeta;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{ResourceQuotaOps, Result};

use crate::GarageK8s;

/// Name of the garage `ResourceQuota`.
pub const GARAGE_QUOTA_NAME: &str = "garage-quota";

/// Trait for garage `ResourceQuota` operations.
pub trait GarageResourceQuotaOps {
    /// Creates the garage-quota `ResourceQuota` in the garage namespace.
    ///
    /// The quota (per garage-isolation.md spec):
    /// - requests.cpu: "4"
    /// - requests.memory: 8Gi
    /// - limits.cpu: "4"
    /// - limits.memory: 8Gi
    /// - pods: "10" (garage + supporting services)
    /// - persistentvolumeclaims: "1"
    /// - services: "10"
    ///
    /// # Errors
    ///
    /// Returns an error if the `ResourceQuota` already exists or creation fails.
    fn create_garage_resource_quota(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<ResourceQuota>> + Send;

    /// Checks if the garage-quota `ResourceQuota` exists.
    fn garage_resource_quota_exists(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<bool>> + Send;
}

impl GarageResourceQuotaOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id))]
    async fn create_garage_resource_quota(&self, id: &GarageId) -> Result<ResourceQuota> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "creating garage-quota ResourceQuota");

        let quota = build_garage_quota(&namespace);

        self.client()
            .create_resource_quota(&namespace, &quota)
            .await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn garage_resource_quota_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace = format!("moto-garage-{}", id.short());
        self.client()
            .resource_quota_exists(&namespace, GARAGE_QUOTA_NAME)
            .await
    }
}

/// Builds the garage-quota `ResourceQuota` per garage-isolation.md spec.
fn build_garage_quota(namespace: &str) -> ResourceQuota {
    let mut hard = BTreeMap::new();

    // CPU and memory limits per spec lines 271-276
    hard.insert("requests.cpu".to_string(), Quantity("4".to_string()));
    hard.insert("requests.memory".to_string(), Quantity("8Gi".to_string()));
    hard.insert("limits.cpu".to_string(), Quantity("4".to_string()));
    hard.insert("limits.memory".to_string(), Quantity("8Gi".to_string()));

    // Pod and resource counts per spec lines 277-279
    hard.insert("pods".to_string(), Quantity("10".to_string()));
    hard.insert(
        "persistentvolumeclaims".to_string(),
        Quantity("1".to_string()),
    );
    hard.insert("services".to_string(), Quantity("10".to_string()));

    ResourceQuota {
        metadata: ObjectMeta {
            name: Some(GARAGE_QUOTA_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(ResourceQuotaSpec {
            hard: Some(hard),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_garage_quota_structure() {
        let quota = build_garage_quota("moto-garage-abc12345");

        // Check metadata
        assert_eq!(quota.metadata.name, Some(GARAGE_QUOTA_NAME.to_string()));
        assert_eq!(
            quota.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check spec
        let spec = quota.spec.as_ref().unwrap();
        let hard = spec.hard.as_ref().unwrap();

        // CPU and memory
        assert_eq!(hard.get("requests.cpu"), Some(&Quantity("4".to_string())));
        assert_eq!(
            hard.get("requests.memory"),
            Some(&Quantity("8Gi".to_string()))
        );
        assert_eq!(hard.get("limits.cpu"), Some(&Quantity("4".to_string())));
        assert_eq!(
            hard.get("limits.memory"),
            Some(&Quantity("8Gi".to_string()))
        );

        // Pod and resource counts
        assert_eq!(hard.get("pods"), Some(&Quantity("10".to_string())));
        assert_eq!(
            hard.get("persistentvolumeclaims"),
            Some(&Quantity("1".to_string()))
        );
        assert_eq!(hard.get("services"), Some(&Quantity("10".to_string())));
    }

    #[test]
    fn garage_quota_has_all_required_limits() {
        let quota = build_garage_quota("test-ns");
        let spec = quota.spec.as_ref().unwrap();
        let hard = spec.hard.as_ref().unwrap();

        // Per garage-isolation.md spec, all these fields must be present
        assert!(hard.contains_key("requests.cpu"));
        assert!(hard.contains_key("requests.memory"));
        assert!(hard.contains_key("limits.cpu"));
        assert!(hard.contains_key("limits.memory"));
        assert!(hard.contains_key("pods"));
        assert!(hard.contains_key("persistentvolumeclaims"));
        assert!(hard.contains_key("services"));

        // Should have exactly 7 limits
        assert_eq!(hard.len(), 7);
    }
}
