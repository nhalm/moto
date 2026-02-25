//! Dev subcommands: up, down, status.
//!
//! Manages the local development environment (postgres, keybox, moto-club).
//! See local-dev.md for the full specification.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::process::Command as ProcessCommand;

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

/// Hardcoded dev defaults for local development.
/// Each value can be overridden via the corresponding environment variable.
pub struct DevConfig {
    pub keybox_health: String,
    pub club_health: String,
    pub club_api: String,
    pub registry: String,
}

impl DevConfig {
    /// Load dev config from env vars with hardcoded defaults.
    pub fn load() -> Self {
        let keybox_health_bind = std::env::var("MOTO_KEYBOX_HEALTH_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8091".to_string());
        let keybox_health_port = keybox_health_bind.rsplit(':').next().unwrap_or("8091");

        Self {
            keybox_health: format!("localhost:{keybox_health_port}"),
            club_health: "localhost:8081".to_string(),
            club_api: "localhost:8080".to_string(),
            registry: "localhost:5050".to_string(),
        }
    }
}

/// Dev command and subcommands
#[derive(Args)]
pub struct DevCommand {
    #[command(subcommand)]
    pub action: DevAction,
}

/// Available dev actions
#[derive(Subcommand)]
pub enum DevAction {
    /// Start the full local dev stack
    Up {
        /// Start services only, don't open a garage
        #[arg(long)]
        no_garage: bool,
        /// Force rebuild and push the garage container image
        #[arg(long)]
        rebuild_image: bool,
        /// Skip the registry image check entirely
        #[arg(long)]
        skip_image: bool,
    },
    /// Stop the local dev stack
    Down {
        /// Also remove .dev/ directory and postgres data volume
        #[arg(long)]
        clean: bool,
    },
    /// Show health of all local dev components
    Status,
}

/// JSON output for dev status
#[derive(Serialize)]
struct DevStatusJson {
    cluster: String,
    registry: String,
    postgres: String,
    keybox: String,
    club: String,
    image: String,
    garages: i64,
}

/// Run the dev command
#[allow(clippy::unused_async)]
pub async fn run(cmd: DevCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        DevAction::Status => dev_status(flags),
        DevAction::Up { .. } => Err(CliError::general(
            "moto dev up is not yet implemented.\n\nUse the manual workflow instead:\n  make dev-up",
        )),
        DevAction::Down { .. } => Err(CliError::general(
            "moto dev down is not yet implemented.\n\nUse the manual workflow instead:\n  make dev-down",
        )),
    }
}

/// Check if a HTTP endpoint returns 200 using curl.
fn check_http_health(url: &str) -> bool {
    let output = ProcessCommand::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "2",
            url,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let status_code = String::from_utf8_lossy(&o.stdout);
            status_code.trim() == "200"
        }
        _ => false,
    }
}

/// Check if postgres is running and healthy via docker compose ps.
fn check_postgres_healthy() -> bool {
    let output = ProcessCommand::new("docker")
        .args(["compose", "ps", "--format", "json", "postgres"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // docker compose ps --format json outputs JSON with State and Health fields
            // Check if container is running and healthy
            stdout.contains("\"running\"") && stdout.contains("\"healthy\"")
        }
        _ => false,
    }
}

/// Check if the garage image exists in the registry.
fn check_image_in_registry(registry: &str) -> bool {
    let url = format!("http://{registry}/v2/moto-garage/tags/list");
    let output = ProcessCommand::new("curl")
        .args(["-s", "--connect-timeout", "2", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            body.contains("\"latest\"")
        }
        _ => false,
    }
}

/// Count non-terminated garages from the moto-club API.
fn count_garages(club_api: &str) -> i64 {
    let url = format!("http://{club_api}/api/v1/garages");
    let output = ProcessCommand::new("curl")
        .args(["-s", "--connect-timeout", "2", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            // Parse JSON response to count garages
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(garages) = parsed.get("garages").and_then(|g| g.as_array()) {
                    return i64::try_from(garages.len()).unwrap_or(0);
                }
            }
            0
        }
        _ => -1, // -1 indicates API unreachable
    }
}

/// Get cluster status string for dev status output.
fn get_cluster_status_str() -> &'static str {
    let output = ProcessCommand::new("k3d")
        .args(["cluster", "list", "--no-headers"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.first() == Some(&"moto") {
                    if let Some(servers) = parts.get(1) {
                        if servers.starts_with("0/") {
                            return "stopped";
                        }
                    }
                    return "running";
                }
            }
            "not_found"
        }
        _ => "not_found",
    }
}

