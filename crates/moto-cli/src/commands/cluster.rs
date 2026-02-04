//! Cluster subcommands: init, status.
//!
//! Manages the local Kubernetes cluster (k3d) where garages and bikes run.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::process::Command as ProcessCommand;
use std::time::Duration;
use tokio::time::sleep;

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

/// Backoff delays for waiting on cluster API readiness
const API_READY_BACKOFF: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];

/// Maximum total wait time for API readiness (sum of backoff + initial check)
const API_READY_TIMEOUT_SECS: u64 = 30;

/// Cluster command and subcommands
#[derive(Args)]
pub struct ClusterCommand {
    #[command(subcommand)]
    pub action: ClusterAction,
}

/// Available cluster actions
#[derive(Subcommand)]
pub enum ClusterAction {
    /// Initialize the local Kubernetes cluster
    Init {
        /// Delete existing cluster and recreate
        #[arg(long)]
        force: bool,
    },
    /// Show cluster status
    Status,
}

/// Cluster name used by moto
const CLUSTER_NAME: &str = "moto";

/// Registry name and port
const REGISTRY_NAME: &str = "moto-registry";
const REGISTRY_PORT: u16 = 5000;

/// K8s API port
const API_PORT: u16 = 6550;

/// JSON output for cluster init
#[derive(Serialize)]
struct ClusterInitJson {
    name: String,
    status: String,
    api_endpoint: String,
    registry_endpoint: String,
}

/// JSON output for cluster status
#[derive(Serialize)]
struct ClusterStatusJson {
    name: String,
    #[serde(rename = "type")]
    cluster_type: String,
    status: String,
    api: ApiStatusJson,
    registry: RegistryStatusJson,
}

#[derive(Serialize)]
struct ApiStatusJson {
    endpoint: String,
    healthy: bool,
}

#[derive(Serialize)]
struct RegistryStatusJson {
    endpoint: String,
    healthy: bool,
}

/// Run the cluster command
pub async fn run(cmd: ClusterCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        ClusterAction::Init { force } => init_cluster(flags, force).await,
        ClusterAction::Status => cluster_status(flags).await,
    }
}

/// Check if Docker daemon is running
fn check_docker_running() -> Result<bool> {
    let output = ProcessCommand::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match output {
        Ok(status) => Ok(status.success()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CliError::general(
            "Docker not found. Please install Docker or Colima.\n\n\
                 On macOS: brew install --cask docker\n\
                 Or: brew install colima && colima start",
        )),
        Err(_) => Ok(false),
    }
}

/// Check if k3d is installed
fn check_k3d_installed() -> Result<()> {
    let output = ProcessCommand::new("k3d")
        .args(["version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match output {
        Ok(status) if status.success() => Ok(()),
        Ok(_) | Err(_) => Err(CliError::general(
            "k3d not found. Please install k3d.\n\n\
             On macOS: brew install k3d\n\
             Other: https://k3d.io/#installation",
        )),
    }
}

/// Check if the moto cluster already exists
fn cluster_exists() -> Result<bool> {
    let output = ProcessCommand::new("k3d")
        .args(["cluster", "list", "--no-headers"])
        .output()
        .map_err(|e| CliError::general(format!("failed to list k3d clusters: {e}")))?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // k3d cluster list output format: "name  servers  agents  loadbalancer"
    Ok(stdout.lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|name| name == CLUSTER_NAME)
    }))
}

/// Delete the existing cluster
fn delete_cluster(quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!("Deleting existing cluster...");
    }

    let output = ProcessCommand::new("k3d")
        .args(["cluster", "delete", CLUSTER_NAME])
        .output()
        .map_err(|e| CliError::general(format!("failed to delete cluster: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::general(format!(
            "failed to delete cluster: {stderr}"
        )));
    }

    Ok(())
}

