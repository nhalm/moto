//! moto-club: Central orchestration server for the moto platform.
//!
//! This binary composes all the moto-club library crates and runs the server:
//! - REST API for garage and `WireGuard` coordination
//! - Database connection pool
//! - Kubernetes client
//! - Background reconciliation loop
//!
//! # Configuration
//!
//! Required environment variables:
//! - `MOTO_CLUB_DATABASE_URL`: `PostgreSQL` connection string
//!
//! Optional environment variables:
//! - `MOTO_CLUB_BIND_ADDR`: Server bind address (default: `0.0.0.0:8080`)
//! - `MOTO_CLUB_DEV_CONTAINER_IMAGE`: Dev container image (default: `ghcr.io/moto-dev/moto-garage:latest`)
//! - `MOTO_CLUB_RECONCILE_INTERVAL_SECONDS`: Reconciliation interval (default: 30)
//! - `MOTO_CLUB_DERP_CONFIG`: Path to DERP config file (default: `/etc/moto-club/derp.toml`)
//! - `KUBECONFIG`: Path to kubeconfig file (auto-detected in-cluster)
//! - `RUST_LOG`: Log filter (default: `moto_club=info`)

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use moto_club_api::{AppState, router};
use moto_club_db::{DbPool, derp_server_repo};
use moto_club_k8s::GarageK8s;
use moto_club_reconcile::{GarageReconciler, ReconcileConfig};
use moto_club_wg::{
    DERP_CONFIG_ENV_VAR, DerpMapManager, InMemoryDerpStore, InMemoryPeerStore,
    InMemorySessionStore, InMemorySshKeyStore, InMemoryStore, Ipam, PeerBroadcaster, PeerRegistry,
    SessionManager, SshKeyManager, load_derp_config,
};
use moto_k8s::K8sClient;

/// Default bind address.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default reconciliation interval in seconds.
const DEFAULT_RECONCILE_INTERVAL_SECS: u64 = 30;

/// Configuration parsed from environment variables.
struct Config {
    /// Database connection URL.
    database_url: String,
    /// Server bind address.
    bind_addr: SocketAddr,
    /// Dev container image.
    dev_container_image: Option<String>,
    /// Reconciliation interval.
    reconcile_interval: Duration,
}

impl Config {
    /// Parses configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid.
    fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("MOTO_CLUB_DATABASE_URL")
            .map_err(|_| ConfigError::Missing("MOTO_CLUB_DATABASE_URL"))?;

        let bind_addr = env::var("MOTO_CLUB_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| ConfigError::Invalid("MOTO_CLUB_BIND_ADDR", "invalid socket address"))?;

        let dev_container_image = env::var("MOTO_CLUB_DEV_CONTAINER_IMAGE").ok();

        let reconcile_interval = env::var("MOTO_CLUB_RECONCILE_INTERVAL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RECONCILE_INTERVAL_SECS);

        Ok(Self {
            database_url,
            bind_addr,
            dev_container_image,
            reconcile_interval: Duration::from_secs(reconcile_interval),
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
    // Initialize structured JSON logging to stdout per spec (moto-club.md lines 1183-1194)
    // - timestamp, level, message at top level
    // - custom fields (garage_id, owner, etc.) flattened into root object
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("moto_club=info")),
        )
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .init();

    if let Err(e) = run().await {
        error!(error = %e, "moto-club failed to start");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse configuration
    let config = Config::from_env()?;

    info!(
        bind_addr = %config.bind_addr,
        reconcile_interval_secs = config.reconcile_interval.as_secs(),
        "starting moto-club"
    );

    // Connect to database
    info!("connecting to database");
    let db_pool: DbPool = moto_club_db::connect(&config.database_url).await?;
    info!("database connected");

    // Load and sync DERP config
    let derp_config_path = env::var(DERP_CONFIG_ENV_VAR).ok();
    match load_derp_config().await {
        Ok(Some(config_file)) => {
            let servers: Vec<derp_server_repo::UpsertDerpServer> = config_file
                .regions
                .iter()
                .flat_map(|region| {
                    region
                        .nodes
                        .iter()
                        .map(|node| derp_server_repo::UpsertDerpServer {
                            region_id: i32::from(region.id),
                            region_name: region.name.clone(),
                            host: node.host.clone(),
                            port: i32::from(node.port),
                            stun_port: i32::from(node.stun_port),
                        })
                })
                .collect();

            let server_count = servers.len();
            let result = derp_server_repo::sync_from_config(&db_pool, servers).await?;

            info!(
                config_path = derp_config_path
                    .as_deref()
                    .unwrap_or("/etc/moto-club/derp.toml"),
                servers = server_count,
                inserted = result.inserted,
                updated = result.updated,
                deleted = result.deleted,
                "DERP config synced to database"
            );
        }
        Ok(None) => {
            info!(
                config_path = derp_config_path
                    .as_deref()
                    .unwrap_or("/etc/moto-club/derp.toml"),
                "DERP config file not found, using in-memory defaults"
            );
        }
        Err(e) => {
            warn!(
                error = %e,
                config_path = derp_config_path.as_deref().unwrap_or("/etc/moto-club/derp.toml"),
                "Failed to load DERP config, using in-memory defaults"
            );
        }
    }

    // Create K8s client
    info!("initializing kubernetes client");
    let k8s_client = K8sClient::new().await?;
    let garage_k8s = match &config.dev_container_image {
        Some(image) => GarageK8s::with_image(k8s_client, image),
        None => GarageK8s::new(k8s_client),
    };
    info!(
        dev_container_image = garage_k8s.dev_container_image(),
        "kubernetes client initialized"
    );

    // Create and start reconciler in background
    let reconcile_interval = config.reconcile_interval;
    let reconcile_config = ReconcileConfig::default().with_interval(reconcile_interval);
    let reconciler = GarageReconciler::new(db_pool.clone(), garage_k8s.clone(), reconcile_config);

    tokio::spawn(async move {
        info!(
            interval_secs = reconcile_interval.as_secs(),
            "starting reconciliation loop"
        );
        reconciler.run().await;
    });

    // Create WireGuard peer registry (in-memory for now)
    let ipam_store = InMemoryStore::new();
    let peer_store = InMemoryPeerStore::new();
    let ipam = Ipam::new(ipam_store);
    let peer_registry = Arc::new(PeerRegistry::new(peer_store, ipam));

    // Create WireGuard session manager (in-memory for now)
    let session_store = InMemorySessionStore::new();
    let session_manager = Arc::new(SessionManager::new(session_store));

    // Create DERP map manager with default configuration
    let derp_store = InMemoryDerpStore::with_default_map();
    let derp_manager = Arc::new(DerpMapManager::new(derp_store));

    // Create SSH key manager for user key registration
    let ssh_key_store = InMemorySshKeyStore::new();
    let ssh_key_manager = Arc::new(SshKeyManager::new(ssh_key_store));

    // Create peer broadcaster for garage WebSocket connections
    let peer_broadcaster = Arc::new(PeerBroadcaster::new());

    // Create API router
    let state = AppState::new(
        db_pool,
        peer_registry,
        session_manager,
        derp_manager,
        ssh_key_manager,
        peer_broadcaster,
    );
    let app = router(state);

    // Start server
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-club listening");

    axum::serve(listener, app).await?;

    Ok(())
}
