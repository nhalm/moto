//! Garage pod lifecycle management.
//!
//! Provides operations for deploying, monitoring, and deleting garage pods
//! within garage namespaces.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{
    Capabilities, ConfigMapVolumeSource, Container, EmptyDirVolumeSource, EnvVar, EnvVarSource,
    PersistentVolumeClaimVolumeSource, Pod, PodSecurityContext, PodSpec, Probe,
    ResourceRequirements, SeccompProfile, SecretKeySelector, SecretVolumeSource, SecurityContext,
    TCPSocketAction, Volume, VolumeMount,
};

use crate::pvc::WORKSPACE_PVC_NAME;
use crate::supporting_services::{
    POSTGRES_CREDENTIALS_SECRET_NAME, POSTGRES_PORT, POSTGRES_SERVICE_NAME,
    REDIS_CREDENTIALS_SECRET_NAME, REDIS_PORT, REDIS_SERVICE_NAME,
};
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
    /// Include `PostgreSQL` environment variables.
    pub with_postgres: bool,
    /// Include Redis environment variables.
    pub with_redis: bool,
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

    /// Checks if the init container (repo clone) succeeded.
    ///
    /// Returns:
    /// - `Some(true)` if the init container completed successfully (exit code 0)
    /// - `Some(false)` if the init container failed
    /// - `None` if there's no init container configured (no repo to clone)
    ///
    /// # Errors
    ///
    /// Returns an error if the pod doesn't exist or the operation fails.
    fn init_container_succeeded(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<Option<bool>>> + Send;
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
            input.with_postgres,
            input.with_redis,
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

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn init_container_succeeded(&self, id: &GarageId) -> Result<Option<bool>> {
        let pod = self.get_garage_pod(id).await?;

        // Check if pod has init containers configured
        let has_init_containers = pod
            .spec
            .as_ref()
            .and_then(|spec| spec.init_containers.as_ref())
            .is_some_and(|ics| !ics.is_empty());

        if !has_init_containers {
            // No init container = no repo to clone, considered "ready" for this check
            debug!("no init containers configured, skipping repo clone check");
            return Ok(None);
        }

        // Get init container statuses
        let init_statuses = pod
            .status
            .as_ref()
            .and_then(|s| s.init_container_statuses.as_ref());

        let Some(init_statuses) = init_statuses else {
            // Init containers configured but no status yet - still pending
            debug!("init containers configured but no status yet");
            return Ok(Some(false));
        };

        // Find the clone-repo init container status
        let clone_status = init_statuses.iter().find(|cs| cs.name == "clone-repo");

        let Some(clone_status) = clone_status else {
            // Init container not found in status - still pending
            debug!("clone-repo init container not found in status");
            return Ok(Some(false));
        };

        // Check if terminated successfully (exit code 0)
        if let Some(state) = &clone_status.state {
            if let Some(terminated) = &state.terminated {
                let succeeded = terminated.exit_code == 0;
                debug!(
                    exit_code = terminated.exit_code,
                    succeeded = succeeded,
                    "clone-repo init container terminated"
                );
                return Ok(Some(succeeded));
            }
        }

        // Init container still running or waiting
        debug!("clone-repo init container not yet terminated");
        Ok(Some(false))
    }
}

/// Builds a dev container pod spec.
#[allow(clippy::too_many_lines)]
pub(crate) fn build_dev_container_pod(
    namespace: &str,
    image: &str,
    branch: &str,
    labels: BTreeMap<String, String>,
    repo: Option<&RepoConfig>,
    with_postgres: bool,
    with_redis: bool,
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
    let mut env_vars = vec![
        EnvVar {
            name: "MOTO_GARAGE_BRANCH".to_string(),
            value: Some(branch.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "MOTO_GARAGE_NAMESPACE".to_string(),
            value: Some(namespace.to_string()),
            ..Default::default()
        },
    ];

    // Inject Postgres env vars per supporting-services.md spec (lines 236-255)
    if with_postgres {
        env_vars.extend(build_postgres_env_vars());
    }

    // Inject Redis env vars per supporting-services.md spec (lines 258-272)
    if with_redis {
        env_vars.extend(build_redis_env_vars());
    }

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
        // Secrets (pushed by moto-club) - read-only per garage-isolation.md spec
        VolumeMount {
            name: "wireguard-config".to_string(),
            mount_path: "/etc/wireguard/config".to_string(),
            read_only: Some(true),
            ..Default::default()
        },
        VolumeMount {
            name: "wireguard-keys".to_string(),
            mount_path: "/etc/wireguard/keys".to_string(),
            read_only: Some(true),
            ..Default::default()
        },
        VolumeMount {
            name: "garage-svid".to_string(),
            mount_path: "/var/run/secrets/svid".to_string(),
            read_only: Some(true),
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
        volume_mounts: Some(volume_mounts),
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
        // Secrets (pushed by moto-club) per garage-isolation.md spec
        Volume {
            name: "wireguard-config".to_string(),
            config_map: Some(ConfigMapVolumeSource {
                name: "wireguard-config".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
        Volume {
            name: "wireguard-keys".to_string(),
            secret: Some(SecretVolumeSource {
                secret_name: Some("wireguard-keys".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        Volume {
            name: "garage-svid".to_string(),
            secret: Some(SecretVolumeSource {
                secret_name: Some("garage-svid".to_string()),
                ..Default::default()
            }),
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
    let clone_script = r#"#!/bin/sh
set -e

REPO_URL="${REPO_URL}"
REPO_BRANCH="${REPO_BRANCH}"
REPO_NAME="${REPO_NAME}"
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
"#;

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
        args: Some(vec![clone_script.to_string()]),
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

/// Builds Postgres environment variables for garage pod.
///
/// Per supporting-services.md spec (lines 236-255):
/// - `POSTGRES_HOST`: postgres
/// - `POSTGRES_PORT`: 5432
/// - `POSTGRES_USER`: dev
/// - `POSTGRES_PASSWORD`: from secret
/// - `POSTGRES_DB`: dev
/// - `DATABASE_URL`: from secret
fn build_postgres_env_vars() -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: "POSTGRES_HOST".to_string(),
            value: Some(POSTGRES_SERVICE_NAME.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_PORT".to_string(),
            value: Some(POSTGRES_PORT.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_USER".to_string(),
            value: Some("dev".to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_PASSWORD".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: POSTGRES_CREDENTIALS_SECRET_NAME.to_string(),
                    key: "password".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_DB".to_string(),
            value: Some("dev".to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "DATABASE_URL".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: POSTGRES_CREDENTIALS_SECRET_NAME.to_string(),
                    key: "url".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ]
}

/// Builds Redis environment variables for garage pod.
///
/// Per supporting-services.md spec (lines 258-272):
/// - `REDIS_HOST`: redis
/// - `REDIS_PORT`: 6379
/// - `REDIS_PASSWORD`: from secret
/// - `REDIS_URL`: from secret
fn build_redis_env_vars() -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: "REDIS_HOST".to_string(),
            value: Some(REDIS_SERVICE_NAME.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "REDIS_PORT".to_string(),
            value: Some(REDIS_PORT.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "REDIS_PASSWORD".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: REDIS_CREDENTIALS_SECRET_NAME.to_string(),
                    key: "password".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "REDIS_URL".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: REDIS_CREDENTIALS_SECRET_NAME.to_string(),
                    key: "url".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ]
}