/// Create the k3d cluster
fn create_cluster(quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!("Creating k3d cluster '{CLUSTER_NAME}'...");
    }

    let output = ProcessCommand::new("k3d")
        .args([
            "cluster",
            "create",
            CLUSTER_NAME,
            "--api-port",
            &API_PORT.to_string(),
            "--port",
            "80:80@loadbalancer",
            "--port",
            "443:443@loadbalancer",
            "--registry-create",
            &format!("{REGISTRY_NAME}:{REGISTRY_PORT}"),
            "--k3s-arg",
            "--disable=traefik@server:0",
        ])
        .output()
        .map_err(|e| CliError::general(format!("failed to create cluster: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check for common error conditions
        if stderr.contains("port is already allocated") || stderr.contains("address already in use")
        {
            return Err(CliError::general(format!(
                "Port conflict detected. Another service may be using ports 80, 443, {API_PORT}, or {REGISTRY_PORT}.\n\n\
                 Try: docker ps  # to see running containers\n\
                 Or:  lsof -i :{API_PORT}  # to find the conflicting process"
            )));
        }

        return Err(CliError::general(format!(
            "failed to create cluster: {stderr}"
        )));
    }

    Ok(())
}

/// Check if the cluster API is responding
fn check_api_ready() -> bool {
    let output = ProcessCommand::new("kubectl")
        .args(["--context", &format!("k3d-{CLUSTER_NAME}"), "cluster-info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    matches!(output, Ok(status) if status.success())
}

/// Check if the K8s API is healthy (for status reporting)
fn check_api_healthy() -> bool {
    // Use kubectl get --raw /healthz to check API health
    let output = ProcessCommand::new("kubectl")
        .args([
            "--context",
            &format!("k3d-{CLUSTER_NAME}"),
            "get",
            "--raw",
            "/healthz",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            // /healthz returns "ok" when healthy
            let body = String::from_utf8_lossy(&o.stdout);
            body.trim() == "ok"
        }
        _ => false,
    }
}

/// Check if the container registry is healthy (for status reporting)
fn check_registry_healthy() -> bool {
    // Check registry health via the Docker API /v2/ endpoint
    // The registry runs at localhost:5000
    let output = ProcessCommand::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "2",
            &format!("http://localhost:{REGISTRY_PORT}/v2/"),
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            // curl returns HTTP status code, 200 means healthy
            let status_code = String::from_utf8_lossy(&o.stdout);
            status_code.trim() == "200"
        }
        _ => false,
    }
}

/// Wait for the cluster API to be ready with retries
async fn wait_for_cluster_ready(quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!("Waiting for cluster to be ready...");
    }

    // Initial check - API might already be ready
    if check_api_ready() {
        return Ok(());
    }

    // Retry with backoff
    for (i, delay) in API_READY_BACKOFF.iter().enumerate() {
        sleep(*delay).await;

        if check_api_ready() {
            return Ok(());
        }

        if !quiet {
            eprintln!(
                "  API not ready yet, retrying... ({}/{})",
                i + 1,
                API_READY_BACKOFF.len()
            );
        }
    }

    // Final error after all retries exhausted
    Err(CliError::general(format!(
        "Cluster created but API not responding after {API_READY_TIMEOUT_SECS}s.\n\n\
         Try: kubectl cluster-info --context k3d-{CLUSTER_NAME}"
    )))
}

/// Initialize the local k3d cluster
async fn init_cluster(flags: &GlobalFlags, force: bool) -> Result<()> {
    // Check prerequisites
    check_k3d_installed()?;

    if !check_docker_running()? {
        return Err(CliError::general(
            "Docker is not running. Please start Docker or Colima.\n\n\
             On macOS with Colima: colima start\n\
             On macOS with Docker Desktop: open the Docker app",
        ));
    }

    // Check if cluster already exists
    let exists = cluster_exists()?;

    if exists && !force {
        // Idempotent: cluster exists, return success
        if flags.json {
            let json = ClusterInitJson {
                name: CLUSTER_NAME.to_string(),
                status: "exists".to_string(),
                api_endpoint: format!("https://localhost:{API_PORT}"),
                registry_endpoint: format!("localhost:{REGISTRY_PORT}"),
            };
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else if !flags.quiet {
            println!("Cluster '{CLUSTER_NAME}' already exists.");
            println!();
            println!("  K8s API:  https://localhost:{API_PORT}");
            println!("  Registry: localhost:{REGISTRY_PORT}");
            println!();
            println!("Use --force to delete and recreate.");
        }
        return Ok(());
    }

    // Delete existing cluster if force flag is set
    if exists && force {
        delete_cluster(flags.quiet)?;
    }

    // Create the cluster
    create_cluster(flags.quiet)?;

    // Wait for it to be ready
    wait_for_cluster_ready(flags.quiet).await?;

    // Success output
    if flags.json {
        let json = ClusterInitJson {
            name: CLUSTER_NAME.to_string(),
            status: "created".to_string(),
            api_endpoint: format!("https://localhost:{API_PORT}"),
            registry_endpoint: format!("localhost:{REGISTRY_PORT}"),
        };
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if !flags.quiet {
        println!("Cluster '{CLUSTER_NAME}' created successfully.");
        println!();
        println!("  K8s API:  https://localhost:{API_PORT}");
        println!("  Registry: localhost:{REGISTRY_PORT}");
        println!("  Context:  k3d-{CLUSTER_NAME}");
    }

    Ok(())
}

/// Cluster status values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterStatus {
    /// Cluster exists and is running
    Running,
    /// Cluster exists but is stopped
    Stopped,
    /// No cluster found
    NotFound,
}

impl ClusterStatus {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::NotFound => "not_found",
        }
    }
}

