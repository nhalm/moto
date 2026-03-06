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

use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use moto_ai_proxy::config::Config;

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
        keybox_url = %config.keybox_url,
        club_url = %config.club_url,
        key_cache_ttl_secs = config.key_cache_ttl_secs,
        garage_cache_ttl_secs = config.garage_cache_ttl_secs,
        model_map_configured = config.model_map.is_some(),
        "starting moto-ai-proxy"
    );

    // Placeholder router — health endpoints and proxy routes will be added in later work items
    let app = axum::Router::new();

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
