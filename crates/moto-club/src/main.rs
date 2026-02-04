//! moto-club: Central orchestration server for the moto platform.
//!
//! This binary composes all the moto-club library crates and runs the server:
//! - REST API for garage and `WireGuard` coordination (port 8080)
//! - Health endpoints for K8s probes (port 8081)
//! - Prometheus metrics endpoint (port 9090)
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
//! - `MOTO_CLUB_HEALTH_BIND_ADDR`: Health server bind address (default: `0.0.0.0:8081`)
//! - `MOTO_CLUB_METRICS_BIND_ADDR`: Metrics server bind address (default: `0.0.0.0:9090`)
//! - `MOTO_CLUB_DEV_CONTAINER_IMAGE`: Dev container image (default: `ghcr.io/moto-dev/moto-garage:latest`)
//! - `MOTO_CLUB_RECONCILE_INTERVAL_SECONDS`: Reconciliation interval (default: 30)
//! - `MOTO_CLUB_DERP_CONFIG`: Path to DERP config file (default: `/etc/moto-club/derp.toml`)
//! - `KUBECONFIG`: Path to kubeconfig file (auto-detected in-cluster)
//! - `RUST_LOG`: Log filter (default: `moto_club=info`)

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use moto_club_api::{AppState, health_server_router, mark_startup_complete, router};
use moto_club_db::{DbPool, derp_server_repo};
use moto_club_garage::GarageService;
use moto_club_k8s::GarageK8s;
use moto_club_reconcile::{GarageReconciler, ReconcileConfig};
use moto_club_wg::{
    DERP_CONFIG_ENV_VAR, DerpMapManager, InMemoryDerpStore, InMemoryPeerStore,
    InMemorySessionStore, InMemoryStore, Ipam, PeerBroadcaster, PeerRegistry, SessionManager,
    load_derp_config,
};
use moto_k8s::K8sClient;

/// Default bind address for main API.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// Default bind address for health endpoints.
const DEFAULT_HEALTH_BIND_ADDR: &str = "0.0.0.0:8081";

/// Default bind address for Prometheus metrics.
const DEFAULT_METRICS_BIND_ADDR: &str = "0.0.0.0:9090";

/// Default reconciliation interval in seconds.
const DEFAULT_RECONCILE_INTERVAL_SECS: u64 = 30;

/// Graceful shutdown grace period in seconds (per moto-bike.md Engine Contract).
const SHUTDOWN_GRACE_PERIOD_SECS: u64 = 30;

/// Configuration parsed from environment variables.
struct Config {
    /// Database connection URL.
    database_url: String,
    /// Server bind address for main API.
    bind_addr: SocketAddr,
    /// Server bind address for health endpoints.
    health_bind_addr: SocketAddr,
    /// Server bind address for Prometheus metrics.
    metrics_bind_addr: SocketAddr,
    /// Dev container image.
    dev_container_image: Option<String>,
    /// Reconciliation interval.
    reconcile_interval: Duration,
    /// Keybox URL for health checks and SVID issuance.
    keybox_url: Option<String>,
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

