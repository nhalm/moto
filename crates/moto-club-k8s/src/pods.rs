//! Garage pod lifecycle management.
//!
//! Provides operations for deploying, monitoring, and deleting garage pods
//! within garage namespaces.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{
    Container, EmptyDirVolumeSource, KeyToPath, Pod, PodSpec, Probe, ResourceRequirements,
    SecretVolumeSource, SecurityContext, TCPSocketAction, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::secrets::{AUTHORIZED_KEYS_KEY, SSH_KEYS_SECRET_NAME};
use kube::api::{Api, DeleteParams, ObjectMeta, PostParams};
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{Labels, PodOps, Result};

use crate::GarageK8s;

/// Default pod name for the dev container in a garage.
pub const DEV_CONTAINER_POD_NAME: &str = "dev-container";

/// Port for ttyd terminal daemon (WebSocket).
/// The container runs ttyd on this port for terminal access.
pub const TTYD_PORT: i32 = 7681;

/// Repository configuration for cloning.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoConfig {
    /// Repository clone URL (e.g., `https://github.com/org/repo.git`).
    pub url: String,
    /// Branch to checkout.
    pub branch: String,
    /// Directory name under /workspace/ (derived from URL if not set).
    pub name: String,
}

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
    /// Optional repository to clone on startup.
    pub repo: Option<RepoConfig>,
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
    /// - `moto.dev/garage-id: {id}`
    /// - `moto.dev/garage-name: {name}`
    /// - `moto.dev/owner: {owner}`
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or pod creation fails.
    fn deploy_garage_pod(&self, input: &GaragePodInput)
    -> impl Future<Output = Result<Pod>> + Send;

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
            input.repo.as_ref(),
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

        api.get(DEV_CONTAINER_POD_NAME).await.map_err(|e| {
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
    repo: Option<&RepoConfig>,
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

    // Volume mount for SSH authorized_keys
    let ssh_volume_mount = VolumeMount {
        name: "ssh-keys".to_string(),
        mount_path: "/home/moto/.ssh".to_string(),
        read_only: Some(true),
        ..Default::default()
    };

    // Volume mount for workspace (shared between init container and main container)
    let workspace_volume_mount = VolumeMount {
        name: "workspace".to_string(),
        mount_path: "/workspace".to_string(),
        ..Default::default()
    };

    // Readiness probe: TCP check on ttyd port
    // Container is ready when ttyd is accepting connections
    let readiness_probe = Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(TTYD_PORT),
            ..Default::default()
        }),
        // Initial delay to allow ttyd to start
        initial_delay_seconds: Some(2),
        // Check every 5 seconds
        period_seconds: Some(5),
        // Fail after 3 consecutive failures
        failure_threshold: Some(3),
        // Succeed after 1 successful check
        success_threshold: Some(1),
        // Timeout for the check
        timeout_seconds: Some(2),
        ..Default::default()
    };

    let container = Container {
        name: "dev".to_string(),
        image: Some(image.to_string()),
        image_pull_policy: Some("Always".to_string()),
        resources: Some(resources),
        security_context: Some(security_context.clone()),
        env: Some(env_vars),
        volume_mounts: Some(vec![ssh_volume_mount, workspace_volume_mount.clone()]),
        // Use container's default entrypoint (garage-entrypoint)
        // which starts ttyd for terminal access
        readiness_probe: Some(readiness_probe),
        ..Default::default()
    };

    // Volume for SSH keys Secret
    // Mount authorized_keys file with mode 0600 (octal 384)
    let ssh_volume = Volume {
        name: "ssh-keys".to_string(),
        secret: Some(SecretVolumeSource {
            secret_name: Some(SSH_KEYS_SECRET_NAME.to_string()),
            default_mode: Some(0o600),
            items: Some(vec![KeyToPath {
                key: AUTHORIZED_KEYS_KEY.to_string(),
                path: "authorized_keys".to_string(),
                mode: Some(0o600),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Volume for workspace (shared between init container and main container)
    let workspace_volume = Volume {
        name: "workspace".to_string(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Default::default()
    };

    let volumes = vec![ssh_volume, workspace_volume];

    // Build init containers for repo cloning if configured
    let init_containers = repo.map(|repo_config| {
        build_repo_clone_init_container(
            image,
            repo_config,
            &workspace_volume_mount,
            &security_context,
        )
    });

    Pod {
        metadata: ObjectMeta {
            name: Some(DEV_CONTAINER_POD_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PodSpec {
            init_containers: init_containers.map(|c| vec![c]),
            containers: vec![container],
            volumes: Some(volumes),
            restart_policy: Some("Never".to_string()),
            // Use default service account (will get garage-specific SA later)
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds an init container for cloning a repository.
///
/// The init container:
/// 1. Clones the repo with `--depth=1` for faster cloning
/// 2. Checks out the specified branch
/// 3. Retries up to 3 times on failure
fn build_repo_clone_init_container(
    image: &str,
    repo: &RepoConfig,
    workspace_mount: &VolumeMount,
    security_context: &SecurityContext,
) -> Container {
    // Clone script with retry logic per spec (3 retries)
    let clone_script = format!(
        r#"#!/bin/sh
set -e

REPO_URL="${{REPO_URL}}"
REPO_BRANCH="${{REPO_BRANCH}}"
REPO_NAME="${{REPO_NAME}}"
MAX_RETRIES=3
RETRY_COUNT=0

echo "Cloning $REPO_URL (branch: $REPO_BRANCH) to /workspace/$REPO_NAME/"

while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if git clone --depth=1 -b "$REPO_BRANCH" "$REPO_URL" "/workspace/$REPO_NAME"; then
        echo "Clone successful"
        # Set working directory marker for ttyd
        echo "/workspace/$REPO_NAME" > /workspace/.workdir
        exit 0
    fi
    RETRY_COUNT=$((RETRY_COUNT + 1))
    echo "Clone failed, attempt $RETRY_COUNT of $MAX_RETRIES"
    sleep 2
done

echo "Clone failed after $MAX_RETRIES attempts"
exit 1
"#
    );

    // Environment variables per spec (lines 112-115)
    let env_vars = vec![
        k8s_openapi::api::core::v1::EnvVar {
            name: "REPO_URL".to_string(),
            value: Some(repo.url.clone()),
            ..Default::default()
        },
        k8s_openapi::api::core::v1::EnvVar {
            name: "REPO_BRANCH".to_string(),
            value: Some(repo.branch.clone()),
            ..Default::default()
        },
        k8s_openapi::api::core::v1::EnvVar {
            name: "REPO_NAME".to_string(),
            value: Some(repo.name.clone()),
            ..Default::default()
        },
    ];

    Container {
        name: "clone-repo".to_string(),
        image: Some(image.to_string()),
        image_pull_policy: Some("Always".to_string()),
        command: Some(vec!["/bin/sh".to_string(), "-c".to_string()]),
        args: Some(vec![clone_script]),
        env: Some(env_vars),
        volume_mounts: Some(vec![workspace_mount.clone()]),
        security_context: Some(security_context.clone()),
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
            repo: None,
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
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        // Check metadata
        assert_eq!(pod.metadata.name, Some(DEV_CONTAINER_POD_NAME.to_string()));
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

    #[test]
    fn build_pod_has_ssh_keys_volume() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();

        // Check volumes are defined (ssh-keys + workspace)
        let volumes = spec.volumes.as_ref().unwrap();
        assert_eq!(volumes.len(), 2);
        let ssh_volume = volumes.iter().find(|v| v.name == "ssh-keys").unwrap();

        let secret = ssh_volume.secret.as_ref().unwrap();
        assert_eq!(secret.secret_name, Some(SSH_KEYS_SECRET_NAME.to_string()));
        assert_eq!(secret.default_mode, Some(0o600));

        // Check items mapping
        let items = secret.items.as_ref().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, AUTHORIZED_KEYS_KEY);
        assert_eq!(items[0].path, "authorized_keys");
        assert_eq!(items[0].mode, Some(0o600));

        // Check container has volume mounts (ssh + workspace)
        let container = &spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 2);
        let ssh_mount = mounts.iter().find(|m| m.name == "ssh-keys").unwrap();
        assert_eq!(ssh_mount.mount_path, "/home/moto/.ssh");
        assert_eq!(ssh_mount.read_only, Some(true));
    }

    #[test]
    fn build_pod_has_ttyd_readiness_probe() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();
        let container = &spec.containers[0];

        // Check readiness probe exists
        let probe = container
            .readiness_probe
            .as_ref()
            .expect("readiness probe should be set");

        // Check it's a TCP socket probe
        let tcp_socket = probe.tcp_socket.as_ref().expect("tcp_socket should be set");
        assert_eq!(tcp_socket.port, IntOrString::Int(TTYD_PORT));

        // Check probe timing settings
        assert_eq!(probe.initial_delay_seconds, Some(2));
        assert_eq!(probe.period_seconds, Some(5));
        assert_eq!(probe.failure_threshold, Some(3));
        assert_eq!(probe.success_threshold, Some(1));
        assert_eq!(probe.timeout_seconds, Some(2));
    }

    #[test]
    fn build_pod_uses_default_entrypoint() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();
        let container = &spec.containers[0];

        // Container should NOT override command - uses image's default (garage-entrypoint)
        assert!(
            container.command.is_none(),
            "container should use image's default entrypoint"
        );
    }

    #[test]
    fn build_pod_with_repo_has_init_container() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let repo = RepoConfig {
            url: "https://github.com/example/repo.git".to_string(),
            branch: "main".to_string(),
            name: "repo".to_string(),
        };
        let pod = build_dev_container_pod(
            "moto-garage-abc12345",
            "test:latest",
            "main",
            labels,
            Some(&repo),
        );

        let spec = pod.spec.as_ref().unwrap();

        // Check init container exists
        let init_containers = spec
            .init_containers
            .as_ref()
            .expect("init_containers should be set");
        assert_eq!(init_containers.len(), 1);

        let init = &init_containers[0];
        assert_eq!(init.name, "clone-repo");
        assert_eq!(init.image, Some("test:latest".to_string()));

        // Check env vars are set correctly
        let env = init.env.as_ref().unwrap();
        let repo_url = env.iter().find(|e| e.name == "REPO_URL").unwrap();
        assert_eq!(
            repo_url.value,
            Some("https://github.com/example/repo.git".to_string())
        );

        let repo_branch = env.iter().find(|e| e.name == "REPO_BRANCH").unwrap();
        assert_eq!(repo_branch.value, Some("main".to_string()));

        let repo_name = env.iter().find(|e| e.name == "REPO_NAME").unwrap();
        assert_eq!(repo_name.value, Some("repo".to_string()));

        // Check workspace volume mount
        let mounts = init.volume_mounts.as_ref().unwrap();
        let workspace_mount = mounts.iter().find(|m| m.name == "workspace").unwrap();
        assert_eq!(workspace_mount.mount_path, "/workspace");
    }

    #[test]
    fn build_pod_without_repo_has_no_init_container() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();

        // No init containers when repo is None
        assert!(spec.init_containers.is_none());
    }

    #[test]
    fn build_pod_has_workspace_volume() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();

        // Check workspace volume is defined
        let volumes = spec.volumes.as_ref().unwrap();
        let workspace_volume = volumes.iter().find(|v| v.name == "workspace").unwrap();
        assert!(workspace_volume.empty_dir.is_some());

        // Check main container has workspace mount
        let container = &spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();
        let workspace_mount = mounts.iter().find(|m| m.name == "workspace").unwrap();
        assert_eq!(workspace_mount.mount_path, "/workspace");
    }
}
