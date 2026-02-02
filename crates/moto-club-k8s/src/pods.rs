//! Garage pod lifecycle management.
//!
//! Provides operations for deploying, monitoring, and deleting garage pods
//! within garage namespaces.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{
    Capabilities, Container, EmptyDirVolumeSource, PersistentVolumeClaimVolumeSource, Pod,
    PodSecurityContext, PodSpec, Probe, ResourceRequirements, SeccompProfile, SecurityContext,
    TCPSocketAction, Volume, VolumeMount,
};

use crate::pvc::WORKSPACE_PVC_NAME;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
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
    limits.insert("cpu".to_string(), Quantity("3".to_string()));
    limits.insert("memory".to_string(), Quantity("7Gi".to_string()));

    let resources = ResourceRequirements {
        requests: Some(requests),
        limits: Some(limits),
        ..Default::default()
    };

    // Security context per garage-isolation.md spec:
    // - Root inside container (container IS the sandbox)
    // - Minimal capabilities (drop all, add only what's needed for dev work)
    // - Read-only root filesystem (writes go to volumes)
    // - No privilege escalation
    // - RuntimeDefault seccomp profile
    let security_context = SecurityContext {
        run_as_user: Some(0),
        run_as_group: Some(0),
        allow_privilege_escalation: Some(false),
        read_only_root_filesystem: Some(true),
        seccomp_profile: Some(SeccompProfile {
            type_: "RuntimeDefault".to_string(),
            ..Default::default()
        }),
        capabilities: Some(Capabilities {
            drop: Some(vec!["ALL".to_string()]),
            add: Some(vec![
                "CHOWN".to_string(),            // Change file ownership
                "DAC_OVERRIDE".to_string(),     // Bypass file permission checks
                "FOWNER".to_string(),           // Bypass ownership checks
                "SETGID".to_string(),           // Set group ID
                "SETUID".to_string(),           // Set user ID
                "NET_BIND_SERVICE".to_string(), // Bind to ports < 1024
            ]),
        }),
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

    // Volume mounts for writable directories per garage-isolation.md spec.
    // Root filesystem is read-only, so we mount emptyDir volumes for writable paths.
    let workspace_volume_mount = VolumeMount {
        name: "workspace".to_string(),
        mount_path: "/workspace".to_string(),
        ..Default::default()
    };

    // Ephemeral writable mounts (destroyed with pod)
    let volume_mounts = vec![
        workspace_volume_mount.clone(),
        VolumeMount {
            name: "tmp".to_string(),
            mount_path: "/tmp".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "var-tmp".to_string(),
            mount_path: "/var/tmp".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "home".to_string(),
            mount_path: "/root".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "nix".to_string(),
            mount_path: "/nix".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "cargo".to_string(),
            mount_path: "/root/.cargo".to_string(),
            ..Default::default()
        },
        // For apt package installation
        VolumeMount {
            name: "var-lib-apt".to_string(),
            mount_path: "/var/lib/apt".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "var-cache-apt".to_string(),
            mount_path: "/var/cache/apt".to_string(),
            ..Default::default()
        },
        VolumeMount {
            name: "usr-local".to_string(),
            mount_path: "/usr/local".to_string(),
            ..Default::default()
        },
    ];

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
        volume_mounts: Some(volume_mounts.clone()),
        // Use container's default entrypoint (garage-entrypoint)
        // which starts ttyd for terminal access
        readiness_probe: Some(readiness_probe),
        ..Default::default()
    };

    // Volumes per garage-isolation.md spec:
    // - workspace: PersistentVolumeClaim (survives pod restarts per spec)
    // - tmp, var-tmp, home, nix, cargo: ephemeral writable paths
    // - var-lib-apt, var-cache-apt, usr-local: for apt package installation
    let volumes = vec![
        Volume {
            name: "workspace".to_string(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: WORKSPACE_PVC_NAME.to_string(),
                read_only: Some(false),
            }),
            ..Default::default()
        },
        Volume {
            name: "tmp".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "var-tmp".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "home".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "nix".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "cargo".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "var-lib-apt".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "var-cache-apt".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
        Volume {
            name: "usr-local".to_string(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Default::default()
        },
    ];

    // Build init containers for repo cloning if configured
    let init_containers = repo.map(|repo_config| {
        build_repo_clone_init_container(
            image,
            repo_config,
            &workspace_volume_mount,
            &security_context,
        )
    });

    // Pod-level security context per garage-isolation.md spec:
    // - No service account token (garages have no K8s API access)
    // - Forbidden: hostNetwork, hostPID, hostIPC (defaults to false)
    let pod_security_context = PodSecurityContext {
        run_as_user: Some(0),
        run_as_group: Some(0),
        fs_group: Some(0),
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
            init_containers: init_containers.map(|c| vec![c]),
            containers: vec![container],
            volumes: Some(volumes),
            restart_policy: Some("Never".to_string()),
            // No K8s API access for garage pods per garage-isolation.md
            automount_service_account_token: Some(false),
            security_context: Some(pod_security_context),
            // Forbidden settings (all default to false, but explicit for clarity)
            host_network: Some(false),
            host_pid: Some(false),
            host_ipc: Some(false),
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
    fn build_pod_has_writable_volumes_per_spec() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();
        let volumes = spec.volumes.as_ref().unwrap();
        let container = &spec.containers[0];
        let mounts = container.volume_mounts.as_ref().unwrap();

        // Expected emptyDir volumes and their mount paths per garage-isolation.md spec
        // Note: workspace is now a PVC, not emptyDir
        let expected_emptydir_volumes = [
            ("tmp", "/tmp"),
            ("var-tmp", "/var/tmp"),
            ("home", "/root"),
            ("nix", "/nix"),
            ("cargo", "/root/.cargo"),
            ("var-lib-apt", "/var/lib/apt"),
            ("var-cache-apt", "/var/cache/apt"),
            ("usr-local", "/usr/local"),
        ];

        // Check workspace volume is a PVC per garage-isolation.md spec
        let workspace_vol = volumes
            .iter()
            .find(|v| v.name == "workspace")
            .expect("workspace volume should exist");
        assert!(
            workspace_vol.persistent_volume_claim.is_some(),
            "workspace should be a PersistentVolumeClaim"
        );
        let pvc = workspace_vol.persistent_volume_claim.as_ref().unwrap();
        assert_eq!(pvc.claim_name, WORKSPACE_PVC_NAME);
        assert_eq!(pvc.read_only, Some(false));

        // Check workspace mount
        let workspace_mount = mounts
            .iter()
            .find(|m| m.name == "workspace")
            .expect("workspace mount should exist");
        assert_eq!(workspace_mount.mount_path, "/workspace");

        // Check all emptyDir volumes exist
        for (vol_name, _) in &expected_emptydir_volumes {
            let volume = volumes
                .iter()
                .find(|v| v.name == *vol_name)
                .unwrap_or_else(|| panic!("volume '{}' should exist", vol_name));
            assert!(
                volume.empty_dir.is_some(),
                "volume '{}' should be emptyDir",
                vol_name
            );
        }

        // Check all volume mounts exist with correct paths
        for (vol_name, mount_path) in &expected_emptydir_volumes {
            let mount = mounts
                .iter()
                .find(|m| m.name == *vol_name)
                .unwrap_or_else(|| panic!("mount '{}' should exist", vol_name));
            assert_eq!(
                mount.mount_path, *mount_path,
                "mount '{}' should have path '{}'",
                vol_name, mount_path
            );
        }

        // Total volume count: 1 PVC (workspace) + 8 emptyDir = 9
        assert_eq!(volumes.len(), 9, "should have exactly 9 volumes");
        assert_eq!(mounts.len(), 9, "should have exactly 9 mounts");
    }

    #[test]
    fn build_pod_has_security_context_per_spec() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let pod =
            build_dev_container_pod("moto-garage-abc12345", "test:latest", "main", labels, None);

        let spec = pod.spec.as_ref().unwrap();

        // Check pod-level security context
        let pod_sec = spec
            .security_context
            .as_ref()
            .expect("pod security_context should be set");
        assert_eq!(pod_sec.run_as_user, Some(0), "pod should run as root");
        assert_eq!(pod_sec.run_as_group, Some(0));

        // Check automountServiceAccountToken is disabled (no K8s API access)
        assert_eq!(
            spec.automount_service_account_token,
            Some(false),
            "service account token should not be mounted"
        );

        // Check forbidden host settings
        assert_eq!(spec.host_network, Some(false));
        assert_eq!(spec.host_pid, Some(false));
        assert_eq!(spec.host_ipc, Some(false));

        // Check container security context
        let container = &spec.containers[0];
        let sec = container
            .security_context
            .as_ref()
            .expect("container security_context should be set");

        assert_eq!(sec.run_as_user, Some(0), "container should run as root");
        assert_eq!(sec.run_as_group, Some(0));
        assert_eq!(
            sec.allow_privilege_escalation,
            Some(false),
            "privilege escalation should be disabled"
        );
        assert_eq!(
            sec.read_only_root_filesystem,
            Some(true),
            "root filesystem should be read-only"
        );

        // Check seccomp profile
        let seccomp = sec
            .seccomp_profile
            .as_ref()
            .expect("seccomp_profile should be set");
        assert_eq!(seccomp.type_, "RuntimeDefault");

        // Check capabilities
        let caps = sec
            .capabilities
            .as_ref()
            .expect("capabilities should be set");
        assert_eq!(caps.drop, Some(vec!["ALL".to_string()]));

        let add = caps.add.as_ref().expect("capabilities.add should be set");
        assert!(add.contains(&"CHOWN".to_string()));
        assert!(add.contains(&"DAC_OVERRIDE".to_string()));
        assert!(add.contains(&"FOWNER".to_string()));
        assert!(add.contains(&"SETGID".to_string()));
        assert!(add.contains(&"SETUID".to_string()));
        assert!(add.contains(&"NET_BIND_SERVICE".to_string()));
        assert_eq!(add.len(), 6, "should have exactly 6 capabilities added");
    }
}
