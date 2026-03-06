//! moto-club API client.
//!
//! This module provides an HTTP client for communicating with the moto-club
//! REST API. It handles:
//!
//! - Device registration (`POST /api/v1/wg/devices`)
//! - Session creation (`POST /api/v1/wg/sessions`)
//! - Session management (`GET/DELETE /api/v1/wg/sessions`)
//!
//! # Example
//!
//! ```ignore
//! use moto_cli_wgtunnel::client::{MotoClubClient, MotoClubConfig};
//!
//! let config = MotoClubConfig::new("http://localhost:8080", "my-username");
//! let client = MotoClubClient::new(config)?;
//!
//! // Register a device (WG public key IS the device identity)
//! let device = client.register_device(&public_key, Some("my-laptop")).await?;
//! println!("Device registered with IP: {}", device.overlay_ip);
//!
//! // Create a session using the garage UUID and device public key
//! let session = client.create_session(garage_id, &public_key, None).await?;
//! println!("Session created: {}", session.session_id);
//! ```

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_tungstenite::tungstenite;
use tracing::{debug, warn};
use uuid::Uuid;

/// Default moto-club base URL for local development.
pub const DEFAULT_MOTO_CLUB_URL: &str = "http://localhost:18080";

/// Default request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Retry configuration for device registration.
pub const DEVICE_REGISTRATION_RETRIES: u32 = 3;

/// Retry delays for device registration (exponential backoff).
pub const DEVICE_REGISTRATION_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// Errors that can occur when communicating with moto-club.
#[derive(Debug, Error)]
pub enum ClientError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Server returned an error response.
    #[error("server error: {code} - {message}")]
    Server {
        /// Error code from the server.
        code: String,
        /// Error message from the server.
        message: String,
    },

    /// Failed to parse response.
    #[error("failed to parse response: {0}")]
    ParseError(String),

    /// Device registration failed after retries.
    #[error("device registration failed after {attempts} attempts: {last_error}")]
    DeviceRegistrationFailed {
        /// Number of attempts made.
        attempts: u32,
        /// Last error encountered.
        last_error: String,
    },

    /// Session creation failed.
    #[error("session creation failed: {0}")]
    SessionCreationFailed(String),

    /// Garage not found.
    #[error("garage not found: {0}")]
    GarageNotFound(String),

    /// Not authorized to access garage.
    #[error("not authorized to access garage: {0}")]
    NotAuthorized(String),

    /// moto-club is unreachable.
    #[error("moto-club unreachable at {url}: {reason}")]
    Unreachable {
        /// The URL that was attempted.
        url: String,
        /// Why it's unreachable.
        reason: String,
    },
}

/// Configuration for the moto-club client.
#[derive(Debug, Clone)]
pub struct MotoClubConfig {
    /// Base URL for moto-club (e.g., `http://localhost:8080`).
    pub base_url: String,

    /// Owner/username for authentication (used as Bearer token in local dev).
    pub owner: String,

    /// Request timeout.
    pub timeout: Duration,
}

impl MotoClubConfig {
    /// Create a new moto-club client configuration.
    #[must_use]
    pub fn new(base_url: impl Into<String>, owner: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            owner: owner.into(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }

    /// Set the request timeout.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Create configuration from environment variables.
    ///
    /// Uses:
    /// - `MOTO_CLUB_URL` for base URL (default: `http://localhost:18080`)
    /// - `MOTO_USER` for owner (required)
    ///
    /// # Errors
    ///
    /// Returns an error if `MOTO_USER` is not set.
    pub fn from_env() -> Result<Self, ClientError> {
        let base_url =
            std::env::var("MOTO_CLUB_URL").unwrap_or_else(|_| DEFAULT_MOTO_CLUB_URL.to_string());

        let owner = std::env::var("MOTO_USER").map_err(|_| ClientError::Server {
            code: "CONFIG_ERROR".to_string(),
            message: "MOTO_USER environment variable is required".to_string(),
        })?;

        Ok(Self::new(base_url, owner))
    }
}

/// Client for communicating with moto-club.
pub struct MotoClubClient {
    /// HTTP client.
    client: Client,

    /// Configuration.
    config: MotoClubConfig,
}

impl MotoClubClient {
    /// Create a new moto-club client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(config: MotoClubConfig) -> Result<Self, ClientError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(ClientError::Request)?;

        Ok(Self { client, config })
    }

    /// Get the authorization header value.
    fn auth_header(&self) -> String {
        format!("Bearer {}", self.config.owner)
    }

