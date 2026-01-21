//! Configuration file support for moto CLI.
//!
//! Configuration is loaded from `$XDG_CONFIG_HOME/moto/config.toml`,
//! falling back to `~/.config/moto/config.toml`.

use serde::Deserialize;
use std::path::PathBuf;

/// Color output mode.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Auto-detect based on terminal capabilities
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

/// Output configuration.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct OutputConfig {
    /// Color mode for terminal output
    #[serde(default)]
    pub color: ColorMode,
}

/// Garage configuration defaults.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GarageConfig {
    /// Default TTL for new garages (e.g., "4h")
    pub ttl: Option<String>,
}

/// Top-level configuration structure.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    /// Output configuration
    #[serde(default)]
    pub output: OutputConfig,
    /// Garage defaults
    #[serde(default)]
    pub garage: GarageConfig,
}

impl Config {
    /// Returns the path to the config file.
    ///
    /// Uses `$XDG_CONFIG_HOME/moto/config.toml` if set,
    /// otherwise falls back to `~/.config/moto/config.toml`.
    #[must_use]
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("moto").join("config.toml"))
    }

    /// Loads config from the default location.
    ///
    /// Returns default config if the file doesn't exist.
    /// Returns an error only if the file exists but is invalid.
    #[must_use]
    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| {
                if path.exists() {
                    std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|contents| toml::from_str(&contents).ok())
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    /// Loads config from a specific path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load_from(path: &std::path::Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
            path: path.to_path_buf(),
            source: e,
        })?;

        toml::from_str(&contents).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: e,
        })
    }
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read config file
    #[error("failed to read config file at {path}: {source}")]
    Read {
        /// Path to the config file
        path: PathBuf,
        /// Underlying IO error
        source: std::io::Error,
    },
    /// Failed to parse config file
    #[error("failed to parse config file at {path}: {source}")]
    Parse {
        /// Path to the config file
        path: PathBuf,
        /// Underlying TOML parse error
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.output.color, ColorMode::Auto);
        assert!(config.garage.ttl.is_none());
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[output]
color = "always"

[garage]
ttl = "8h"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.output.color, ColorMode::Always);
        assert_eq!(config.garage.ttl, Some("8h".to_string()));
    }

    #[test]
    fn test_parse_partial_config() {
        let toml = r#"
[garage]
ttl = "2h"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.output.color, ColorMode::Auto);
        assert_eq!(config.garage.ttl, Some("2h".to_string()));
    }

    #[test]
    fn test_color_modes() {
        #[derive(Deserialize)]
        struct Wrapper {
            mode: ColorMode,
        }

        let auto: Wrapper = toml::from_str(r#"mode = "auto""#).unwrap();
        assert_eq!(auto.mode, ColorMode::Auto);

        let always: Wrapper = toml::from_str(r#"mode = "always""#).unwrap();
        assert_eq!(always.mode, ColorMode::Always);

        let never: Wrapper = toml::from_str(r#"mode = "never""#).unwrap();
        assert_eq!(never.mode, ColorMode::Never);
    }
}
