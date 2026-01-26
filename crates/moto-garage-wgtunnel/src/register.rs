//! Registration with moto-club coordination server.
//!
//! When a garage pod starts, it must register with moto-club to:
//! 1. Advertise its ephemeral `WireGuard` public key
//! 2. Receive an overlay IP address
//! 3. Get the DERP map for relay fallback
//!
//! # API
//!
//! ```text
//! POST /api/v1/wg/garages
//! Authorization: Bearer <k8s-service-account-token>
//!
//! {
//!   "garage_id": "abc123",
//!   "public_key": "base64-encoded-garage-wg-public-key",
//!   "endpoints": ["10.42.0.5:51820"]
//! }
//!
//! Response 200:
//! {
//!   "assigned_ip": "fd00:moto:1::abc1",
//!   "derp_map": { ... }
//! }
//! ```
//!
//! # Example
//!
//! ```ignore
//! use moto_garage_wgtunnel::register::{GarageRegistrar, RegistrationConfig};
//! use moto_wgtunnel_types::keys::WgPrivateKey;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = RegistrationConfig {
//!     moto_club_url: "https://moto-club.example.com".to_string(),
//!     garage_id: "my-garage".to_string(),
//!     auth_token: "k8s-service-account-token".to_string(),
//! };
//!
//! let private_key = WgPrivateKey::generate();
//! let registrar = GarageRegistrar::new(config);
//!
//! let response = registrar.register(&private_key, &[]).await?;
//! println!("Assigned IP: {}", response.assigned_ip);
//! # Ok(())
//! # }
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use moto_wgtunnel_types::derp::DerpMap;
use moto_wgtunnel_types::ip::OverlayIp;
use moto_wgtunnel_types::keys::{WgPrivateKey, WgPublicKey};

/// Default timeout for registration requests.
pub const DEFAULT_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Retry backoff intervals for registration (1s, 2s, 4s).
pub const RETRY_BACKOFF: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// Error type for registration operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    /// moto-club server is unreachable.
    #[error("moto-club unreachable: {0}")]
    Unreachable(String),

    /// Request failed with an HTTP error.
    #[error("registration failed: HTTP {status}: {message}")]
    HttpError {
        /// HTTP status code.
        status: u16,
        /// Error message from server.
        message: String,
    },

    /// Response parsing failed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// All retry attempts exhausted.
    #[error("registration failed after {attempts} attempts: {last_error}")]
    RetriesExhausted {
        /// Number of attempts made.
        attempts: usize,
        /// The last error encountered.
        last_error: String,
    },

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}

/// Result type for registration operations.
pub type Result<T> = std::result::Result<T, RegistrationError>;

/// Configuration for garage registration.
#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    /// Base URL of the moto-club server (e.g., `https://moto-club.example.com`).
    pub moto_club_url: String,

    /// Unique identifier for this garage.
    pub garage_id: String,

    /// Authentication token (K8s service account token).
    pub auth_token: String,

    /// Request timeout. Defaults to 30 seconds.
    pub timeout: Duration,

    /// Whether to retry on failure. Defaults to true.
    pub retry_enabled: bool,
}

impl RegistrationConfig {
    /// Create a new registration config with required fields.
    #[must_use]
    pub const fn new(moto_club_url: String, garage_id: String, auth_token: String) -> Self {
        Self {
            moto_club_url,
            garage_id,
            auth_token,
            timeout: DEFAULT_REGISTRATION_TIMEOUT,
            retry_enabled: true,
        }
    }

    /// Set the request timeout.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Disable automatic retry on failure.
    #[must_use]
    pub const fn without_retry(mut self) -> Self {
        self.retry_enabled = false;
        self
    }

    /// Get the registration endpoint URL.
    #[must_use]
    pub fn registration_url(&self) -> String {
        let base = self.moto_club_url.trim_end_matches('/');
        format!("{base}/api/v1/wg/garages")
    }

    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn validate(&self) -> Result<()> {
        if self.moto_club_url.is_empty() {
            return Err(RegistrationError::Config(
                "moto_club_url is required".to_string(),
            ));
        }

        if self.garage_id.is_empty() {
            return Err(RegistrationError::Config(
                "garage_id is required".to_string(),
            ));
        }

        if self.auth_token.is_empty() {
            return Err(RegistrationError::Config(
                "auth_token is required".to_string(),
            ));
        }

        Ok(())
    }
}

/// Request body for garage registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    /// Garage identifier.
    pub garage_id: String,

    /// Garage's ephemeral `WireGuard` public key.
    pub public_key: WgPublicKey,

    /// Direct UDP endpoints for P2P connections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<SocketAddr>,
}

impl RegistrationRequest {
    /// Create a new registration request.
    #[must_use]
    pub const fn new(garage_id: String, public_key: WgPublicKey) -> Self {
        Self {
            garage_id,
            public_key,
            endpoints: Vec::new(),
        }
    }