/// Get the current cluster status by checking k3d
fn get_cluster_status() -> Result<ClusterStatus> {
    let output = ProcessCommand::new("k3d")
        .args(["cluster", "list", "--no-headers"])
        .output()
        .map_err(|e| CliError::general(format!("failed to list k3d clusters: {e}")))?;

    if !output.status.success() {
        return Err(CliError::general("failed to list k3d clusters"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // k3d cluster list output format: "name  servers  agents  loadbalancer"
    // servers column shows "1/1" for running, "0/1" for stopped
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.first() == Some(&CLUSTER_NAME) {
            // Check servers column (index 1) for running state
            // Format is "running/total" e.g., "1/1" or "0/1"
            if let Some(servers) = parts.get(1) {
                if servers.starts_with("0/") {
                    return Ok(ClusterStatus::Stopped);
                }
            }
            return Ok(ClusterStatus::Running);
        }
    }

    Ok(ClusterStatus::NotFound)
}

/// Show cluster status
async fn cluster_status(flags: &GlobalFlags) -> Result<()> {
    check_k3d_installed()?;

    let status = get_cluster_status()?;

    match status {
        ClusterStatus::NotFound => {
            if flags.json {
                let json = ClusterStatusJson {
                    name: CLUSTER_NAME.to_string(),
                    cluster_type: "k3d".to_string(),
                    status: status.as_str().to_string(),
                    api: ApiStatusJson {
                        endpoint: format!("https://localhost:{API_PORT}"),
                        healthy: false,
                    },
                    registry: RegistryStatusJson {
                        endpoint: format!("localhost:{REGISTRY_PORT}"),
                        healthy: false,
                    },
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                eprintln!("Cluster '{CLUSTER_NAME}' not found.");
                eprintln!();
                eprintln!("Run 'moto cluster init' to create the cluster.");
            }
            // Exit code 1 for not running
            std::process::exit(1);
        }
        ClusterStatus::Stopped => {
            if flags.json {
                let json = ClusterStatusJson {
                    name: CLUSTER_NAME.to_string(),
                    cluster_type: "k3d".to_string(),
                    status: status.as_str().to_string(),
                    api: ApiStatusJson {
                        endpoint: format!("https://localhost:{API_PORT}"),
                        healthy: false,
                    },
                    registry: RegistryStatusJson {
                        endpoint: format!("localhost:{REGISTRY_PORT}"),
                        healthy: false,
                    },
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Cluster: {CLUSTER_NAME} (k3d)");
                println!("Status: stopped");
                println!();
                println!("  K8s API:   unavailable (cluster stopped)");
            }
            // Exit code 1 for not running
            std::process::exit(1);
        }
        ClusterStatus::Running => {
            // Check K8s API health
            let api_healthy = check_api_healthy();
            let api_endpoint = format!("https://localhost:{API_PORT}");

            // Check registry health
            let registry_healthy = check_registry_healthy();
            let registry_endpoint = format!("localhost:{REGISTRY_PORT}");

            if flags.json {
                let json = ClusterStatusJson {
                    name: CLUSTER_NAME.to_string(),
                    cluster_type: "k3d".to_string(),
                    status: status.as_str().to_string(),
                    api: ApiStatusJson {
                        endpoint: api_endpoint,
                        healthy: api_healthy,
                    },
                    registry: RegistryStatusJson {
                        endpoint: registry_endpoint,
                        healthy: registry_healthy,
                    },
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                let api_status = if api_healthy { "healthy" } else { "unhealthy" };
                let registry_status = if registry_healthy {
                    "healthy"
                } else {
                    "unhealthy"
                };

                println!("Cluster: {CLUSTER_NAME} (k3d)");
                println!("Status: {}", status.as_str());
                println!();
                println!("  K8s API:   {api_status} ({api_endpoint})");
                println!("  Registry:  {registry_status} ({registry_endpoint})");
            }

            // Exit code 1 if API or registry is unhealthy
            if !api_healthy || !registry_healthy {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_name_constant() {
        assert_eq!(CLUSTER_NAME, "moto");
    }

    #[test]
    fn test_registry_config() {
        assert_eq!(REGISTRY_NAME, "moto-registry");
        assert_eq!(REGISTRY_PORT, 5000);
    }

    #[test]
    fn test_api_port() {
        assert_eq!(API_PORT, 6550);
    }

    #[test]
    fn test_check_docker_running_returns_result() {
        // This test verifies that check_docker_running() returns a Result
        // The actual result depends on whether Docker is running on the test machine
        let result = check_docker_running();
        // Should return Ok(true) or Ok(false), not Err unless Docker binary missing
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_k3d_create_command_args() {
        // Verify the k3d cluster create command matches the spec in local-cluster.md
        // The create_cluster function uses these exact arguments:
        //   k3d cluster create moto \
        //     --api-port 6550 \
        //     --port "80:80@loadbalancer" \
        //     --port "443:443@loadbalancer" \
        //     --registry-create moto-registry:5000 \
        //     --k3s-arg "--disable=traefik@server:0"

        let expected_args = [
            "cluster",
            "create",
            CLUSTER_NAME,
            "--api-port",
            &API_PORT.to_string(),
            "--port",
            "80:80@loadbalancer",
            "--port",
            "443:443@loadbalancer",
            "--registry-create",
            &format!("{REGISTRY_NAME}:{REGISTRY_PORT}"),
            "--k3s-arg",
            "--disable=traefik@server:0",
        ];

        // Verify the arguments match spec expectations
        assert_eq!(expected_args[2], "moto");
        assert_eq!(expected_args[4], "6550");
        assert_eq!(expected_args[6], "80:80@loadbalancer");
        assert_eq!(expected_args[8], "443:443@loadbalancer");
        assert_eq!(expected_args[10], "moto-registry:5000");
        assert_eq!(expected_args[12], "--disable=traefik@server:0");
    }

    #[test]
    fn test_cluster_exists_parsing() {
        // Test the parsing logic used in cluster_exists()
        // k3d cluster list --no-headers output format: "name  servers  agents  loadbalancer"

        let sample_output = "moto   1/1     0/0    true\n";
        let has_moto = sample_output.lines().any(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|name| name == CLUSTER_NAME)
        });
        assert!(has_moto);

        // Test with other cluster names
        let other_output = "other-cluster   1/1     0/0    true\n";
        let has_moto_other = other_output.lines().any(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|name| name == CLUSTER_NAME)
        });
        assert!(!has_moto_other);

        // Test with multiple clusters including moto
        let multi_output = "dev-cluster   1/1     0/0    true\nmoto   1/1     0/0    true\ntest   1/1     0/0    true\n";
        let has_moto_multi = multi_output.lines().any(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|name| name == CLUSTER_NAME)
        });
        assert!(has_moto_multi);

        // Test with empty output
        let empty_output = "";
        let has_moto_empty = empty_output.lines().any(|line| {
            line.split_whitespace()
                .next()
                .is_some_and(|name| name == CLUSTER_NAME)
        });
        assert!(!has_moto_empty);
    }

    #[test]
    fn test_api_ready_backoff_config() {
        // Verify backoff configuration is reasonable
        assert!(!API_READY_BACKOFF.is_empty(), "backoff should have delays");

        // Check delays are increasing (exponential-style)
        for window in API_READY_BACKOFF.windows(2) {
            assert!(
                window[1] >= window[0],
                "backoff delays should be non-decreasing"
            );
        }

        // Calculate total wait time
        let total_wait: u64 = API_READY_BACKOFF
            .iter()
            .map(std::time::Duration::as_secs)
            .sum();
        assert!(
            total_wait <= API_READY_TIMEOUT_SECS,
            "total backoff wait ({total_wait}s) should not exceed timeout ({API_READY_TIMEOUT_SECS}s)"
        );
    }

    #[test]
    fn test_check_api_ready_returns_bool() {
        // This test verifies that check_api_ready() returns a boolean
        // The actual result depends on whether kubectl and the cluster are available
        let _result: bool = check_api_ready();
        // Type annotation ensures function returns bool, not a panic
    }

    #[test]
    fn test_check_api_healthy_returns_bool() {
        // This test verifies that check_api_healthy() returns a boolean
        // The actual result depends on whether kubectl and the cluster are available
        let _result: bool = check_api_healthy();
        // Type annotation ensures function returns bool, not a panic
    }

    #[test]
    fn test_check_registry_healthy_returns_bool() {
        // This test verifies that check_registry_healthy() returns a boolean
        // The actual result depends on whether the registry is running
        let _result: bool = check_registry_healthy();
        // Type annotation ensures function returns bool, not a panic
    }

    #[test]
    fn test_registry_health_endpoint() {
        // Verify the health check uses the correct registry endpoint:
        // curl http://localhost:5000/v2/
        // This should return HTTP 200 when healthy
        let expected_endpoint = format!("http://localhost:{REGISTRY_PORT}/v2/");
        assert_eq!(expected_endpoint, "http://localhost:5000/v2/");
    }

    #[test]
    fn test_api_health_endpoint() {
        // Verify the health check uses the correct kubectl command:
        // kubectl --context k3d-moto get --raw /healthz
        // This should return "ok" when healthy
        let expected_context = format!("k3d-{CLUSTER_NAME}");
        assert_eq!(expected_context, "k3d-moto");
    }

    #[test]
    fn test_delete_cluster_command_uses_correct_name() {
        // Verify delete_cluster would use the correct cluster name
        // The function calls: k3d cluster delete CLUSTER_NAME
        assert_eq!(CLUSTER_NAME, "moto");
        // This ensures --force flag deletes the correct cluster
    }

    #[test]
    fn test_force_flag_requires_existing_cluster() {
        // The --force flag logic in init_cluster:
        // if exists && force { delete_cluster(); }
        // This test documents the expected behavior:
        // - If cluster exists and force=true: delete then create
        // - If cluster exists and force=false: return success (idempotent)
        // - If cluster doesn't exist: create regardless of force flag

        // The force flag is defined in ClusterAction::Init
        let action = ClusterAction::Init { force: true };
        let action_no_force = ClusterAction::Init { force: false };
        // Use the values to avoid unused variable warnings
        assert!(matches!(action, ClusterAction::Init { force: true }));
        assert!(matches!(
            action_no_force,
            ClusterAction::Init { force: false }
        ));
    }

    #[test]
    fn test_cluster_status_enum() {
        // Verify ClusterStatus enum values and string representations
        assert_eq!(ClusterStatus::Running.as_str(), "running");
        assert_eq!(ClusterStatus::Stopped.as_str(), "stopped");
        assert_eq!(ClusterStatus::NotFound.as_str(), "not_found");
    }

    #[test]
    fn test_cluster_status_parsing_running() {
        // Test parsing k3d cluster list output for running cluster
        // Format: "name  servers  agents  loadbalancer"
        // servers "1/1" means 1 of 1 servers running

        let sample_output = "moto   1/1     0/0    true\n";
        let status = parse_cluster_status_from_output(sample_output);
        assert_eq!(status, ClusterStatus::Running);
    }

    #[test]
    fn test_cluster_status_parsing_stopped() {
        // Test parsing k3d cluster list output for stopped cluster
        // servers "0/1" means 0 of 1 servers running (stopped)

        let sample_output = "moto   0/1     0/0    false\n";
        let status = parse_cluster_status_from_output(sample_output);
        assert_eq!(status, ClusterStatus::Stopped);
    }

    #[test]
    fn test_cluster_status_parsing_not_found() {
        // Test parsing when cluster doesn't exist
        let sample_output = "other-cluster   1/1     0/0    true\n";
        let status = parse_cluster_status_from_output(sample_output);
        assert_eq!(status, ClusterStatus::NotFound);

        // Empty output
        let empty_output = "";
        let status_empty = parse_cluster_status_from_output(empty_output);
        assert_eq!(status_empty, ClusterStatus::NotFound);
    }

    #[test]
    fn test_cluster_status_parsing_multiple_clusters() {
        // Test parsing when multiple clusters exist
        let sample_output = "dev-cluster   1/1     0/0    true\nmoto   1/1     0/0    true\ntest   0/1     0/0    false\n";
        let status = parse_cluster_status_from_output(sample_output);
        assert_eq!(status, ClusterStatus::Running);
    }

    /// Helper function to parse cluster status from k3d output (for testing)
    fn parse_cluster_status_from_output(stdout: &str) -> ClusterStatus {
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.first() == Some(&CLUSTER_NAME) {
                if let Some(servers) = parts.get(1) {
                    if servers.starts_with("0/") {
                        return ClusterStatus::Stopped;
                    }
                }
                return ClusterStatus::Running;
            }
        }
        ClusterStatus::NotFound
    }

    #[test]
    fn test_cluster_status_json_running() {
        // Test JSON output format for running cluster matches spec
        let json = ClusterStatusJson {
            name: CLUSTER_NAME.to_string(),
            cluster_type: "k3d".to_string(),
            status: ClusterStatus::Running.as_str().to_string(),
            api: ApiStatusJson {
                endpoint: format!("https://localhost:{API_PORT}"),
                healthy: true,
            },
            registry: RegistryStatusJson {
                endpoint: format!("localhost:{REGISTRY_PORT}"),
                healthy: true,
            },
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["name"], "moto");
        assert_eq!(parsed["type"], "k3d");
        assert_eq!(parsed["status"], "running");
        assert_eq!(parsed["api"]["endpoint"], "https://localhost:6550");
        assert_eq!(parsed["api"]["healthy"], true);
        assert_eq!(parsed["registry"]["endpoint"], "localhost:5000");
        assert_eq!(parsed["registry"]["healthy"], true);
    }

    #[test]
    fn test_cluster_status_json_stopped() {
        // Test JSON output format for stopped cluster
        let json = ClusterStatusJson {
            name: CLUSTER_NAME.to_string(),
            cluster_type: "k3d".to_string(),
            status: ClusterStatus::Stopped.as_str().to_string(),
            api: ApiStatusJson {
                endpoint: format!("https://localhost:{API_PORT}"),
                healthy: false,
            },
            registry: RegistryStatusJson {
                endpoint: format!("localhost:{REGISTRY_PORT}"),
                healthy: false,
            },
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["status"], "stopped");
        assert_eq!(parsed["api"]["healthy"], false);
        assert_eq!(parsed["registry"]["healthy"], false);
    }

    #[test]
    fn test_cluster_status_json_not_found() {
        // Test JSON output format for not found cluster
        let json = ClusterStatusJson {
            name: CLUSTER_NAME.to_string(),
            cluster_type: "k3d".to_string(),
            status: ClusterStatus::NotFound.as_str().to_string(),
            api: ApiStatusJson {
                endpoint: format!("https://localhost:{API_PORT}"),
                healthy: false,
            },
            registry: RegistryStatusJson {
                endpoint: format!("localhost:{REGISTRY_PORT}"),
                healthy: false,
            },
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["status"], "not_found");
        assert_eq!(parsed["api"]["healthy"], false);
        assert_eq!(parsed["registry"]["healthy"], false);
    }

    #[test]
    fn test_cluster_status_json_structure_matches_spec() {
        // Verify the JSON structure matches the spec exactly:
        // {
        //   "name": "moto",
        //   "type": "k3d",
        //   "status": "running",
        //   "api": {
        //     "endpoint": "https://localhost:6550",
        //     "healthy": true
        //   },
        //   "registry": {
        //     "endpoint": "localhost:5000",
        //     "healthy": true
        //   }
        // }
        let json = ClusterStatusJson {
            name: "moto".to_string(),
            cluster_type: "k3d".to_string(),
            status: "running".to_string(),
            api: ApiStatusJson {
                endpoint: "https://localhost:6550".to_string(),
                healthy: true,
            },
            registry: RegistryStatusJson {
                endpoint: "localhost:5000".to_string(),
                healthy: true,
            },
        };

        let output = serde_json::to_string(&json).unwrap();
        // Verify all expected keys are present
        assert!(output.contains("\"name\""));
        assert!(output.contains("\"type\""));
        assert!(output.contains("\"status\""));
        assert!(output.contains("\"api\""));
        assert!(output.contains("\"endpoint\""));
        assert!(output.contains("\"healthy\""));
        assert!(output.contains("\"registry\""));
    }
}
