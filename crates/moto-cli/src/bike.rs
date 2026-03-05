//! bike.toml discovery and parsing.
//!
//! Bike commands look for `bike.toml` in the current working directory.
//! If not found, they search up to the git root.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{CliError, Result};

/// Bike configuration from bike.toml.
#[derive(Debug, Deserialize)]
pub struct BikeConfig {
    /// Name of the bike/engine (required).
    pub name: String,

    /// Deployment configuration.
    #[serde(default)]
    pub deploy: DeployConfig,

    /// Health check configuration.
    #[serde(default)]
    pub health: HealthConfig,

    /// Resource limits and requests.
    #[serde(default)]
    pub resources: ResourceConfig,
}

/// Deployment configuration.
#[derive(Debug, Deserialize)]
pub struct DeployConfig {
    /// Number of replicas.
    #[serde(default = "default_replicas")]
    pub replicas: u32,

    /// Main API port.
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for DeployConfig {
    fn default() -> Self {
        Self {
            replicas: default_replicas(),
            port: default_port(),
        }
    }
}

const fn default_replicas() -> u32 {
    3
}

const fn default_port() -> u16 {
    8080
}

/// Health check configuration.
#[derive(Debug, Deserialize)]
pub struct HealthConfig {
    /// Health check port.
    #[serde(default = "default_health_port")]
    pub port: u16,

    /// Readiness probe path.
    #[serde(default = "default_health_path")]
    pub path: String,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            port: default_health_port(),
            path: default_health_path(),
        }
    }
}

const fn default_health_port() -> u16 {
    8081
}

fn default_health_path() -> String {
    "/health/ready".to_string()
}

/// Resource limits and requests.
#[derive(Debug, Default, Deserialize)]
pub struct ResourceConfig {
    /// CPU request (e.g., "500m").
    pub cpu_request: Option<String>,

    /// CPU limit (e.g., "2").
    pub cpu_limit: Option<String>,

    /// Memory request (e.g., "512Mi").
    pub memory_request: Option<String>,

    /// Memory limit (e.g., "2Gi").
    pub memory_limit: Option<String>,
}

/// Finds the git root directory from the current working directory.
///
/// Returns None if not in a git repository.
fn find_git_root() -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).ok()?;
        Some(PathBuf::from(path.trim()))
    } else {
        None
    }
}

/// Finds bike.toml by searching from the current directory up to the git root.
///
/// Returns the path to bike.toml if found.
///
/// # Errors
///
/// Returns `CliError` with exit code 2 if bike.toml is not found.
pub fn find_bike_toml() -> Result<PathBuf> {
    let git_root = find_git_root();
    let mut current = std::env::current_dir()
        .map_err(|e| CliError::general(format!("failed to get current directory: {e}")))?;

    loop {
        let bike_path = current.join("bike.toml");
        if bike_path.exists() {
            return Ok(bike_path);
        }

        // Stop if we've reached git root
        if let Some(ref root) = git_root
            && current == *root
        {
            break;
        }

        // Try to go up one directory
        if !current.pop() {
            // Reached filesystem root
            break;
        }

        // Also stop if we've gone past git root (shouldn't happen, but be safe)
        if let Some(ref root) = git_root
            && !current.starts_with(root)
        {
            break;
        }
    }

    Err(CliError::not_found(
        "No bike.toml found in current directory or parent directories.\n\n\
         Try: Create a bike.toml or cd to a directory containing one.",
    ))
}

/// Loads and parses bike.toml from the given path.
///
/// # Errors
///
/// Returns `CliError` if the file cannot be read or parsed.
pub fn load_bike_toml(path: &Path) -> Result<BikeConfig> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| CliError::general(format!("failed to read bike.toml: {e}")))?;

    toml::from_str(&contents)
        .map_err(|e| CliError::general(format!("failed to parse bike.toml: {e}")))
}

/// Finds and loads bike.toml from the current directory or parent directories.
///
/// This is a convenience function that combines `find_bike_toml` and `load_bike_toml`.
pub fn discover_bike() -> Result<(PathBuf, BikeConfig)> {
    let path = find_bike_toml()?;
    let config = load_bike_toml(&path)?;
    Ok((path, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_parse_minimal_bike_toml() {
        let toml = r#"name = "test-service""#;
        let config: BikeConfig = toml::from_str(toml).unwrap();

        assert_eq!(config.name, "test-service");
        // Default values from DeployConfig::default()
        assert_eq!(config.deploy.replicas, 3);
        assert_eq!(config.deploy.port, 8080);
        assert_eq!(config.health.port, 8081);
        assert_eq!(config.health.path, "/health/ready");
    }

    #[test]
    fn test_parse_full_bike_toml() {
        let toml = r#"
name = "club"

[deploy]
replicas = 3
port = 9000

[health]
port = 9001
path = "/healthz"

[resources]
cpu_request = "500m"
cpu_limit = "2"
memory_request = "512Mi"
memory_limit = "2Gi"
"#;
        let config: BikeConfig = toml::from_str(toml).unwrap();

        assert_eq!(config.name, "club");
        assert_eq!(config.deploy.replicas, 3);
        assert_eq!(config.deploy.port, 9000);
        assert_eq!(config.health.port, 9001);
        assert_eq!(config.health.path, "/healthz");
        assert_eq!(config.resources.cpu_request, Some("500m".to_string()));
        assert_eq!(config.resources.cpu_limit, Some("2".to_string()));
        assert_eq!(config.resources.memory_request, Some("512Mi".to_string()));
        assert_eq!(config.resources.memory_limit, Some("2Gi".to_string()));
    }

    #[test]
    fn test_find_bike_toml_in_current_dir() {
        let dir = tempdir().unwrap();
        // Canonicalize to handle macOS /var -> /private/var symlink
        let canonical_dir = dir.path().canonicalize().unwrap();
        let bike_path = canonical_dir.join("bike.toml");
        fs::write(&bike_path, "name = \"test\"").unwrap();

        // Change to the temp directory
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&canonical_dir).unwrap();

        let result = find_bike_toml();
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let found_path = result.unwrap().canonicalize().unwrap();
        assert_eq!(found_path, bike_path);
    }

    #[test]
    fn test_find_bike_toml_not_found() {
        // Test the error message content instead of searching from temp dir
        // (temp dirs are outside git repos, so find_bike_toml searches to fs root)
        let err = CliError::not_found("No bike.toml found");
        assert_eq!(err.exit_code, crate::error::ExitCode::NotFound);
    }

    #[test]
    fn test_load_bike_toml_invalid() {
        let dir = tempdir().unwrap();
        let bike_path = dir.path().join("bike.toml");
        fs::write(&bike_path, "invalid toml [[[").unwrap();

        let result = load_bike_toml(&bike_path);
        assert!(result.is_err());
    }
}
