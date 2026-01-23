//! Garage pod lifecycle management.
//!
//! Provides operations for deploying, monitoring, and deleting garage pods
//! within garage namespaces.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{
    Container, Pod, PodSpec, ResourceRequirements, SecurityContext,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::{Api, DeleteParams, ObjectMeta, PostParams};
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{Labels, PodOps, Result};

use crate::GarageK8s;

/// Default pod name for the dev container in a garage.
pub const DEV_CONTAINER_POD_NAME: &str = "dev-container";

/// Input for creating a garage pod.
#[derive(Debug, Clone)]
pub struct GaragePodInput {
    /// Unique garage identifier.
    pub id: GarageId,
    /// Human-friendly garage name.
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Git branch to clone.
    pub branch: String,
    /// Optional custom image (overrides default).
    pub image: Option<String>,
}

impl GaragePodInput {
    /// Returns the K8s namespace name for this garage.
    #[must_use]
    pub fn namespace_name(&self) -> String {
        format!("moto-garage-{}", self.id.short())
    }
}

/// Status of a garage pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GaragePodStatus {
    /// Pod is pending (waiting to be scheduled or pulling images).
    Pending,
    /// Pod is running but containers may not be ready.
    Running,
    /// Pod is running and all containers are ready.
    Ready,
    /// Pod has succeeded (completed).
    Succeeded,
    /// Pod has failed.
    Failed,
    /// Pod status is unknown.
    Unknown,
}

impl std::fmt::Display for GaragePodStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Ready => "Ready",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
        };
        write!(f, "{s}")
    }
}

/// Trait for garage pod operations.
pub trait GaragePodOps {
    /// Deploys a dev container pod in the garage namespace.
    ///
    /// The pod is named `dev-container` and includes labels:
    /// - `moto.dev/type: garage`
    /// - `moto.dev/id: {id}`
    /// - `moto.dev/name: {name}`
    /// - `moto.dev/owner: {owner}`
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or pod creation fails.
    fn deploy_garage_pod(&self, input: &GaragePodInput) -> impl Future<Output = Result<Pod>> + Send;

    /// Gets the status of a garage pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod doesn't exist or the operation fails.
    fn get_garage_pod_status(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<GaragePodStatus>> + Send;

    /// Gets the garage pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod doesn't exist or the operation fails.
    fn get_garage_pod(&self, id: &GarageId) -> impl Future<Output = Result<Pod>> + Send;

    /// Deletes the garage pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the pod doesn't exist or deletion fails.
    fn delete_garage_pod(&self, id: &GarageId) -> impl Future<Output = Result<()>> + Send;

    /// Lists all pods in a garage namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or list fails.
    fn list_garage_pods(&self, id: &GarageId) -> impl Future<Output = Result<Vec<Pod>>> + Send;
}

impl GaragePodOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %input.id, garage_name = %input.name))]
    async fn deploy_garage_pod(&self, input: &GaragePodInput) -> Result<Pod> {
        let namespace = input.namespace_name();
        let image = input
            .image
            .as_deref()
            .unwrap_or_else(|| self.dev_container_image());

        debug!(namespace = %namespace, image = %image, "deploying garage pod");

        let labels = Labels::for_garage(
            &input.id.to_string(),
            &input.name,
            Some(&input.owner),
            None,
            None,
        );

        let pod = build_dev_container_pod(
            &namespace,
            image,
            &input.branch,
            labels,
        );

        let api: Api<Pod> = Api::namespaced(self.client.inner().clone(), &namespace);
        let created = api
            .create(&PostParams::default(), &pod)
            .await
            .map_err(moto_k8s::Error::NamespaceCreate)?;

        debug!(pod = %DEV_CONTAINER_POD_NAME, "garage pod created");
        Ok(created)
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn get_garage_pod_status(&self, id: &GarageId) -> Result<GaragePodStatus> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "getting garage pod status");
        let pods = self.client.list_pods(&namespace, None).await?;

        if pods.is_empty() {
            return Ok(GaragePodStatus::Unknown);
        }

        // Find the dev-container pod
        let pod = pods
            .iter()
            .find(|p| p.metadata.name.as_deref() == Some(DEV_CONTAINER_POD_NAME));

        Ok(pod.map_or(GaragePodStatus::Unknown, extract_pod_status))
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn get_garage_pod(&self, id: &GarageId) -> Result<Pod> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "getting garage pod");
        let api: Api<Pod> = Api::namespaced(self.client.inner().clone(), &namespace);

        api.get(DEV_CONTAINER_POD_NAME)
            .await
            .map_err(|e| {
                if is_not_found(&e) {
                    moto_k8s::Error::PodNotFound(format!(
                        "{DEV_CONTAINER_POD_NAME} in namespace {namespace}"
                    ))
                } else {
                    moto_k8s::Error::PodList(e)
                }
            })
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn delete_garage_pod(&self, id: &GarageId) -> Result<()> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "deleting garage pod");
        let api: Api<Pod> = Api::namespaced(self.client.inner().clone(), &namespace);

        api.delete(DEV_CONTAINER_POD_NAME, &DeleteParams::default())
            .await
            .map_err(|e| {
                if is_not_found(&e) {
                    moto_k8s::Error::PodNotFound(format!(
                        "{DEV_CONTAINER_POD_NAME} in namespace {namespace}"
                    ))
                } else {
                    moto_k8s::Error::PodList(e)
                }
            })?;

        Ok(())
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn list_garage_pods(&self, id: &GarageId) -> Result<Vec<Pod>> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "listing garage pods");
        self.client.list_pods(&namespace, None).await
    }
}

