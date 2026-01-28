//! Bike subcommands: build, deploy, list, logs.

use clap::{Args, Subcommand};
use moto_k8s::{BikeDeploymentConfig, DeploymentOps, K8sClient};
use serde::Serialize;
use std::process::{Command as ProcessCommand, Stdio};

use crate::bike::{BikeConfig, discover_bike};
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

    /// Deploy a bike to the current context
    Deploy {
        /// Deploy specific image (default: latest local build)
        #[arg(long)]
        image: Option<String>,

        /// Override replica count
        #[arg(long)]
        replicas: Option<u32>,

        /// Target namespace (default: current context namespace)
        #[arg(long, short = 'n')]
        namespace: Option<String>,

        /// Wait for deployment to complete
        #[arg(long)]
        wait: bool,

        /// Timeout for --wait (default: 5m)
        #[arg(long, default_value = "5m")]
        wait_timeout: String,
    },

    /// List bikes in the current context
    List {
        /// Target namespace (default: current context namespace)
        #[arg(long, short = 'n')]
        namespace: Option<String>,
    },
}

/// JSON output for bike build
#[derive(Serialize)]
struct BikeBuildJson {
    name: String,
    image: String,
    pushed: bool,
}

/// JSON output for bike deploy
#[derive(Serialize)]
struct BikeDeployJson {
    name: String,
    image: String,
    replicas: u32,
    status: String,
}

/// JSON output for bike list
#[derive(Serialize)]
struct BikeListJson {
    bikes: Vec<BikeJson>,
}

/// JSON representation of a bike (matches spec)
#[derive(Serialize)]
struct BikeJson {
    name: String,
    status: String,
    replicas_ready: i32,
    replicas_desired: i32,
    age_seconds: i64,
    image: String,
}

/// Parse a duration string like "5m", "1h", "2d" into seconds.
fn parse_duration(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(CliError::invalid_input("empty duration"));
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str
        .parse()
        .map_err(|_| CliError::invalid_input(format!("invalid duration number: {num_str}")))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(CliError::invalid_input(format!(
                "invalid duration unit: {unit} (use s, m, h, or d)"
            )));
        }
    };

    Ok(num * multiplier)
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

/// Get the registry from MOTO_REGISTRY env var.
fn get_registry() -> Option<String> {
    std::env::var("MOTO_REGISTRY").ok()
}