    /// Add endpoints to the request.
    #[must_use]
    pub fn with_endpoints(mut self, endpoints: Vec<SocketAddr>) -> Self {
        self.endpoints = endpoints;
        self
    }
}

/// Response from successful garage registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationResponse {
    /// Assigned overlay IP address for this garage.
    pub assigned_ip: OverlayIp,

    /// DERP map for relay fallback.
    pub derp_map: DerpMap,
}

/// Garage registrar for communicating with moto-club.
///
/// Handles the registration of a garage with the moto-club coordination server,
/// including retry logic with exponential backoff.
#[derive(Debug)]
pub struct GarageRegistrar {
    config: RegistrationConfig,
}

impl GarageRegistrar {
    /// Create a new garage registrar.
    #[must_use]
    pub const fn new(config: RegistrationConfig) -> Self {
        Self { config }
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &RegistrationConfig {
        &self.config
    }

    /// Register this garage with moto-club.
    ///
    /// Generates a registration request with the provided private key and endpoints,
    /// sends it to moto-club, and returns the assigned overlay IP and DERP map.
    ///
    /// If retry is enabled (default), will retry up to 3 times with exponential backoff
    /// (1s, 2s, 4s) on transient failures.
    ///
    /// # Arguments
    ///
    /// * `private_key` - The garage's ephemeral `WireGuard` private key
    /// * `endpoints` - Direct UDP endpoints for P2P connections (can be empty)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Configuration is invalid
    /// - moto-club is unreachable
    /// - All retry attempts fail
    /// - Server returns an error response
    pub async fn register(
        &self,
        private_key: &WgPrivateKey,
        endpoints: &[SocketAddr],
    ) -> Result<RegistrationResponse> {
        self.config.validate()?;

        let request =
            RegistrationRequest::new(self.config.garage_id.clone(), private_key.public_key())
                .with_endpoints(endpoints.to_vec());

        if self.config.retry_enabled {
            self.register_with_retry(&request).await
        } else {
            self.register_once(&request).await
        }
    }

    /// Register with automatic retry on transient failures.
    async fn register_with_retry(
        &self,
        request: &RegistrationRequest,
    ) -> Result<RegistrationResponse> {
        for (attempt, backoff) in RETRY_BACKOFF.iter().enumerate() {
            match self.register_once(request).await {
                Ok(response) => return Ok(response),
                Err(RegistrationError::Unreachable(msg)) => {
                    tracing::warn!(
                        attempt = attempt + 1,
                        backoff_secs = backoff.as_secs(),
                        error = %msg,
                        "Registration failed, retrying after backoff"
                    );
                    tokio::time::sleep(*backoff).await;
                }
                Err(e) => {
                    // Non-transient errors should not be retried
                    return Err(e);
                }
            }
        }

        // Final attempt after all backoffs
        match self.register_once(request).await {
            Ok(response) => Ok(response),
            Err(RegistrationError::Unreachable(msg)) => Err(RegistrationError::RetriesExhausted {
                attempts: RETRY_BACKOFF.len() + 1,
                last_error: msg,
            }),
            Err(e) => Err(e),
        }
    }

    /// Perform a single registration attempt.
    ///
    /// This method is a placeholder that will be replaced with actual HTTP
    /// client logic when integrated with the HTTP client crate.
    #[allow(clippy::unused_async)]
    async fn register_once(&self, request: &RegistrationRequest) -> Result<RegistrationResponse> {
        // This is a placeholder implementation.
        // In the actual implementation, this would use an HTTP client to:
        // 1. POST to self.config.registration_url()
        // 2. Include Authorization header with self.config.auth_token
        // 3. Send request body as JSON
        // 4. Parse response as RegistrationResponse
        //
        // For now, we just log and return a placeholder error to indicate
        // the registration infrastructure is in place.
        tracing::debug!(
            garage_id = %request.garage_id,
            public_key = %request.public_key,
            endpoint_count = request.endpoints.len(),
            url = %self.config.registration_url(),
            "Attempting garage registration"
        );

        // Return unreachable error to simulate the real behavior
        // This will be replaced with actual HTTP client calls
        Err(RegistrationError::Unreachable(
            "HTTP client not yet integrated".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_config() -> RegistrationConfig {
        RegistrationConfig::new(
            "https://moto-club.example.com".to_string(),
            "test-garage".to_string(),
            "test-token".to_string(),
        )
    }

    #[test]
    fn config_creation() {
        let config = create_config();

        assert_eq!(config.moto_club_url, "https://moto-club.example.com");
        assert_eq!(config.garage_id, "test-garage");
        assert_eq!(config.auth_token, "test-token");
        assert_eq!(config.timeout, DEFAULT_REGISTRATION_TIMEOUT);
        assert!(config.retry_enabled);
    }

    #[test]
    fn config_builders() {
        let config = RegistrationConfig::new(
            "https://moto-club.example.com".to_string(),
            "test-garage".to_string(),
            "test-token".to_string(),
        )
        .with_timeout(Duration::from_secs(60))
        .without_retry();

        assert_eq!(config.timeout, Duration::from_secs(60));
        assert!(!config.retry_enabled);
    }

    #[test]
    fn config_registration_url() {
        let config = create_config();
        assert_eq!(
            config.registration_url(),
            "https://moto-club.example.com/api/v1/wg/garages"
        );

        // With trailing slash
        let config2 = RegistrationConfig::new(
            "https://moto-club.example.com/".to_string(),
            "test".to_string(),
            "token".to_string(),
        );
        assert_eq!(
            config2.registration_url(),
            "https://moto-club.example.com/api/v1/wg/garages"
        );
    }

    #[test]
    fn config_validation() {
        // Valid config
        let config = create_config();
        assert!(config.validate().is_ok());

        // Empty URL
        let config =
            RegistrationConfig::new(String::new(), "garage".to_string(), "token".to_string());
        assert!(matches!(
            config.validate(),
            Err(RegistrationError::Config(_))
        ));

        // Empty garage_id
        let config = RegistrationConfig::new(
            "https://example.com".to_string(),
            String::new(),
            "token".to_string(),
        );
        assert!(matches!(
            config.validate(),
            Err(RegistrationError::Config(_))
        ));

        // Empty auth_token
        let config = RegistrationConfig::new(
            "https://example.com".to_string(),
            "garage".to_string(),
            String::new(),
        );
        assert!(matches!(
            config.validate(),
            Err(RegistrationError::Config(_))
        ));
    }

    #[test]
    fn registration_request_creation() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();

        let request = RegistrationRequest::new("test-garage".to_string(), public_key.clone());

        assert_eq!(request.garage_id, "test-garage");
        assert_eq!(request.public_key, public_key);
        assert!(request.endpoints.is_empty());
    }

    #[test]
    fn registration_request_with_endpoints() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();

        let request = RegistrationRequest::new("test-garage".to_string(), public_key)
            .with_endpoints(vec![endpoint]);

        assert_eq!(request.endpoints, vec![endpoint]);
    }

    #[test]
    fn registration_request_serde() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();

        let request = RegistrationRequest::new("test-garage".to_string(), public_key)
            .with_endpoints(vec![endpoint]);

        let json = serde_json::to_string(&request).unwrap();
        let parsed: RegistrationRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.garage_id, parsed.garage_id);
        assert_eq!(request.public_key, parsed.public_key);
        assert_eq!(request.endpoints, parsed.endpoints);
    }

