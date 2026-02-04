//! moto-keybox: Secrets manager server for the moto platform.
//!
//! This binary runs the keybox API server providing:
//! - SVID token issuance for SPIFFE-inspired identity
//! - Secret CRUD operations with envelope encryption
//! - ABAC (Attribute-Based Access Control)
//! - Audit logging
//! - Health endpoints for K8s probes (port 8081)
//!
//! # Storage Modes
//!
//! The server supports two storage backends:
//! - **`PostgreSQL`** (recommended for production): Set `MOTO_KEYBOX_DATABASE_URL`
//! - **In-memory** (for testing only): Omit `MOTO_KEYBOX_DATABASE_URL`
//!
//! # Configuration
//!
//! Required environment variables:
//! - `MOTO_KEYBOX_MASTER_KEY_FILE`: Path to file containing base64-encoded KEK
//! - `MOTO_KEYBOX_SVID_SIGNING_KEY_FILE`: Path to file containing base64-encoded Ed25519 signing key
//!
//! Optional environment variables:
//! - `MOTO_KEYBOX_DATABASE_URL`: `PostgreSQL` connection string (enables persistent storage)
//! - `MOTO_KEYBOX_BIND_ADDR`: Server bind address (default: `0.0.0.0:8080`)
//! - `MOTO_KEYBOX_HEALTH_BIND_ADDR`: Health server bind address (default: `0.0.0.0:8081`)
//! - `MOTO_KEYBOX_ADMIN_SERVICE`: Service name with admin privileges (default: `moto-club`)
//! - `MOTO_KEYBOX_SVID_TTL_SECONDS`: SVID TTL in seconds (default: `900`)
//! - `MOTO_KEYBOX_SERVICE_TOKEN`: Static shared token for moto-club authentication
//! - `MOTO_KEYBOX_SERVICE_TOKEN_FILE`: Path to file containing service token (alternative to env var)
//! - `RUST_LOG`: Log filter (default: `moto_keybox=info`)

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use moto_keybox::{
    AppState, MasterKey, PgAppState, SvidIssuer, SvidValidator, health_router,
    mark_startup_complete, pg_router, router,
};

/// Default bind address for keybox API.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default bind address for health endpoints.
const DEFAULT_HEALTH_BIND_ADDR: &str = "0.0.0.0:8081";

/// Default SVID TTL in seconds (15 minutes).
const DEFAULT_SVID_TTL_SECS: i64 = 900;

/// Default admin service name.
const DEFAULT_ADMIN_SERVICE: &str = "moto-club";

/// Configuration parsed from environment variables.
struct Config {
    /// Server bind address.
    bind_addr: SocketAddr,
    /// Health server bind address.
    health_bind_addr: SocketAddr,
    /// Path to master key (KEK) file.
    master_key_file: PathBuf,
    /// Path to SVID signing key file.
    signing_key_file: PathBuf,
    /// Service name with admin privileges.
    admin_service: String,
    /// SVID TTL in seconds.
    svid_ttl_secs: i64,
    /// Service token for moto-club authentication.
    service_token: Option<String>,
    /// `PostgreSQL` database URL (if provided, uses `PostgreSQL` backend).
    database_url: Option<String>,
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

        let health_bind_addr = env::var("MOTO_KEYBOX_HEALTH_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_HEALTH_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| {
                ConfigError::Invalid("MOTO_KEYBOX_HEALTH_BIND_ADDR", "invalid socket address")
            })?;

        let admin_service = env::var("MOTO_KEYBOX_ADMIN_SERVICE")
            .unwrap_or_else(|_| DEFAULT_ADMIN_SERVICE.to_string());

        let svid_ttl_secs = env::var("MOTO_KEYBOX_SVID_TTL_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_SVID_TTL_SECS);

        // Load service token from env var or file
        let service_token = env::var("MOTO_KEYBOX_SERVICE_TOKEN").ok().or_else(|| {
            env::var("MOTO_KEYBOX_SERVICE_TOKEN_FILE")
                .ok()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(|s| s.trim().to_string())
        });

        // Database URL (optional - if not provided, uses in-memory storage)
        let database_url = env::var("MOTO_KEYBOX_DATABASE_URL").ok();

        Ok(Self {
            bind_addr,
            health_bind_addr,
            master_key_file,
            signing_key_file,
            admin_service,
            svid_ttl_secs,
            service_token,
            database_url,
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
        health_bind_addr = %config.health_bind_addr,
        svid_ttl_secs = config.svid_ttl_secs,
        admin_service = %config.admin_service,
        service_token_configured = config.service_token.is_some(),
        database_configured = config.database_url.is_some(),
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

    // Create the API router based on storage mode
    let app = if let Some(database_url) = config.database_url {
        // PostgreSQL mode
        info!("connecting to PostgreSQL database");
        let pool = moto_keybox_db::connect(&database_url)
            .await
            .map_err(|e| format!("failed to connect to database: {e}"))?;

        info!("running database migrations");
        moto_keybox_db::run_migrations(&pool)
            .await
            .map_err(|e| format!("failed to run migrations: {e}"))?;
        info!("database migrations complete");

        let mut state = PgAppState::new(
            pool,
            master_key,
            svid_issuer,
            svid_validator,
            &config.admin_service,
        );

        if let Some(token) = config.service_token {
            state = state.with_service_token(token);
            info!("service token configured for moto-club authentication");
        } else {
            tracing::warn!(
                "no service token configured - POST /auth/issue-garage-svid will be unavailable"
            );
        }

        info!("using PostgreSQL storage backend");
        pg_router(state)
    } else {
        // In-memory mode (for testing)
        tracing::warn!("no DATABASE_URL configured - using in-memory storage (NOT FOR PRODUCTION)");

        let mut state = AppState::new(
            master_key,
            svid_issuer,
            svid_validator,
            &config.admin_service,
        );

        if let Some(token) = config.service_token {
            state = state.with_service_token(token);
            info!("service token configured for moto-club authentication");
        } else {
            tracing::warn!(
                "no service token configured - POST /auth/issue-garage-svid will be unavailable"
            );
        }

        router(state)
    };

    // Start health server in background (port 8081)
    let health_app = health_router();
    let health_listener = TcpListener::bind(config.health_bind_addr).await?;
    info!(addr = %config.health_bind_addr, "health server listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(health_listener, health_app).await {
            error!(error = %e, "health server failed");
        }
    });

    // Start main API server
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-keybox listening");

    // Mark startup as complete - K8s startup probe will now return 200
    mark_startup_complete();
    info!("startup complete");

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
