//! Workspace PVC management for garages.
//!
//! Per garage-isolation.md spec, workspace uses a `PersistentVolumeClaim`
//! so uncommitted work survives pod restarts.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{
    PersistentVolumeClaim, PersistentVolumeClaimSpec, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::ObjectMeta;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{Labels, PvcOps, Result};

use crate::GarageK8s;

/// Name of the workspace PVC.
pub const WORKSPACE_PVC_NAME: &str = "workspace-pvc";

/// Default storage size for workspace PVC.
const WORKSPACE_STORAGE_SIZE: &str = "10Gi";

/// Trait for garage workspace PVC operations.
pub trait GarageWorkspacePvcOps {
    /// Creates a workspace PVC in the garage namespace.
    ///
    /// The PVC is named `workspace-pvc` and uses the default storage class.
    /// Storage size is 10Gi (sufficient for most dev workloads).
    ///
    /// # Errors
    ///
    /// Returns an error if the PVC already exists or creation fails.
    fn create_workspace_pvc(
        &self,
        id: &GarageId,
        garage_name: &str,
        owner: &str,
    ) -> impl Future<Output = Result<PersistentVolumeClaim>> + Send;

    /// Checks if workspace PVC exists in the garage namespace.
    fn workspace_pvc_exists(&self, id: &GarageId) -> impl Future<Output = Result<bool>> + Send;
}

impl GarageWorkspacePvcOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id, garage_name = %garage_name))]
    async fn create_workspace_pvc(
        &self,
        id: &GarageId,
        garage_name: &str,
        owner: &str,
    ) -> Result<PersistentVolumeClaim> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "creating workspace PVC");

        let labels = Labels::for_garage(&id.to_string(), garage_name, Some(owner), None, None);

        let pvc = build_workspace_pvc(&namespace, labels);

        self.client.create_pvc(&namespace, &pvc).await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn workspace_pvc_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace = format!("moto-garage-{}", id.short());
        self.client.pvc_exists(&namespace, WORKSPACE_PVC_NAME).await
    }
}

/// Builds a workspace PVC spec per garage-isolation.md.
fn build_workspace_pvc(namespace: &str, labels: BTreeMap<String, String>) -> PersistentVolumeClaim {
    let mut requests = BTreeMap::new();
    requests.insert(
        "storage".to_string(),
        Quantity(WORKSPACE_STORAGE_SIZE.to_string()),
    );

    PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(WORKSPACE_PVC_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".to_string()]),
            resources: Some(VolumeResourceRequirements {
                requests: Some(requests),
                ..Default::default()
            }),
            // Use default storage class (nil means default)
            storage_class_name: None,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_workspace_pvc_structure() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pvc = build_workspace_pvc("moto-garage-abc12345", labels);

        // Check metadata
        assert_eq!(pvc.metadata.name, Some(WORKSPACE_PVC_NAME.to_string()));
        assert_eq!(
            pvc.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check spec
        let spec = pvc.spec.as_ref().unwrap();
        assert_eq!(spec.access_modes, Some(vec!["ReadWriteOnce".to_string()]));

        // Check resources
        let resources = spec.resources.as_ref().unwrap();
        let requests = resources.requests.as_ref().unwrap();
        assert_eq!(
            requests.get("storage"),
            Some(&Quantity(WORKSPACE_STORAGE_SIZE.to_string()))
        );

        // Default storage class (nil)
        assert!(spec.storage_class_name.is_none());
    }

    #[test]
    fn workspace_pvc_has_labels() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pvc = build_workspace_pvc("moto-garage-abc12345", labels);

        let pvc_labels = pvc.metadata.labels.as_ref().unwrap();
        assert_eq!(pvc_labels.get(Labels::TYPE), Some(&"garage".to_string()));
        assert_eq!(
            pvc_labels.get(Labels::GARAGE_ID),
            Some(&"abc-123".to_string())
        );
        assert_eq!(
            pvc_labels.get(Labels::GARAGE_NAME),
            Some(&"test".to_string())
        );
        assert_eq!(pvc_labels.get(Labels::OWNER), Some(&"alice".to_string()));
    }
}
