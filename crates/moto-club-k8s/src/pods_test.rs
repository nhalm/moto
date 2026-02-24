//! Tests for garage pod management.
//!
//! Per AGENTS.md test organization convention, tests for `pods.rs` are in this separate file.

use super::pods::*;
use crate::pvc::WORKSPACE_PVC_NAME;
use crate::supporting_services::{
    POSTGRES_CREDENTIALS_SECRET_NAME, POSTGRES_PORT, POSTGRES_SERVICE_NAME,
    REDIS_CREDENTIALS_SECRET_NAME, REDIS_PORT, REDIS_SERVICE_NAME,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use moto_club_types::GarageId;
use moto_k8s::Labels;

#[test]
fn pod_input_namespace_name() {
    let input = GaragePodInput {
        id: GarageId::new(),
        name: "my-project".to_string(),
        owner: "alice".to_string(),
        branch: "main".to_string(),
        image: None,
        repo: None,
        with_postgres: false,
        with_redis: false,
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
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

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
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

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
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

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
        false,
        false,
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
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

    let spec = pod.spec.as_ref().unwrap();

    // No init containers when repo is None
    assert!(spec.init_containers.is_none());
}

#[test]
#[allow(clippy::too_many_lines)]
fn build_pod_has_writable_volumes_per_spec() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

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
            .unwrap_or_else(|| panic!("volume '{vol_name}' should exist"));
        assert!(
            volume.empty_dir.is_some(),
            "volume '{vol_name}' should be emptyDir"
        );
    }

    // Check all volume mounts exist with correct paths
    for (vol_name, mount_path) in &expected_emptydir_volumes {
        let mount = mounts
            .iter()
            .find(|m| m.name == *vol_name)
            .unwrap_or_else(|| panic!("mount '{vol_name}' should exist"));
        assert_eq!(
            mount.mount_path, *mount_path,
            "mount '{vol_name}' should have path '{mount_path}'"
        );
    }

    // Check secret/configmap volumes per garage-isolation.md spec
    // wireguard-config (ConfigMap), wireguard-keys (Secret), garage-svid (Secret)
    let wg_config_vol = volumes
        .iter()
        .find(|v| v.name == "wireguard-config")
        .expect("wireguard-config volume should exist");
    assert!(
        wg_config_vol.config_map.is_some(),
        "wireguard-config should be a ConfigMap"
    );
    assert_eq!(
        wg_config_vol.config_map.as_ref().unwrap().name,
        "wireguard-config"
    );

    let wg_keys_vol = volumes
        .iter()
        .find(|v| v.name == "wireguard-keys")
        .expect("wireguard-keys volume should exist");
    assert!(
        wg_keys_vol.secret.is_some(),
        "wireguard-keys should be a Secret"
    );
    assert_eq!(
        wg_keys_vol.secret.as_ref().unwrap().secret_name,
        Some("wireguard-keys".to_string())
    );

    let svid_vol = volumes
        .iter()
        .find(|v| v.name == "garage-svid")
        .expect("garage-svid volume should exist");
    assert!(svid_vol.secret.is_some(), "garage-svid should be a Secret");
    assert_eq!(
        svid_vol.secret.as_ref().unwrap().secret_name,
        Some("garage-svid".to_string())
    );

    // Check secret/configmap mounts are read-only per spec
    let wg_config_mount = mounts
        .iter()
        .find(|m| m.name == "wireguard-config")
        .expect("wireguard-config mount should exist");
    assert_eq!(wg_config_mount.mount_path, "/etc/wireguard/config");
    assert_eq!(
        wg_config_mount.read_only,
        Some(true),
        "wireguard-config should be read-only"
    );

    let wg_keys_mount = mounts
        .iter()
        .find(|m| m.name == "wireguard-keys")
        .expect("wireguard-keys mount should exist");
    assert_eq!(wg_keys_mount.mount_path, "/etc/wireguard/keys");
    assert_eq!(
        wg_keys_mount.read_only,
        Some(true),
        "wireguard-keys should be read-only"
    );

    let svid_mount = mounts
        .iter()
        .find(|m| m.name == "garage-svid")
        .expect("garage-svid mount should exist");
    assert_eq!(svid_mount.mount_path, "/var/run/secrets/svid");
    assert_eq!(
        svid_mount.read_only,
        Some(true),
        "garage-svid should be read-only"
    );

    // Total volume count: 1 PVC (workspace) + 7 emptyDir + 1 ConfigMap + 2 Secrets = 11
    assert_eq!(volumes.len(), 11, "should have exactly 11 volumes");
    assert_eq!(mounts.len(), 11, "should have exactly 11 mounts");
}

#[test]
fn build_pod_has_security_context_per_spec() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false,
        false,
    );

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
    let container_sec = container
        .security_context
        .as_ref()
        .expect("container security_context should be set");

    assert_eq!(
        container_sec.run_as_user,
        Some(0),
        "container should run as root"
    );
    assert_eq!(container_sec.run_as_group, Some(0));
    assert_eq!(
        container_sec.allow_privilege_escalation,
        Some(false),
        "privilege escalation should be disabled"
    );
    assert_eq!(
        container_sec.read_only_root_filesystem,
        Some(true),
        "root filesystem should be read-only"
    );

    // Check seccomp profile
    let seccomp = container_sec
        .seccomp_profile
        .as_ref()
        .expect("seccomp_profile should be set");
    assert_eq!(seccomp.type_, "RuntimeDefault");

    // Check capabilities
    let caps = container_sec
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