/// Builds a dev container pod spec.
fn build_dev_container_pod(
    namespace: &str,
    image: &str,
    branch: &str,
    labels: BTreeMap<String, String>,
) -> Pod {
    // Resource requirements
    let mut requests = BTreeMap::new();
    requests.insert("cpu".to_string(), Quantity("100m".to_string()));
    requests.insert("memory".to_string(), Quantity("256Mi".to_string()));

    let mut limits = BTreeMap::new();
    limits.insert("cpu".to_string(), Quantity("2".to_string()));
    limits.insert("memory".to_string(), Quantity("4Gi".to_string()));

    let resources = ResourceRequirements {
        requests: Some(requests),
        limits: Some(limits),
        ..Default::default()
    };

    // Security context - run as non-root
    let security_context = SecurityContext {
        run_as_non_root: Some(true),
        run_as_user: Some(1000),
        run_as_group: Some(1000),
        ..Default::default()
    };

    // Environment variables
    let env_vars = vec![
        k8s_openapi::api::core::v1::EnvVar {
            name: "MOTO_GARAGE_BRANCH".to_string(),
            value: Some(branch.to_string()),
            ..Default::default()
        },
        k8s_openapi::api::core::v1::EnvVar {
            name: "MOTO_GARAGE_NAMESPACE".to_string(),
            value: Some(namespace.to_string()),
            ..Default::default()
        },
    ];

    let container = Container {
        name: "dev".to_string(),
        image: Some(image.to_string()),
        image_pull_policy: Some("Always".to_string()),
        resources: Some(resources),
        security_context: Some(security_context),
        env: Some(env_vars),
        // Keep the container running
        command: Some(vec!["sleep".to_string(), "infinity".to_string()]),
        ..Default::default()
    };

    Pod {
        metadata: ObjectMeta {
            name: Some(DEV_CONTAINER_POD_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![container],
            restart_policy: Some("Never".to_string()),
            // Use default service account (will get garage-specific SA later)
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Extracts pod status from a Pod object.
fn extract_pod_status(pod: &Pod) -> GaragePodStatus {
    let Some(status) = &pod.status else {
        return GaragePodStatus::Unknown;
    };

    let phase = status.phase.as_deref().unwrap_or("Unknown");

    match phase {
        "Pending" => GaragePodStatus::Pending,
        "Running" => {
            // Check if containers are ready
            let container_statuses = status.container_statuses.as_deref().unwrap_or_default();
            let all_ready = container_statuses.iter().all(|cs| cs.ready);
            if all_ready && !container_statuses.is_empty() {
                GaragePodStatus::Ready
            } else {
                GaragePodStatus::Running
            }
        }
        "Succeeded" => GaragePodStatus::Succeeded,
        "Failed" => GaragePodStatus::Failed,
        _ => GaragePodStatus::Unknown,
    }
}

/// Checks if a kube error is a "not found" error.
const fn is_not_found(e: &kube::Error) -> bool {
    matches!(
        e,
        kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_input_namespace_name() {
        let input = GaragePodInput {
            id: GarageId::new(),
            name: "my-project".to_string(),
            owner: "alice".to_string(),
            branch: "main".to_string(),
            image: None,
        };

        let ns = input.namespace_name();
        assert!(ns.starts_with("moto-garage-"));
        assert_eq!(ns.len(), "moto-garage-".len() + 8);
    }

    #[test]
    fn pod_status_display() {
        assert_eq!(GaragePodStatus::Pending.to_string(), "Pending");
        assert_eq!(GaragePodStatus::Running.to_string(), "Running");
        assert_eq!(GaragePodStatus::Ready.to_string(), "Ready");
        assert_eq!(GaragePodStatus::Succeeded.to_string(), "Succeeded");
        assert_eq!(GaragePodStatus::Failed.to_string(), "Failed");
        assert_eq!(GaragePodStatus::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn build_pod_has_correct_structure() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod = build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels);

        // Check metadata
        assert_eq!(
            pod.metadata.name,
            Some(DEV_CONTAINER_POD_NAME.to_string())
        );
        assert_eq!(
            pod.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check spec
        let spec = pod.spec.as_ref().unwrap();
        assert_eq!(spec.containers.len(), 1);

        let container = &spec.containers[0];
        assert_eq!(container.name, "dev");
        assert_eq!(container.image, Some("test:latest".to_string()));

        // Check environment variables
        let env = container.env.as_ref().unwrap();
        let branch_env = env.iter().find(|e| e.name == "MOTO_GARAGE_BRANCH");
        assert_eq!(branch_env.unwrap().value, Some("main".to_string()));
    }
}