/// Push an image to the registry.
fn push_image(image_ref: &str, quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!("Pushing {}...", image_ref);
    }

    let output = ProcessCommand::new("docker")
        .args(["push", image_ref])
        .output()
        .map_err(|e| CliError::general(format!("failed to push image: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::general(format!("docker push failed: {stderr}")));
    }

    Ok(())
}

/// Get the latest local image tag for a bike.
/// Checks docker images to find the most recent build.
fn get_latest_local_image(bike_name: &str) -> Result<String> {
    let output = ProcessCommand::new("docker")
        .args(["images", "--format", "{{.Tag}}", bike_name])
        .output()
        .map_err(|e| CliError::general(format!("failed to list docker images: {e}")))?;

    if !output.status.success() {
        return Err(CliError::general("failed to list docker images"));
    }

    let tags = String::from_utf8_lossy(&output.stdout);
    let first_tag = tags.lines().next();

    match first_tag {
        Some(tag) if !tag.is_empty() => Ok(format!("{}:{}", bike_name, tag)),
        _ => Err(CliError::invalid_input(format!(
            "No local image found for '{}'. Build first with: moto bike build",
            bike_name
        ))),
    }
}

/// Convert BikeConfig to BikeDeploymentConfig for K8s deployment.
fn to_deployment_config(
    config: &BikeConfig,
    image: &str,
    replicas_override: Option<u32>,
) -> BikeDeploymentConfig {
    BikeDeploymentConfig {
        name: config.name.clone(),
        image: image.to_string(),
        replicas: replicas_override.unwrap_or(config.deploy.replicas),
        port: config.deploy.port,
        health_port: config.health.port,
        health_path: config.health.path.clone(),
        cpu_request: config.resources.cpu_request.clone(),
        cpu_limit: config.resources.cpu_limit.clone(),
        memory_request: config.resources.memory_request.clone(),
        memory_limit: config.resources.memory_limit.clone(),
    }
}

/// Get the current kubectl namespace from kubeconfig.
fn get_current_namespace() -> Result<String> {
    let output = ProcessCommand::new("kubectl")
        .args([
            "config",
            "view",
            "--minify",
            "--output",
            "jsonpath={..namespace}",
        ])
        .output()
        .map_err(|e| CliError::general(format!("failed to get current namespace: {e}")))?;

    let ns = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // If no namespace is set in the context, default to "default"
    Ok(if ns.is_empty() {
        "default".to_string()
    } else {
        ns
    })
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

            let local_image_ref = format!("{}:{}", config.name, image_tag);

            // Handle --push flag
            let (image_ref, pushed) = if push {
                let registry = get_registry().ok_or_else(|| {
                    CliError::general(
                        "MOTO_REGISTRY environment variable not set.\n\n\
                         Set it to your container registry, e.g.:\n\
                         export MOTO_REGISTRY=ghcr.io/myorg\n\
                         export MOTO_REGISTRY=localhost:5000",
                    )
                })?;

                // Full image reference with registry prefix
                let registry_image_ref = format!("{}/{}:{}", registry, config.name, image_tag);

                // Tag the local image with the registry prefix
                let tag_output = ProcessCommand::new("docker")
                    .args(["tag", &local_image_ref, &registry_image_ref])
                    .output()
                    .map_err(|e| {
                        CliError::general(format!("failed to tag image for registry: {e}"))
                    })?;

                if !tag_output.status.success() {
                    let stderr = String::from_utf8_lossy(&tag_output.stderr);
                    return Err(CliError::general(format!(
                        "failed to tag image for registry: {stderr}"
                    )));
                }

                // Push to registry
                push_image(&registry_image_ref, flags.quiet)?;

                (registry_image_ref, true)
            } else {
                (local_image_ref, false)
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

        BikeAction::Deploy {
            image,
            replicas,
            namespace,
            wait,
            wait_timeout,
        } => {
            // Discover bike.toml - this validates the file exists and is valid
            let (_bike_path, config) = discover_bike()?;

            // Determine the image to deploy
            let image_ref = match image {
                Some(img) => img,
                None => get_latest_local_image(&config.name)?,
            };

            // Get namespace: use --namespace if provided, otherwise current context namespace
            let namespace = match namespace {
                Some(ns) => ns,
                None => get_current_namespace()?,
            };

            // Parse wait timeout
            let timeout_seconds = parse_duration(&wait_timeout)?;
            let timeout = std::time::Duration::from_secs(timeout_seconds);

            // Effective replica count
            let effective_replicas = replicas.unwrap_or(config.deploy.replicas);

            if !flags.quiet {
                eprintln!("Deploying {}...", image_ref);
            }

            // Create K8s client and deploy
            let client = K8sClient::new().await?;
            let deploy_config = to_deployment_config(&config, &image_ref, replicas);
            client.deploy_bike(&namespace, &deploy_config).await?;

            // Wait for deployment if requested
            if wait {
                if !flags.quiet {
                    eprint!("  Waiting for pods... ");
                }

                let deployment_name = format!("moto-{}", config.name);
                client
                    .wait_for_deployment(&namespace, &deployment_name, timeout)
                    .await?;

                if !flags.quiet {
                    eprintln!("{}/{} ready", effective_replicas, effective_replicas);
                }
            }

            // Output results
            if flags.json {
                let json = BikeDeployJson {
                    name: config.name.clone(),
                    image: image_ref.clone(),
                    replicas: effective_replicas,
                    status: "deployed".to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Deployment complete.");
            }
        }

        BikeAction::List { namespace } => {
            // Get namespace: use --namespace if provided, otherwise current context namespace
            let namespace = match namespace {
                Some(ns) => ns,
                None => get_current_namespace()?,
            };

            // Create K8s client and list bikes
            let client = K8sClient::new().await?;
            let bikes = client.list_bikes(&namespace).await?;

            if flags.json {
                let json = BikeListJson {
                    bikes: bikes
                        .iter()
                        .map(|b| BikeJson {
                            name: b.name.clone(),
                            status: b.status.clone(),
                            replicas_ready: b.replicas_ready,
                            replicas_desired: b.replicas_desired,
                            age_seconds: b.age_seconds,
                            image: b.image.clone(),
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if bikes.is_empty() {
                if !flags.quiet {
                    println!("No bikes found.");
                }
            } else {
                println!(
                    "{:<14} {:<10} {:<10} {:<8} {}",
                    "NAME", "STATUS", "REPLICAS", "AGE", "IMAGE"
                );
                for bike in bikes {
                    let replicas = format!("{}/{}", bike.replicas_ready, bike.replicas_desired);
                    let age = format_duration(bike.age_seconds);
                    println!(
                        "{:<14} {:<10} {:<10} {:<8} {}",
                        truncate(&bike.name, 14),
                        bike.status,
                        replicas,
                        age,
                        truncate_image(&bike.image, 40)
                    );
                }
            }
        }
    }

    Ok(())
}

/// Truncate a string to a maximum length, adding "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Truncate an image reference, preferring to show the tag.
fn truncate_image(image: &str, max_len: usize) -> String {
    if image.len() <= max_len {
        return image.to_string();
    }

    // Try to preserve the tag portion
    if let Some(colon_pos) = image.rfind(':') {
        let tag = &image[colon_pos..];
        let name = &image[..colon_pos];
        let available = max_len.saturating_sub(tag.len()).saturating_sub(3);
        if available > 0 {
            return format!("{}...{}", &name[..available.min(name.len())], tag);
        }
    }

    format!("{}...", &image[..max_len - 3])
}

/// Format a duration in seconds as a human-readable string (e.g., "2h15m", "45m", "3d").
fn format_duration(seconds: i64) -> String {
    if seconds < 0 {
        return "0s".to_string();
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        if hours > 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{hours}h")
        }
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
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

    #[test]
    fn test_bike_build_json_with_registry_image() {
        let json = BikeBuildJson {
            name: "club".to_string(),
            image: "ghcr.io/moto/club:abc123".to_string(),
            pushed: true,
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["name"], "club");
        assert_eq!(parsed["image"], "ghcr.io/moto/club:abc123");
        assert_eq!(parsed["pushed"], true);
    }

    #[test]
    fn test_bike_deploy_json_serialization() {
        let json = BikeDeployJson {
            name: "club".to_string(),
            image: "club:abc123".to_string(),
            replicas: 2,
            status: "deployed".to_string(),
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["name"], "club");
        assert_eq!(parsed["image"], "club:abc123");
        assert_eq!(parsed["replicas"], 2);
        assert_eq!(parsed["status"], "deployed");
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("1s").unwrap(), 1);
        assert_eq!(parse_duration("120s").unwrap(), 120);
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("1m").unwrap(), 60);
        assert_eq!(parse_duration("10m").unwrap(), 600);
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("1d").unwrap(), 86400);
    }

    #[test]
    fn test_parse_duration_invalid_unit() {
        assert!(parse_duration("5x").is_err());
    }

    #[test]
    fn test_parse_duration_empty() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("  ").is_err());
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3540), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h1m");
        assert_eq!(format_duration(8100), "2h15m");
        assert_eq!(format_duration(7200), "2h");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d1h");
        assert_eq!(format_duration(172800), "2d");
    }

    #[test]
    fn test_format_duration_negative() {
        assert_eq!(format_duration(-100), "0s");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly10c", 10), "exactly10c");
        assert_eq!(truncate("this is too long", 10), "this is...");
    }

    #[test]
    fn test_truncate_image_short() {
        assert_eq!(truncate_image("api:v1", 40), "api:v1");
    }

    #[test]
    fn test_truncate_image_long() {
        let image = "ghcr.io/moto/very-long-image-name:abc123";
        let truncated = truncate_image(image, 30);
        assert!(truncated.len() <= 30);
        assert!(truncated.ends_with(":abc123"));
    }

    #[test]
    fn test_bike_list_json_serialization() {
        let json = BikeListJson {
            bikes: vec![BikeJson {
                name: "api-service".to_string(),
                status: "running".to_string(),
                replicas_ready: 2,
                replicas_desired: 2,
                age_seconds: 259200,
                image: "api-service:abc123f".to_string(),
            }],
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["bikes"][0]["name"], "api-service");
        assert_eq!(parsed["bikes"][0]["status"], "running");
        assert_eq!(parsed["bikes"][0]["replicas_ready"], 2);
        assert_eq!(parsed["bikes"][0]["replicas_desired"], 2);
        assert_eq!(parsed["bikes"][0]["age_seconds"], 259200);
        assert_eq!(parsed["bikes"][0]["image"], "api-service:abc123f");
    }
}