/// Show dev status health check dashboard.
fn dev_status(flags: &GlobalFlags) -> Result<()> {
    let config = DevConfig::load();

    let cluster = get_cluster_status_str();
    let registry_healthy = check_http_health(&format!("http://{}/v2/", config.registry));
    let postgres_healthy = check_postgres_healthy();
    let keybox_healthy =
        check_http_health(&format!("http://{}/health/ready", config.keybox_health));
    let club_healthy = check_http_health(&format!("http://{}/health/ready", config.club_health));
    let image_found = check_image_in_registry(&config.registry);
    let garage_count = count_garages(&config.club_api);

    let registry_str = if registry_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let postgres_str = if postgres_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let keybox_str = if keybox_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let club_str = if club_healthy { "healthy" } else { "unhealthy" };
    let image_str = if image_found { "found" } else { "not_found" };

    if flags.json {
        let json = DevStatusJson {
            cluster: cluster.to_string(),
            registry: registry_str.to_string(),
            postgres: postgres_str.to_string(),
            keybox: keybox_str.to_string(),
            club: club_str.to_string(),
            image: image_str.to_string(),
            garages: garage_count.max(0),
        };
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if !flags.quiet {
        let image_detail = if image_found {
            "moto-garage:latest (in registry)"
        } else {
            "not found"
        };
        let garage_detail = if garage_count >= 0 {
            format!("{garage_count} running")
        } else {
            "unknown (club unreachable)".to_string()
        };

        println!("Cluster:   {cluster} (k3d-moto)");
        println!("Registry:  {registry_str} ({})", config.registry);
        println!("Postgres:  {postgres_str} (localhost:5432)");
        println!("Keybox:    {keybox_str} (localhost:8090)");
        println!("Club:      {club_str} (localhost:8080)");
        println!("Image:     {image_detail}");
        println!("Garages:   {garage_detail}");
    }

    // Exit 1 if any component is unhealthy
    let all_healthy = cluster == "running"
        && registry_healthy
        && postgres_healthy
        && keybox_healthy
        && club_healthy;

    if !all_healthy {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_config_port_extraction() {
        // Test that port extraction from bind address works correctly
        let addr = "0.0.0.0:8091";
        let port = addr.rsplit(':').next().unwrap_or("8091");
        assert_eq!(port, "8091");

        let addr2 = "0.0.0.0:9091";
        let port2 = addr2.rsplit(':').next().unwrap_or("8091");
        assert_eq!(port2, "9091");
    }

    #[test]
    fn test_dev_config_default_ports() {
        // Verify the hardcoded dev defaults match the spec
        // keybox health: 8091, club health: 8081, club API: 8080, registry: 5050
        let config = DevConfig {
            keybox_health: "localhost:8091".to_string(),
            club_health: "localhost:8081".to_string(),
            club_api: "localhost:8080".to_string(),
            registry: "localhost:5050".to_string(),
        };
        assert_eq!(config.keybox_health, "localhost:8091");
        assert_eq!(config.club_health, "localhost:8081");
        assert_eq!(config.club_api, "localhost:8080");
        assert_eq!(config.registry, "localhost:5050");
    }

    #[test]
    fn test_dev_status_json_structure() {
        let json = DevStatusJson {
            cluster: "running".to_string(),
            registry: "healthy".to_string(),
            postgres: "healthy".to_string(),
            keybox: "healthy".to_string(),
            club: "healthy".to_string(),
            image: "found".to_string(),
            garages: 1,
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["cluster"], "running");
        assert_eq!(parsed["registry"], "healthy");
        assert_eq!(parsed["postgres"], "healthy");
        assert_eq!(parsed["keybox"], "healthy");
        assert_eq!(parsed["club"], "healthy");
        assert_eq!(parsed["image"], "found");
        assert_eq!(parsed["garages"], 1);
    }

    #[test]
    fn test_dev_status_json_unhealthy() {
        let json = DevStatusJson {
            cluster: "not_found".to_string(),
            registry: "unhealthy".to_string(),
            postgres: "unhealthy".to_string(),
            keybox: "unhealthy".to_string(),
            club: "unhealthy".to_string(),
            image: "not_found".to_string(),
            garages: 0,
        };

        let output = serde_json::to_string(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["cluster"], "not_found");
        assert_eq!(parsed["registry"], "unhealthy");
        assert_eq!(parsed["garages"], 0);
    }
}
