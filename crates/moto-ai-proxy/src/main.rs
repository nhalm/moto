//! moto-ai-proxy: AI provider reverse proxy for the moto platform.
//!
//! HTTP reverse proxy between garages and AI providers (Anthropic, `OpenAI`, Gemini).
//! Injects API credentials from keybox so garages never see real API keys.
//! Runs as a shared service in the `moto-system` namespace.
//!
//! # Configuration
//!
//! All configuration is read from `MOTO_AI_PROXY_*` environment variables:
//! - `MOTO_AI_PROXY_BIND_ADDR`: Listen address (default: `0.0.0.0:8080`)
//! - `MOTO_AI_PROXY_KEYBOX_URL`: Keybox endpoint (default: `http://keybox.moto-system:8080`)
//! - `MOTO_AI_PROXY_SVID_FILE`: Path to SVID JWT (default: `/var/run/secrets/svid/svid.jwt`)
//! - `MOTO_AI_PROXY_CLUB_URL`: moto-club endpoint (default: `http://moto-club.moto-system:8080`)
//! - `MOTO_AI_PROXY_KEY_CACHE_TTL_SECS`: API key cache duration (default: `300`)
//! - `MOTO_AI_PROXY_GARAGE_CACHE_TTL_SECS`: Garage validation cache duration (default: `60`)
//! - `MOTO_AI_PROXY_MODEL_MAP`: Custom model prefix → provider mappings (JSON)

use std::time::Duration;

use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use moto_ai_proxy::auth::ClubGarageValidator;
use moto_ai_proxy::config::Config;
use moto_ai_proxy::health;
use moto_ai_proxy::keys::KeyboxKeyStore;
use moto_ai_proxy::provider::ModelRouter;
use moto_ai_proxy::proxy;

use moto_keybox_client::{KeyboxClient, KeyboxConfig, SvidCache};

#[tokio::main]
async fn main() {
    // Initialize structured JSON logging to stdout
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("moto_ai_proxy=info")),
        )
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .init();

    if let Err(e) = run().await {
        error!(error = %e, "moto-ai-proxy failed to start");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    info!(
        bind_addr = %config.bind_addr,
        health_bind_addr = %config.health_bind_addr,
        keybox_url = %config.keybox_url,
        club_url = %config.club_url,
        key_cache_ttl_secs = config.key_cache_ttl_secs,
        garage_cache_ttl_secs = config.garage_cache_ttl_secs,
        model_map_configured = config.model_map.is_some(),
        "starting moto-ai-proxy"
    );

    // Start health server on port 8081 (per Engine Contract)
    let health_app = health::health_router();
    let health_listener = TcpListener::bind(config.health_bind_addr).await?;
    info!(addr = %config.health_bind_addr, "health server listening");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(health_listener, health_app).await {
            error!(error = %e, "health server failed");
        }
    });

    // Initialize keybox client with SVID for ai-proxy service identity.
    let svid_file = config.svid_file.to_string_lossy().to_string();
    let svid_cache = SvidCache::from_file(&svid_file).await.unwrap_or_else(|e| {
        info!(error = %e, svid_file = %svid_file, "SVID file not available, using empty cache (will retry on first request)");
        SvidCache::new()
    });

    let keybox_config = KeyboxConfig::new(&config.keybox_url);
    let keybox_client = KeyboxClient::new(keybox_config, svid_cache)?;

    let key_store = KeyboxKeyStore::new(
        keybox_client,
        Duration::from_secs(config.key_cache_ttl_secs),
    );

    // Mark startup complete — SVID is loaded (or will be retried on first request).
    health::mark_startup_complete();

    // Build HTTP client for upstream requests with timeouts per spec:
    // - Connect: 10s (TCP connection to upstream)
    // - Read/idle: 120s (max time between response chunks for streaming)
    // - Total: 600s (max total request duration, 10 min)
    // - First byte: 30s (handled in proxy.rs via tokio::time::timeout)
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120))
        .timeout(Duration::from_secs(600))
        .build()?;

    // Initialize garage validator for identity checks via moto-club.
    let garage_validator = ClubGarageValidator::new(
        client.clone(),
        config.club_url.clone(),
        Duration::from_secs(config.garage_cache_ttl_secs),
    );

    // Parse custom model mappings from MOTO_AI_PROXY_MODEL_MAP.
    let model_router = ModelRouter::new(config.model_map.as_deref())
        .map_err(|e| format!("invalid MOTO_AI_PROXY_MODEL_MAP: {e}"))?;

    // Build proxy router with passthrough routes, key injection, and garage auth
    let app = proxy::proxy_router(client, key_store, garage_validator, model_router);

    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(addr = %config.bind_addr, "moto-ai-proxy listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("moto-ai-proxy shutdown complete");

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