    /// Register a device with moto-club.
    ///
    /// This registers the device's `WireGuard` public key with moto-club,
    /// which allocates an overlay IP address for the device.
    ///
    /// # Arguments
    ///
    /// * `public_key` - Device's `WireGuard` public key
    /// * `device_name` - Optional human-readable device name
    ///
    /// # Returns
    ///
    /// Device registration response with assigned IP.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable (after retries)
    /// - Server returns an error
    pub async fn register_device(
        &self,
        public_key: &WgPublicKey,
        device_name: Option<&str>,
    ) -> Result<DeviceResponse, ClientError> {
        let url = format!("{}/api/v1/wg/devices", self.config.base_url);

        let request = RegisterDeviceRequest {
            public_key: public_key.to_base64(),
            device_name: device_name.map(str::to_string),
        };

        debug!(url = %url, "registering device with moto-club");

        // Retry with exponential backoff
        let mut last_error = String::new();
        for (attempt, delay) in DEVICE_REGISTRATION_DELAYS.iter().enumerate() {
            // attempt index fits in u32 since DEVICE_REGISTRATION_DELAYS is small
            let attempt = u32::try_from(attempt).unwrap_or(u32::MAX) + 1;

            match self.post_json::<_, DeviceResponse>(&url, &request).await {
                Ok(response) => {
                    debug!(
                        public_key = %response.public_key,
                        overlay_ip = %response.overlay_ip,
                        "device registered successfully"
                    );
                    return Ok(response);
                }
                Err(e) => {
                    last_error = e.to_string();
                    warn!(
                        attempt = attempt,
                        max_attempts = DEVICE_REGISTRATION_RETRIES,
                        error = %last_error,
                        "device registration failed, retrying"
                    );

                    if attempt < DEVICE_REGISTRATION_RETRIES {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }
        }

        Err(ClientError::DeviceRegistrationFailed {
            attempts: DEVICE_REGISTRATION_RETRIES,
            last_error,
        })
    }

    /// Create a tunnel session for connecting to a garage.
    ///
    /// # Arguments
    ///
    /// * `garage_id` - Garage UUID
    /// * `device_pubkey` - Device's `WireGuard` public key (device identity)
    /// * `ttl_seconds` - Optional session TTL (defaults to garage TTL)
    ///
    /// # Returns
    ///
    /// Session response with garage connection info.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Garage is not found
    /// - User is not authorized
    pub async fn create_session(
        &self,
        garage_id: Uuid,
        device_pubkey: &WgPublicKey,
        ttl_seconds: Option<u32>,
    ) -> Result<SessionResponse, ClientError> {
        let url = format!("{}/api/v1/wg/sessions", self.config.base_url);

        let request = CreateSessionRequest {
            garage_id,
            device_pubkey: device_pubkey.clone(),
            ttl_seconds,
        };

        debug!(url = %url, garage = %garage_id, "creating tunnel session");

        self.post_json(&url, &request).await
    }

    /// List active sessions for the current user.
    ///
    /// # Errors
    ///
    /// Returns an error if moto-club is unreachable.
    pub async fn list_sessions(&self) -> Result<ListSessionsResponse, ClientError> {
        let url = format!("{}/api/v1/wg/sessions", self.config.base_url);

        debug!(url = %url, "listing sessions");

        self.get_json(&url).await
    }

    /// Close a tunnel session.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Session ID to close
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Session is not found
    pub async fn close_session(&self, session_id: &str) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/wg/sessions/{}", self.config.base_url, session_id);

        debug!(url = %url, session_id = %session_id, "closing session");

        let response = self
            .client
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Get garage details by name.
    ///
    /// # Arguments
    ///
    /// * `garage_name` - Garage name
    ///
    /// # Returns
    ///
    /// Garage details including the UUID needed for session creation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Garage is not found
    /// - User is not authorized to access the garage
    pub async fn get_garage(
        &self,
        garage_name: &str,
    ) -> Result<GarageDetailsResponse, ClientError> {
        let url = format!("{}/api/v1/garages/{}", self.config.base_url, garage_name);

        debug!(url = %url, garage = %garage_name, "getting garage details");

        self.get_json(&url).await
    }

    /// List all garages for the current user.
    ///
    /// # Errors
    ///
    /// Returns an error if moto-club is unreachable.
    pub async fn list_garages(&self) -> Result<ListGaragesResponse, ClientError> {
        let url = format!("{}/api/v1/garages", self.config.base_url);

        debug!(url = %url, "listing garages");

        self.get_json(&url).await
    }

    /// Create a new garage.
    ///
    /// # Arguments
    ///
    /// * `request` - Garage creation parameters
    ///
    /// # Returns
    ///
    /// Created garage details.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Name already exists (`GARAGE_ALREADY_EXISTS`)
    /// - Invalid TTL (`INVALID_TTL`)
    pub async fn create_garage(
        &self,
        request: &CreateGarageRequest,
    ) -> Result<GarageDetailsResponse, ClientError> {
        let url = format!("{}/api/v1/garages", self.config.base_url);

        debug!(url = %url, name = ?request.name, "creating garage");

        self.post_json(&url, request).await
    }

    /// Close (delete) a garage by name.
    ///
    /// # Arguments
    ///
    /// * `garage_name` - Name of the garage to close
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Garage is not found
    /// - User is not authorized
    pub async fn close_garage(&self, garage_name: &str) -> Result<(), ClientError> {
        let url = format!("{}/api/v1/garages/{}", self.config.base_url, garage_name);

        debug!(url = %url, garage = %garage_name, "closing garage");

        let response = self
            .client
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Extend a garage's TTL.
    ///
    /// # Arguments
    ///
    /// * `garage_name` - Name of the garage to extend
    /// * `seconds` - Seconds to add to current expiry
    ///
    /// # Returns
    ///
    /// New expiration info.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - moto-club is unreachable
    /// - Garage is not found
    /// - Garage has expired or is terminated
    /// - Extension would exceed max TTL
    pub async fn extend_garage(
        &self,
        garage_name: &str,
        seconds: i64,
    ) -> Result<ExtendGarageResponse, ClientError> {
        let url = format!(
            "{}/api/v1/garages/{}/extend",
            self.config.base_url, garage_name
        );

        let request = ExtendGarageRequest { seconds };

        debug!(url = %url, garage = %garage_name, seconds = %seconds, "extending garage TTL");

        self.post_json(&url, &request).await
    }

    /// Stream garage logs over WebSocket.
    ///
    /// Connects to `/ws/v1/garages/{name}/logs` and returns a receiver of log lines.
    /// Each received string is a raw log line (extracted from the JSON `log` message).
    /// On `error` or `eof` messages, the stream ends.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection cannot be established.
    pub async fn stream_logs_ws(
        &self,
        garage_name: &str,
        tail: i64,
        follow: bool,
        since: Option<&str>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String, String>>, ClientError> {
        // Convert http(s):// to ws(s)://
        let ws_base = if self.config.base_url.starts_with("https://") {
            self.config.base_url.replacen("https://", "wss://", 1)
        } else {
            self.config.base_url.replacen("http://", "ws://", 1)
        };

        let mut url =
            format!("{ws_base}/ws/v1/garages/{garage_name}/logs?tail={tail}&follow={follow}");
        if let Some(s) = since {
            use std::fmt::Write;
            let _ = write!(url, "&since={s}");
        }

        debug!(url = %url, "connecting to log WebSocket");

        let request = tungstenite::http::Request::builder()
            .uri(&url)
            .header("Authorization", self.auth_header())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| ClientError::Server {
                code: "WEBSOCKET_ERROR".to_string(),
                message: format!("failed to build WebSocket request: {e}"),
            })?;

        // Use the configured timeout for connecting
        let tcp_stream = tokio::time::timeout(
            self.config.timeout,
            tokio::net::TcpStream::connect(
                request
                    .uri()
                    .authority()
                    .map_or("localhost:18080", tungstenite::http::uri::Authority::as_str),
            ),
        )
        .await
        .map_err(|_| ClientError::Unreachable {
            url: url.clone(),
            reason: "connection timed out".to_string(),
        })?
        .map_err(|e| ClientError::Unreachable {
            url: url.clone(),
            reason: e.to_string(),
        })?;

        let (ws_stream, _response) = tokio_tungstenite::client_async(request, tcp_stream)
            .await
            .map_err(|e| ClientError::Unreachable {
                url: url.clone(),
                reason: format!("WebSocket handshake failed: {e}"),
            })?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            while let Some(msg_result) = ws_receiver.next().await {
                match msg_result {
                    Ok(tungstenite::Message::Text(text)) => {
                        // Parse the JSON message
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                            match value.get("type").and_then(|t| t.as_str()) {
                                Some("log") => {
                                    if let Some(line) = value.get("line").and_then(|l| l.as_str())
                                        && tx.send(Ok(format!("{line}\n"))).await.is_err()
                                    {
                                        break;
                                    }
                                }
                                Some("error") => {
                                    let msg = value
                                        .get("message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("unknown error");
                                    let _ = tx.send(Err(msg.to_string())).await;
                                    break;
                                }
                                Some("eof") => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(tungstenite::Message::Ping(data)) => {
                        let _ = ws_sender.send(tungstenite::Message::Pong(data)).await;
                    }
                    Ok(tungstenite::Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    /// Send a POST request with JSON body and parse JSON response.
    #[allow(clippy::future_not_send)] // Self is not Sync, but this is an internal method
    async fn post_json<Req, Resp>(&self, url: &str, body: &Req) -> Result<Resp, ClientError>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .post(url)
            .header("Authorization", self.auth_header())
            .json(body)
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ClientError::ParseError(format!("failed to parse response: {e}")))
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Send a GET request and parse JSON response.
    async fn get_json<Resp>(&self, url: &str) -> Result<Resp, ClientError>
    where
        Resp: for<'de> Deserialize<'de>,
    {
        let response = self
            .client
            .get(url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|e| ClientError::ParseError(format!("failed to parse response: {e}")))
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Convert a reqwest error to a connection error if appropriate.
    fn connection_error(&self, error: reqwest::Error) -> ClientError {
        if error.is_connect() || error.is_timeout() {
            ClientError::Unreachable {
                url: self.config.base_url.clone(),
                reason: error.to_string(),
            }
        } else {
            ClientError::Request(error)
        }
    }

    /// Handle an error response from the server.
    async fn handle_error_response<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, ClientError> {
        let status = response.status();

        // Try to parse error response
        let error: ApiErrorResponse = match response.json().await {
            Ok(e) => e,
            Err(_) => {
                return Err(ClientError::Server {
                    code: "UNKNOWN".to_string(),
                    message: format!("HTTP {status}"),
                });
            }
        };

        let code = error.error.code;
        let message = error.error.message;

        // Map known error codes to specific errors
        match code.as_str() {
            "GARAGE_NOT_FOUND" => Err(ClientError::GarageNotFound(message)),
            "GARAGE_NOT_OWNED" => Err(ClientError::NotAuthorized(message)),
            _ => Err(ClientError::Server { code, message }),
        }
    }
}

// ============================================================================
// Request/Response types (matching moto-club-api)
// ============================================================================

/// Request to register a device.
#[derive(Debug, Clone, Serialize)]
struct RegisterDeviceRequest {
    /// Device's `WireGuard` public key (base64).
    public_key: String,
    /// Optional human-readable device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
}

/// Response for device registration.
///
/// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
/// No separate device ID - the public key is the identifier.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceResponse {
    /// Device's `WireGuard` public key (this IS the device identity).
    pub public_key: WgPublicKey,
    /// Assigned overlay IP address.
    #[serde(rename = "assigned_ip")]
    pub overlay_ip: OverlayIp,
    /// Optional human-readable device name.
    pub device_name: Option<String>,
    /// When the device was registered.
    pub created_at: String,
}

/// Request to create a tunnel session.
///
/// Per spec (moto-club.md v1.1): Uses `device_pubkey` (`WireGuard` public key IS the device identity).
#[derive(Debug, Clone, Serialize)]
struct CreateSessionRequest {
    /// Garage to connect to (UUID).
    garage_id: Uuid,
    /// Device's `WireGuard` public key (this IS the device identity).
    device_pubkey: WgPublicKey,
    /// Optional session TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_seconds: Option<u32>,
}

/// Response for session creation.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionResponse {
    /// Session ID assigned by moto-club.
    pub session_id: String,
    /// Garage connection information.
    pub garage: GarageInfo,
    /// Client's assigned overlay IP.
    pub client_ip: OverlayIp,
    /// DERP map for relay fallback.
    pub derp_map: DerpMap,
    /// Session expiration time (ISO 8601).
    pub expires_at: String,
}

/// Garage connection information from session response.
#[derive(Debug, Clone, Deserialize)]
pub struct GarageInfo {
    /// Garage's `WireGuard` public key (base64).
    pub public_key: String,
    /// Garage's overlay IP.
    pub overlay_ip: OverlayIp,
    /// Garage's direct endpoints (if known).
    #[serde(default)]
    pub endpoints: Vec<String>,
}

/// Response for listing sessions.
#[derive(Debug, Clone, Deserialize)]
pub struct ListSessionsResponse {
    /// Active sessions.
    pub sessions: Vec<SessionInfo>,
}

/// Session info for listing.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    /// Unique session identifier.
    pub session_id: String,
    /// Garage this session connects to.
    pub garage_id: String,
    /// Human-readable garage name.
    pub garage_name: String,
    /// When this session was created.
    pub created_at: String,
    /// When this session expires.
    pub expires_at: String,
}

/// Response for getting garage details.
#[derive(Debug, Clone, Deserialize)]
pub struct GarageDetailsResponse {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// Human-friendly name.
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Git branch.
    #[serde(default)]
    pub branch: String,
    /// Current status.
    pub status: String,
    /// Dev container image used.
    #[serde(default)]
    pub image: String,
    /// Time-to-live in seconds.
    pub ttl_seconds: i32,
    /// When the garage expires (ISO 8601).
    pub expires_at: String,
    /// Kubernetes namespace.
    #[serde(default)]
    pub namespace: String,
    /// Kubernetes pod name.
    #[serde(default)]
    pub pod_name: String,
    /// When the garage was created (ISO 8601).
    pub created_at: String,
    /// Engine name (optional).
    #[serde(default)]
    pub engine: Option<String>,
}

/// Response for listing garages.
#[derive(Debug, Clone, Deserialize)]
pub struct ListGaragesResponse {
    /// List of garages.
    pub garages: Vec<GarageDetailsResponse>,
}

/// Request to create a garage.
#[derive(Debug, Clone, Serialize, Default)]
pub struct CreateGarageRequest {
    /// Human-friendly name (auto-generated if omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Git branch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Time-to-live in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<i64>,
    /// Override dev container image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Engine name (what the garage is working on).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// Include `PostgreSQL` database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_postgres: Option<bool>,
    /// Include Redis cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_redis: Option<bool>,
}

/// Request to extend a garage's TTL.
#[derive(Debug, Clone, Serialize)]
struct ExtendGarageRequest {
    /// Seconds to add to current expiry.
    seconds: i64,
}

/// Response for extending a garage's TTL.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtendGarageResponse {
    /// New expiration time (ISO 8601).
    pub expires_at: String,
    /// Remaining TTL in seconds.
    pub ttl_remaining_seconds: i64,
}

/// API error response format.
#[derive(Debug, Clone, Deserialize)]
struct ApiErrorResponse {
    /// Error details.
    error: ApiErrorDetail,
}

/// API error detail.
#[derive(Debug, Clone, Deserialize)]
struct ApiErrorDetail {
    /// Error code.
    code: String,
    /// Error message.
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moto_club_config_defaults() {
        let config = MotoClubConfig::new("http://localhost:8080", "testuser");
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.owner, "testuser");
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn moto_club_config_with_timeout() {
        let config = MotoClubConfig::new("http://localhost:8080", "testuser")
            .with_timeout(Duration::from_secs(60));
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn device_response_deserialize() {
        // WireGuard public key IS the device identity (Cloudflare WARP model)
        // API returns public_key, not device_id
        let json = r#"{
            "public_key": "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=",
            "assigned_ip": "fd00:6d6f:746f:2::1",
            "device_name": "my-laptop",
            "created_at": "2026-01-21T10:00:00Z"
        }"#;
        let response: DeviceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.device_name, Some("my-laptop".to_string()));
        assert_eq!(response.created_at, "2026-01-21T10:00:00Z");
        // Verify the overlay IP was parsed correctly
        assert!(response.overlay_ip.is_client());
    }

    #[test]
    fn session_response_deserialize() {
        let json = r#"{
            "session_id": "sess_123",
            "garage": {
                "public_key": "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=",
                "overlay_ip": "fd00:6d6f:746f:1::abc",
                "endpoints": ["10.0.0.1:51820"]
            },
            "client_ip": "fd00:6d6f:746f:2::1",
            "derp_map": { "regions": {} },
            "expires_at": "2026-01-22T12:00:00Z"
        }"#;
        let response: SessionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.session_id, "sess_123");
        assert_eq!(response.garage.endpoints.len(), 1);
    }

    #[test]
    fn list_sessions_response_deserialize() {
        let json = r#"{
            "sessions": [
                {
                    "session_id": "sess_123",
                    "garage_id": "abc123",
                    "garage_name": "my-garage",
                    "created_at": "2026-01-22T10:00:00Z",
                    "expires_at": "2026-01-22T14:00:00Z"
                }
            ]
        }"#;
        let response: ListSessionsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].garage_name, "my-garage");
    }
}
