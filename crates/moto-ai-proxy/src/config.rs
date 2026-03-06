//! Configuration parsing for `moto-ai-proxy`.
//!
//! All configuration is read from `MOTO_AI_PROXY_*` environment variables.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Default listen address.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default keybox endpoint.
const DEFAULT_KEYBOX_URL: &str = "http://keybox.moto-system:8080";

/// Default SVID file path.
const DEFAULT_SVID_FILE: &str = "/var/run/secrets/svid/svid.jwt";

/// Default moto-club endpoint.
const DEFAULT_CLUB_URL: &str = "http://moto-club.moto-system:8080";

/// Default API key cache TTL in seconds (5 minutes).
const DEFAULT_KEY_CACHE_TTL_SECS: u64 = 300;

/// Default garage validation cache TTL in seconds.
const DEFAULT_GARAGE_CACHE_TTL_SECS: u64 = 60;

/// Configuration for ai-proxy parsed from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address for the proxy server.
    pub bind_addr: SocketAddr,
    /// Keybox endpoint for fetching API keys.
    pub keybox_url: String,
    /// Path to the ai-proxy SVID JWT file.
    pub svid_file: PathBuf,
    /// moto-club endpoint for garage validation.
    pub club_url: String,
    /// API key cache duration in seconds.
    pub key_cache_ttl_secs: u64,
    /// Garage validation cache duration in seconds.
    pub garage_cache_ttl_secs: u64,
    /// Custom model prefix → provider mappings (JSON).
    pub model_map: Option<String>,
}

impl Config {
    /// Parses configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if environment variables contain invalid values.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr = env::var("MOTO_AI_PROXY_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| {
                ConfigError::Invalid("MOTO_AI_PROXY_BIND_ADDR", "invalid socket address")
            })?;

        let keybox_url =
            env::var("MOTO_AI_PROXY_KEYBOX_URL").unwrap_or_else(|_| DEFAULT_KEYBOX_URL.to_string());

        let svid_file = env::var("MOTO_AI_PROXY_SVID_FILE")
            .unwrap_or_else(|_| DEFAULT_SVID_FILE.to_string())
            .into();

        let club_url =
            env::var("MOTO_AI_PROXY_CLUB_URL").unwrap_or_else(|_| DEFAULT_CLUB_URL.to_string());

        let key_cache_ttl_secs = env::var("MOTO_AI_PROXY_KEY_CACHE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_KEY_CACHE_TTL_SECS);

        let garage_cache_ttl_secs = env::var("MOTO_AI_PROXY_GARAGE_CACHE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_GARAGE_CACHE_TTL_SECS);

        let model_map = env::var("MOTO_AI_PROXY_MODEL_MAP").ok();

        Ok(Self {
            bind_addr,
            keybox_url,
            svid_file,
            club_url,
            key_cache_ttl_secs,
            garage_cache_ttl_secs,
            model_map,
        })
    }
}

/// Configuration errors.
#[derive(Debug)]
pub enum ConfigError {
    /// Environment variable has an invalid value.
    Invalid(&'static str, &'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(var, reason) => write!(f, "invalid {var}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        // Ensure the default bind address parses as a SocketAddr.
        let addr: SocketAddr = DEFAULT_BIND_ADDR.parse().unwrap();
        assert_eq!(addr.port(), 8080);
    }
}