#[test]
fn build_pod_with_postgres_injects_env_vars() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        true,  // with_postgres
        false, // with_redis
    );

    let spec = pod.spec.as_ref().unwrap();
    let container = &spec.containers[0];
    let env = container.env.as_ref().unwrap();

    // Check POSTGRES_HOST
    let host = env.iter().find(|e| e.name == "POSTGRES_HOST").unwrap();
    assert_eq!(host.value, Some(POSTGRES_SERVICE_NAME.to_string()));

    // Check POSTGRES_PORT
    let port = env.iter().find(|e| e.name == "POSTGRES_PORT").unwrap();
    assert_eq!(port.value, Some(POSTGRES_PORT.to_string()));

    // Check POSTGRES_USER
    let user = env.iter().find(|e| e.name == "POSTGRES_USER").unwrap();
    assert_eq!(user.value, Some("dev".to_string()));

    // Check POSTGRES_DB
    let db = env.iter().find(|e| e.name == "POSTGRES_DB").unwrap();
    assert_eq!(db.value, Some("dev".to_string()));

    // Check POSTGRES_PASSWORD (from secret)
    let pass = env.iter().find(|e| e.name == "POSTGRES_PASSWORD").unwrap();
    let secret_ref = pass
        .value_from
        .as_ref()
        .unwrap()
        .secret_key_ref
        .as_ref()
        .unwrap();
    assert_eq!(secret_ref.name, POSTGRES_CREDENTIALS_SECRET_NAME);
    assert_eq!(secret_ref.key, "password");

    // Check DATABASE_URL (from secret)
    let url = env.iter().find(|e| e.name == "DATABASE_URL").unwrap();
    let secret_ref = url
        .value_from
        .as_ref()
        .unwrap()
        .secret_key_ref
        .as_ref()
        .unwrap();
    assert_eq!(secret_ref.name, POSTGRES_CREDENTIALS_SECRET_NAME);
    assert_eq!(secret_ref.key, "url");
}

#[test]
fn build_pod_without_postgres_no_postgres_env_vars() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false, // with_postgres
        false, // with_redis
    );

    let spec = pod.spec.as_ref().unwrap();
    let container = &spec.containers[0];
    let env = container.env.as_ref().unwrap();

    // Check no Postgres env vars exist
    assert!(
        !env.iter().any(|e| e.name == "POSTGRES_HOST"),
        "POSTGRES_HOST should not be present"
    );
    assert!(
        !env.iter().any(|e| e.name == "DATABASE_URL"),
        "DATABASE_URL should not be present"
    );
}

#[test]
fn build_pod_with_redis_injects_env_vars() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false, // with_postgres
        true,  // with_redis
    );

    let spec = pod.spec.as_ref().unwrap();
    let container = &spec.containers[0];
    let env = container.env.as_ref().unwrap();

    // Check REDIS_HOST
    let host = env.iter().find(|e| e.name == "REDIS_HOST").unwrap();
    assert_eq!(host.value, Some(REDIS_SERVICE_NAME.to_string()));

    // Check REDIS_PORT
    let port = env.iter().find(|e| e.name == "REDIS_PORT").unwrap();
    assert_eq!(port.value, Some(REDIS_PORT.to_string()));

    // Check REDIS_PASSWORD (from secret)
    let pass = env.iter().find(|e| e.name == "REDIS_PASSWORD").unwrap();
    let secret_ref = pass
        .value_from
        .as_ref()
        .unwrap()
        .secret_key_ref
        .as_ref()
        .unwrap();
    assert_eq!(secret_ref.name, REDIS_CREDENTIALS_SECRET_NAME);
    assert_eq!(secret_ref.key, "password");

    // Check REDIS_URL (from secret)
    let url = env.iter().find(|e| e.name == "REDIS_URL").unwrap();
    let secret_ref = url
        .value_from
        .as_ref()
        .unwrap()
        .secret_key_ref
        .as_ref()
        .unwrap();
    assert_eq!(secret_ref.name, REDIS_CREDENTIALS_SECRET_NAME);
    assert_eq!(secret_ref.key, "url");
}

#[test]
fn build_pod_without_redis_no_redis_env_vars() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        false, // with_postgres
        false, // with_redis
    );

    let spec = pod.spec.as_ref().unwrap();
    let container = &spec.containers[0];
    let env = container.env.as_ref().unwrap();

    // Check no Redis env vars exist
    assert!(
        !env.iter().any(|e| e.name == "REDIS_HOST"),
        "REDIS_HOST should not be present"
    );
    assert!(
        !env.iter().any(|e| e.name == "REDIS_URL"),
        "REDIS_URL should not be present"
    );
}

#[test]
fn build_pod_with_both_postgres_and_redis_injects_all_env_vars() {
    let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
    let pod = build_dev_container_pod(
        "moto-garage-abc12345",
        "test:latest",
        "main",
        labels,
        None,
        true, // with_postgres
        true, // with_redis
    );

    let spec = pod.spec.as_ref().unwrap();
    let container = &spec.containers[0];
    let env = container.env.as_ref().unwrap();

    // Check both Postgres and Redis env vars exist
    assert!(
        env.iter().any(|e| e.name == "POSTGRES_HOST"),
        "POSTGRES_HOST should be present"
    );
    assert!(
        env.iter().any(|e| e.name == "DATABASE_URL"),
        "DATABASE_URL should be present"
    );
    assert!(
        env.iter().any(|e| e.name == "REDIS_HOST"),
        "REDIS_HOST should be present"
    );
    assert!(
        env.iter().any(|e| e.name == "REDIS_URL"),
        "REDIS_URL should be present"
    );
}
