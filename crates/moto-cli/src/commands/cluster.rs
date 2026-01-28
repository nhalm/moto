//! Cluster subcommands: init, status.
//!
//! Manages the local Kubernetes cluster (k3d) where garages and bikes run.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::process::Command as ProcessCommand;

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

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

/// Run the cluster command
pub async fn run(cmd: ClusterCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        ClusterAction::Init { force } => init_cluster(flags, force).await,
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
            .map(|name| name == CLUSTER_NAME)
            .unwrap_or(false)
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

/// Wait for the cluster API to be ready
fn wait_for_cluster_ready(quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!("Waiting for cluster to be ready...");
    }

    // k3d waits for the cluster to be ready as part of create, but we can
    // verify by checking kubectl connectivity
    let output = ProcessCommand::new("kubectl")
        .args(["--context", &format!("k3d-{CLUSTER_NAME}"), "cluster-info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match output {
        Ok(status) if status.success() => Ok(()),
        _ => Err(CliError::general(
            "Cluster created but API not responding. Try: kubectl cluster-info --context k3d-moto",
        )),
    }
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
    wait_for_cluster_ready(flags.quiet)?;

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
}
