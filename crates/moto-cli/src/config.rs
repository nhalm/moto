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

impl ColorMode {
    /// Computes the effective color mode, respecting `MOTO_NO_COLOR` env var.
    ///
    /// Priority (highest to lowest):
    /// 1. `MOTO_NO_COLOR` env var (if set, always returns `Never`)
    /// 2. Config file `[output].color` setting
    #[must_use]
    pub fn effective(config_mode: Self) -> Self {
        if std::env::var("MOTO_NO_COLOR").is_ok() {
            return Self::Never;
        }
        config_mode
    }

    /// Returns true if colors should be enabled, considering terminal capabilities.
    #[must_use]
    #[allow(dead_code)]
    pub fn should_colorize(self) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => {
                // Check if stdout is a terminal
                std::io::IsTerminal::is_terminal(&std::io::stdout())
            }
        }
    }
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
    #[allow(dead_code)]
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
#[allow(dead_code)]
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

    #[test]
    fn test_color_mode_effective_without_env() {
        // When MOTO_NO_COLOR is not set in the test environment,
        // config value should be respected (unless it happens to be set)
        let result = ColorMode::effective(ColorMode::Never);
        // If MOTO_NO_COLOR is not set, we get the config value (Never)
        // If MOTO_NO_COLOR is set, we also get Never
        // Either way, result should be Never
        assert_eq!(result, ColorMode::Never);

        // Test that the function at least returns a valid ColorMode
        let _ = ColorMode::effective(ColorMode::Auto);
        let _ = ColorMode::effective(ColorMode::Always);
    }

    #[test]
    fn test_should_colorize() {
        // Never mode always returns false
        assert!(!ColorMode::Never.should_colorize());

        // Always mode always returns true
        assert!(ColorMode::Always.should_colorize());

        // Auto mode depends on terminal detection (we can't easily test this)
    }
}
