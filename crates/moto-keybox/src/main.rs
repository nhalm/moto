//! moto-keybox: Secrets manager for the moto platform.
//!
//! This binary provides:
//! - SVID (SPIFFE-inspired identity) issuance
//! - Secret storage with envelope encryption
//! - ABAC (Attribute-Based Access Control) policy enforcement
//! - Audit logging
//!
//! # Configuration
//!
//! Required environment variables:
//! - `MOTO_KEYBOX_DATABASE_URL`: `PostgreSQL` connection string
//! - `MOTO_KEYBOX_MASTER_KEY_FILE`: Path to master key (KEK) file
//! - `MOTO_KEYBOX_SVID_SIGNING_KEY_FILE`: Path to Ed25519 signing key file
//!
//! Optional environment variables:
//! - `MOTO_KEYBOX_BIND_ADDR`: Server bind address (default: `0.0.0.0:8080`)
//! - `MOTO_KEYBOX_SVID_TTL_SECONDS`: SVID TTL in seconds (default: 900)
//! - `MOTO_KEYBOX_SERVICE_TOKEN`: Shared token for moto-club auth
//! - `RUST_LOG`: Log filter (default: `moto_keybox=info`)

mod abac;
mod api;
mod crypto;
mod db;
mod svid;

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::api::{AppState, router};
use crate::crypto::KeyManager;
use crate::db::DbPool;

/// Default bind address.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default SVID TTL in seconds (15 minutes).
const DEFAULT_SVID_TTL_SECS: u64 = 900;

/// Configuration parsed from environment variables.
struct Config {
    /// Database connection URL.
    database_url: String,
    /// Server bind address.
    bind_addr: SocketAddr,
    /// Path to master key file.
    master_key_file: String,
    /// Path to SVID signing key file.
    svid_signing_key_file: String,
    /// SVID TTL duration.
    svid_ttl: Duration,
    /// Service token for moto-club authentication.
    service_token: Option<String>,
}

impl Config {
    /// Parses configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid.
    fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("MOTO_KEYBOX_DATABASE_URL")
            .map_err(|_| ConfigError::Missing("MOTO_KEYBOX_DATABASE_URL"))?;

        let master_key_file = env::var("MOTO_KEYBOX_MASTER_KEY_FILE")
            .map_err(|_| ConfigError::Missing("MOTO_KEYBOX_MASTER_KEY_FILE"))?;

        let svid_signing_key_file = env::var("MOTO_KEYBOX_SVID_SIGNING_KEY_FILE")
            .map_err(|_| ConfigError::Missing("MOTO_KEYBOX_SVID_SIGNING_KEY_FILE"))?;

        let bind_addr = env::var("MOTO_KEYBOX_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| ConfigError::Invalid("MOTO_KEYBOX_BIND_ADDR", "invalid socket address"))?;

        let svid_ttl_secs = env::var("MOTO_KEYBOX_SVID_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_SVID_TTL_SECS);

        let service_token = env::var("MOTO_KEYBOX_SERVICE_TOKEN").ok();

        Ok(Self {
            database_url,
            bind_addr,
            master_key_file,
            svid_signing_key_file,
            svid_ttl: Duration::from_secs(svid_ttl_secs),
            service_token,
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
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("moto_keybox=info")),
        )
        .json()
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
        svid_ttl_secs = config.svid_ttl.as_secs(),
        "starting moto-keybox"
    );

    // Initialize key manager
    info!("loading encryption keys");
    let key_manager =
        KeyManager::from_files(&config.master_key_file, &config.svid_signing_key_file)?;
    info!("encryption keys loaded");

    // Connect to database
    info!("connecting to database");
    let db_pool: DbPool = db::connect(&config.database_url).await?;
    info!("database connected");

    // Create API router
    let state = AppState::new(
        db_pool,
        Arc::new(key_manager),
        config.svid_ttl,
        config.service_token,
    );
    let app = router(state);

    // Start server
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-keybox listening");

    axum::serve(listener, app).await?;

    Ok(())
}
