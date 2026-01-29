//! moto-keybox: Secrets manager server for the moto platform.
//!
//! This binary runs the keybox API server providing:
//! - SVID token issuance for SPIFFE-inspired identity
//! - Secret CRUD operations with envelope encryption
//! - ABAC (Attribute-Based Access Control)
//! - Audit logging
//!
//! # Configuration
//!
//! Required environment variables:
//! - `MOTO_KEYBOX_MASTER_KEY_FILE`: Path to file containing base64-encoded KEK
//! - `MOTO_KEYBOX_SVID_SIGNING_KEY_FILE`: Path to file containing base64-encoded Ed25519 signing key
//!
//! Optional environment variables:
//! - `MOTO_KEYBOX_BIND_ADDR`: Server bind address (default: `0.0.0.0:8080`)
//! - `MOTO_KEYBOX_ADMIN_SERVICE`: Service name with admin privileges (default: `moto-club`)
//! - `MOTO_KEYBOX_SVID_TTL_SECONDS`: SVID TTL in seconds (default: `900`)
//! - `RUST_LOG`: Log filter (default: `moto_keybox=info`)

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use moto_keybox::{AppState, MasterKey, SvidIssuer, SvidValidator, router};

/// Default bind address for keybox API.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default SVID TTL in seconds (15 minutes).
const DEFAULT_SVID_TTL_SECS: i64 = 900;

/// Default admin service name.
const DEFAULT_ADMIN_SERVICE: &str = "moto-club";

/// Configuration parsed from environment variables.
struct Config {
    /// Server bind address.
    bind_addr: SocketAddr,
    /// Path to master key (KEK) file.
    master_key_file: PathBuf,
    /// Path to SVID signing key file.
    signing_key_file: PathBuf,
    /// Service name with admin privileges.
    admin_service: String,
    /// SVID TTL in seconds.
    svid_ttl_secs: i64,
}

impl Config {
    /// Parses configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid.
    fn from_env() -> Result<Self, ConfigError> {
        let master_key_file = env::var("MOTO_KEYBOX_MASTER_KEY_FILE")
            .map_err(|_| ConfigError::Missing("MOTO_KEYBOX_MASTER_KEY_FILE"))?
            .into();

        let signing_key_file = env::var("MOTO_KEYBOX_SVID_SIGNING_KEY_FILE")
            .map_err(|_| ConfigError::Missing("MOTO_KEYBOX_SVID_SIGNING_KEY_FILE"))?
            .into();

        let bind_addr = env::var("MOTO_KEYBOX_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| ConfigError::Invalid("MOTO_KEYBOX_BIND_ADDR", "invalid socket address"))?;

        let admin_service = env::var("MOTO_KEYBOX_ADMIN_SERVICE")
            .unwrap_or_else(|_| DEFAULT_ADMIN_SERVICE.to_string());

        let svid_ttl_secs = env::var("MOTO_KEYBOX_SVID_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_SVID_TTL_SECS);

        Ok(Self {
            bind_addr,
            master_key_file,
            signing_key_file,
            admin_service,
            svid_ttl_secs,
        })
    }
}

/// Configuration errors.
#[derive(Debug)]
enum ConfigError {
    /// Required environment variable is missing.
    Missing(&'static str),
    /// Environment variable has an invalid value.
    Invalid(&'static str, &'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(var) => write!(f, "missing required environment variable: {var}"),
            Self::Invalid(var, reason) => write!(f, "invalid {var}: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[tokio::main]
async fn main() {
    // Initialize structured JSON logging to stdout
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("moto_keybox=info")),
        )
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .init();

    if let Err(e) = run().await {
        error!(error = %e, "moto-keybox failed to start");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse configuration
    let config = Config::from_env()?;

    info!(
        bind_addr = %config.bind_addr,
        svid_ttl_secs = config.svid_ttl_secs,
        admin_service = %config.admin_service,
        "starting moto-keybox"
    );

    // Load master key (KEK)
    info!(path = %config.master_key_file.display(), "loading master key");
    let master_key = MasterKey::from_file(&config.master_key_file)
        .map_err(|e| format!("failed to load master key: {e}"))?;
    info!("master key loaded");

    // Load SVID signing key
    info!(path = %config.signing_key_file.display(), "loading SVID signing key");
    let svid_issuer = SvidIssuer::from_file(&config.signing_key_file)
        .map_err(|e| format!("failed to load SVID signing key: {e}"))?
        .with_ttl(config.svid_ttl_secs);
    let svid_validator = SvidValidator::new(svid_issuer.verifying_key());
    info!("SVID signing key loaded");

    // Create application state with in-memory repository
    let state = AppState::new(
        master_key,
        svid_issuer,
        svid_validator,
        &config.admin_service,
    );
    let app = router(state);

    // Start API server
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-keybox listening");

    // Graceful shutdown on SIGTERM or Ctrl+C
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("moto-keybox shutdown complete");

    Ok(())
}

/// Waits for SIGTERM (Unix) or Ctrl+C to initiate graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {
            info!("received Ctrl+C, initiating graceful shutdown");
        }
        () = terminate => {
            info!("received SIGTERM, initiating graceful shutdown");
        }
    }
}
