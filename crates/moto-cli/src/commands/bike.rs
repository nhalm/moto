//! Bike subcommands: build, deploy, list, logs.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::process::{Command as ProcessCommand, Stdio};

use crate::bike::discover_bike;
use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

/// Bike command and subcommands
#[derive(Args)]
pub struct BikeCommand {
    #[command(subcommand)]
    pub action: BikeAction,
}

/// Available bike actions
#[derive(Subcommand)]
pub enum BikeAction {
    /// Build container image from bike.toml
    Build {
        /// Override image tag (default: git sha)
        #[arg(long)]
        tag: Option<String>,

        /// Push to registry after build
        #[arg(long)]
        push: bool,
    },
}

/// JSON output for bike build
#[derive(Serialize)]
struct BikeBuildJson {
    name: String,
    image: String,
    pushed: bool,
}

/// Get the Linux target system based on host architecture.
/// Maps arm64/aarch64 -> aarch64-linux, x86_64 -> x86_64-linux.
fn get_linux_target() -> Result<String> {
    let output = ProcessCommand::new("uname")
        .arg("-m")
        .output()
        .map_err(|e| CliError::general(format!("failed to detect architecture: {e}")))?;

    if !output.status.success() {
        return Err(CliError::general("failed to detect architecture"));
    }

    let arch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let linux_arch = match arch.as_str() {
        "arm64" => "aarch64",
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => {
            return Err(CliError::general(format!(
                "unsupported architecture: {other}"
            )));
        }
    };

    Ok(format!("{linux_arch}-linux"))
}

/// Get the short git SHA for the default image tag.
fn get_git_sha() -> Result<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(|e| CliError::general(format!("failed to get git sha: {e}")))?;

    if !output.status.success() {
        return Err(CliError::general(
            "failed to get git sha (not in a git repository?)",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the git root directory.
fn get_git_root() -> Result<String> {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| CliError::general(format!("failed to get git root: {e}")))?;

    if !output.status.success() {
        return Err(CliError::general(
            "failed to get git root (not in a git repository?)",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if Docker is running.
fn check_docker_running() -> Result<bool> {
    let output = ProcessCommand::new("docker")
        .args(["info"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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

/// Build the bike container using Docker-wrapped Nix.
fn build_bike_image(bike_name: &str, tag: &str, quiet: bool) -> Result<()> {
    let linux_target = get_linux_target()?;
    let git_root = get_git_root()?;

    // Nix flake output for the bike
    // For now, we build moto-bike (the base image). In the future, mkBike will
    // create engine-specific images like moto-{bike_name}.
    let nix_output = format!(".#packages.{linux_target}.moto-bike");

    if !quiet {
        eprintln!("Building {}:{}...", bike_name, tag);
    }

    // Build using Docker-wrapped Nix (same pattern as Makefile)
    // This runs nix build inside a nixos/nix container, works on Mac without
    // configuring a Linux builder.
    let nix_command = format!(
        "nix build {} --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result",
        nix_output
    );

    // First, run the Nix build and capture the OCI archive
    let nix_output = ProcessCommand::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/workspace", git_root),
            "-v",
            "nix-store:/nix",
            "-w",
            "/workspace",
            "nixos/nix:latest",
            "sh",
            "-c",
            &nix_command,
        ])
        .output()
        .map_err(|e| CliError::general(format!("failed to run Nix build: {e}")))?;

    if !nix_output.status.success() {
        let stderr = String::from_utf8_lossy(&nix_output.stderr);
        return Err(CliError::general(format!("Nix build failed: {stderr}")));
    }

    // Now pipe the OCI archive to docker load
    let mut docker_load = ProcessCommand::new("docker")
        .args(["load"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CliError::general(format!("failed to start docker load: {e}")))?;

    // Write the OCI archive to docker load's stdin
    if let Some(ref mut stdin) = docker_load.stdin {
        use std::io::Write;
        stdin
            .write_all(&nix_output.stdout)
            .map_err(|e| CliError::general(format!("failed to write to docker load: {e}")))?;
    }

    let docker_result = docker_load
        .wait_with_output()
        .map_err(|e| CliError::general(format!("failed to wait for docker load: {e}")))?;

    if !docker_result.status.success() {
        let stderr = String::from_utf8_lossy(&docker_result.stderr);
        return Err(CliError::general(format!("docker load failed: {stderr}")));
    }

    // Tag the image with the bike name and requested tag
    // Nix builds moto-bike:latest, we tag it as {bike_name}:{tag}
    let tag_output = ProcessCommand::new("docker")
        .args(["tag", "moto-bike:latest", &format!("{}:{}", bike_name, tag)])
        .output()
        .map_err(|e| CliError::general(format!("failed to tag image: {e}")))?;

    if !tag_output.status.success() {
        let stderr = String::from_utf8_lossy(&tag_output.stderr);
        return Err(CliError::general(format!("failed to tag image: {stderr}")));
    }

    Ok(())
}

/// Run the bike command
pub async fn run(cmd: BikeCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        BikeAction::Build { tag, push } => {
            // Discover bike.toml - this validates the file exists and is valid
            let (_bike_path, config) = discover_bike()?;

            // Check Docker is running
            if !check_docker_running()? {
                return Err(CliError::general(
                    "Docker is not running. Please start Docker or Colima.\n\n\
                     On macOS with Colima: colima start\n\
                     On macOS with Docker Desktop: open the Docker app",
                ));
            }

            // Determine the image tag (default: git sha)
            let image_tag = match tag {
                Some(t) => t,
                None => get_git_sha()?,
            };

            // Build the container image
            build_bike_image(&config.name, &image_tag, flags.quiet)?;

            let image_ref = format!("{}:{}", config.name, image_tag);

            // Handle --push flag (not yet implemented, reserved for future)
            let pushed = if push {
                if !flags.quiet {
                    eprintln!("Note: --push not yet implemented");
                }
                false
            } else {
                false
            };

            // Output results
            if flags.json {
                let json = BikeBuildJson {
                    name: config.name.clone(),
                    image: image_ref.clone(),
                    pushed,
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Build complete: {}", image_ref);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_linux_target_format() {
        // This test verifies the function returns a properly formatted target
        let result = get_linux_target();
        assert!(result.is_ok());
        let target = result.unwrap();
        assert!(
            target == "aarch64-linux" || target == "x86_64-linux",
            "unexpected target: {target}"
        );
    }

    #[test]
    fn test_bike_build_json_serialization() {
        let json = BikeBuildJson {
            name: "test-service".to_string(),
            image: "test-service:abc123".to_string(),
            pushed: false,
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["name"], "test-service");
        assert_eq!(parsed["image"], "test-service:abc123");
        assert_eq!(parsed["pushed"], false);
    }

    #[test]
    fn test_bike_build_json_with_push() {
        let json = BikeBuildJson {
            name: "api".to_string(),
            image: "api:v1.0.0".to_string(),
            pushed: true,
        };

        let output = serde_json::to_string(&json).unwrap();
        assert!(output.contains("\"pushed\":true"));
    }
}