        let health_bind_addr = env::var("MOTO_CLUB_HEALTH_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_HEALTH_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| {
                ConfigError::Invalid("MOTO_CLUB_HEALTH_BIND_ADDR", "invalid socket address")
            })?;

        let metrics_bind_addr = env::var("MOTO_CLUB_METRICS_BIND_ADDR")
            .unwrap_or_else(|_| DEFAULT_METRICS_BIND_ADDR.to_string())
            .parse()
            .map_err(|_| {
                ConfigError::Invalid("MOTO_CLUB_METRICS_BIND_ADDR", "invalid socket address")
            })?;

        let dev_container_image = env::var("MOTO_CLUB_DEV_CONTAINER_IMAGE").ok();

        let reconcile_interval = env::var("MOTO_CLUB_RECONCILE_INTERVAL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RECONCILE_INTERVAL_SECS);

        let keybox_url = env::var("MOTO_CLUB_KEYBOX_URL").ok();

        Ok(Self {
            database_url,
            bind_addr,
            health_bind_addr,
            metrics_bind_addr,
            dev_container_image,
            reconcile_interval: Duration::from_secs(reconcile_interval),
            keybox_url,
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

#[allow(clippy::too_many_lines)]
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Parse configuration
    let config = Config::from_env()?;

    info!(
        bind_addr = %config.bind_addr,
        health_bind_addr = %config.health_bind_addr,
        metrics_bind_addr = %config.metrics_bind_addr,
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

    // Create peer broadcaster for garage WebSocket connections
    let peer_broadcaster = Arc::new(PeerBroadcaster::new());

    // Create GarageService for full K8s integration in garage create flow
    let garage_service = GarageService::new(db_pool.clone(), garage_k8s.clone());
    info!("garage service initialized with full K8s integration");

    // Create API router with GarageService and GarageK8s for operations
    let api_garage_k8s = garage_k8s.clone();
    let mut state = AppState::new(
        db_pool,
        peer_registry,
        session_manager,
        derp_manager,
        peer_broadcaster,
    )
    .with_garage_k8s(api_garage_k8s)
    .with_garage_service(garage_service);

    // Add keybox URL for health checks if configured
    if let Some(ref keybox_url) = config.keybox_url {
        info!(keybox_url = %keybox_url, "keybox health checks enabled");
        state = state.with_keybox_url(keybox_url.clone());
    } else {
        info!("keybox URL not configured, health checks will skip keybox");
    }

    let app = router(state.clone());

    // Create health server router for K8s probes (port 8081)
    let health_app = health_server_router().with_state(state);

    // Start health server in background
    let health_listener = TcpListener::bind(config.health_bind_addr).await?;
    info!(addr = %config.health_bind_addr, "health server listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(health_listener, health_app).await {
            error!(error = %e, "health server failed");
        }
    });

    // Start Prometheus metrics server on port 9090 (per moto-bike.md Engine Contract)
    // Exports: http_requests_total, http_request_duration_seconds, process_cpu_seconds_total,
    // process_resident_memory_bytes (process metrics via metrics-process crate)
    let metrics_addr = config.metrics_bind_addr;
    tokio::spawn(async move {
        // Build and install the Prometheus exporter with HTTP listener
        let builder = PrometheusBuilder::new().with_http_listener(metrics_addr);

        if let Err(e) = builder.install() {
            error!(error = %e, "failed to install prometheus exporter");
            return;
        }

        info!(addr = %metrics_addr, "metrics server listening");

        // Create process metrics collector for CPU and memory metrics
        let process_collector = metrics_process::Collector::default();
        // Register help strings for process metrics
        process_collector.describe();

        // Periodically collect process metrics (every 5 seconds)
        loop {
            process_collector.collect();
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    // Start main API server
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-club listening");

    // Mark startup as complete - K8s startup probe will now return 200
    mark_startup_complete();
    info!("startup complete");

    // Graceful shutdown on SIGTERM (per moto-bike.md Engine Contract):
    // - Handle SIGTERM
    // - Stop accepting new requests
    // - Complete in-flight requests
    // - 30-second grace period
    // - Exit cleanly
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("moto-club shutdown complete");

    Ok(())
}

/// Waits for SIGTERM (Unix) or Ctrl+C to initiate graceful shutdown.
///
/// Per moto-bike.md Engine Contract, engines must handle SIGTERM to allow
/// in-flight requests to complete before termination.
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
            info!(
                grace_period_secs = SHUTDOWN_GRACE_PERIOD_SECS,
                "received Ctrl+C, initiating graceful shutdown"
            );
        }
        () = terminate => {
            info!(
                grace_period_secs = SHUTDOWN_GRACE_PERIOD_SECS,
                "received SIGTERM, initiating graceful shutdown"
            );
        }
    }
}