    #[test]
    fn registration_request_serde_without_endpoints() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();

        let request = RegistrationRequest::new("test-garage".to_string(), public_key);

        let json = serde_json::to_string(&request).unwrap();

        // endpoints should be omitted
        assert!(!json.contains("endpoints"));

        let parsed: RegistrationRequest = serde_json::from_str(&json).unwrap();
        assert!(parsed.endpoints.is_empty());
    }

    #[test]
    fn registration_response_serde() {
        use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

        let response = RegistrationResponse {
            assigned_ip: OverlayIp::garage(12345),
            derp_map: DerpMap::new().with_region(
                DerpRegion::new(1, "primary")
                    .with_node(DerpNode::with_defaults("derp.example.com")),
            ),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: RegistrationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.assigned_ip, parsed.assigned_ip);
        assert_eq!(response.derp_map.len(), parsed.derp_map.len());
    }

    #[test]
    fn registrar_creation() {
        let config = create_config();
        let registrar = GarageRegistrar::new(config.clone());

        assert_eq!(registrar.config().garage_id, config.garage_id);
    }

    #[tokio::test]
    async fn registrar_validates_config() {
        let config = RegistrationConfig::new(
            String::new(), // Invalid: empty URL
            "garage".to_string(),
            "token".to_string(),
        )
        .without_retry();

        let registrar = GarageRegistrar::new(config);
        let private_key = WgPrivateKey::generate();

        let result = registrar.register(&private_key, &[]).await;
        assert!(matches!(result, Err(RegistrationError::Config(_))));
    }

    #[test]
    fn retry_backoff_values() {
        assert_eq!(RETRY_BACKOFF.len(), 3);
        assert_eq!(RETRY_BACKOFF[0], Duration::from_secs(1));
        assert_eq!(RETRY_BACKOFF[1], Duration::from_secs(2));
        assert_eq!(RETRY_BACKOFF[2], Duration::from_secs(4));
    }

    #[test]
    fn error_display() {
        let err = RegistrationError::Unreachable("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));

        let err = RegistrationError::HttpError {
            status: 401,
            message: "unauthorized".to_string(),
        };
        assert!(err.to_string().contains("401"));
        assert!(err.to_string().contains("unauthorized"));

        let err = RegistrationError::RetriesExhausted {
            attempts: 4,
            last_error: "timeout".to_string(),
        };
        assert!(err.to_string().contains("4 attempts"));
        assert!(err.to_string().contains("timeout"));
    }
}
