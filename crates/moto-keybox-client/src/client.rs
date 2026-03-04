//! HTTP client for fetching secrets from keybox.
//!
//! This module provides the `KeyboxClient` for communicating with the keybox
//! server. It handles:
//!
//! - SVID authentication (automatic token exchange and refresh)
//! - Secret fetching by scope (global, service, instance)
//! - Secure secret handling via `SecretString`
//!
//! # Example
//!
//! ```rust,no_run
//! use moto_keybox_client::{KeyboxClient, KeyboxConfig, Scope};
//!
//! # async fn example() -> moto_keybox_client::Result<()> {
//! // Create client from environment
//! let client = KeyboxClient::from_env().await?;
//!
//! // Fetch a global secret
//! let secret = client.get_secret(Scope::Global, "ai/anthropic").await?;
//!
//! // Use the secret (automatically zeroizes on drop)
//! use secrecy::ExposeSecret;
//! let api_key = secret.expose_secret();
//! # Ok(())
//! # }
//! ```
//!
//! # Local Development
//!
//! For local development without K8s, set environment variables:
//!
//! ```bash
//! export MOTO_KEYBOX_URL=http://localhost:8080
//! export MOTO_KEYBOX_SVID_FILE=./dev-svid.jwt
//! ```

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use reqwest::Client;
use secrecy::SecretString;
use serde::Deserialize;
use tracing::debug;

use moto_keybox::api::{
    GetSecretResponse, ListSecretsResponse, SecretMetadataResponse, TokenRequest, TokenResponse,
};
use moto_keybox::types::{PrincipalType, Scope};

use crate::{Error, Result, SvidCache};

/// Default keybox URL for local development.
pub const DEFAULT_KEYBOX_URL: &str = "http://localhost:8080";

/// Default request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Configuration for the keybox client.
#[derive(Debug, Clone)]
pub struct KeyboxConfig {
    /// Base URL for keybox (e.g., `http://localhost:8080`).
    pub base_url: String,

    /// Request timeout.
    pub timeout: Duration,
}

impl KeyboxConfig {
    /// Create a new keybox client configuration.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
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
    /// - `MOTO_KEYBOX_URL` for base URL (default: `http://localhost:8080`)
    #[must_use]
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("MOTO_KEYBOX_URL").unwrap_or_else(|_| DEFAULT_KEYBOX_URL.to_string());
        Self::new(base_url)
    }
}

impl Default for KeyboxConfig {
    fn default() -> Self {
        Self::new(DEFAULT_KEYBOX_URL)
    }
}

/// Client for fetching secrets from keybox.
///
/// The client handles SVID authentication automatically:
/// - In K8s mode, it exchanges the `ServiceAccount` JWT for an SVID
/// - In local mode, it reads a pre-issued SVID from a file
///
/// SVIDs are cached and automatically refreshed before expiry.
pub struct KeyboxClient {
    /// HTTP client.
    client: Client,

    /// Configuration.
    config: KeyboxConfig,

    /// SVID cache for authentication.
    svid_cache: Arc<SvidCache>,

    /// Principal info for token requests (K8s mode only).
    principal: Option<PrincipalInfo>,
}

/// Principal information for K8s mode token requests.
#[derive(Debug, Clone)]
struct PrincipalInfo {
    /// Principal type (garage, bike, service).
    principal_type: PrincipalType,
    /// Principal ID.
    principal_id: String,
    /// Pod UID (for binding).
    pod_uid: Option<String>,
    /// Service name for bikes (required for service-scoped secret access via ABAC).
    service: Option<String>,
}

impl KeyboxClient {
    /// Create a new keybox client with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(config: KeyboxConfig, svid_cache: SvidCache) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(Error::Request)?;

        Ok(Self {
            client,
            config,
            svid_cache: Arc::new(svid_cache),
            principal: None,
        })
    }

    /// Create a keybox client from environment configuration.
    ///
    /// Uses:
    /// - `MOTO_KEYBOX_URL` for base URL
    /// - `MOTO_KEYBOX_SVID_FILE` for local SVID (if set)
    ///
    /// # Errors
    ///
    /// Returns an error if the SVID file is configured but cannot be loaded.
    pub async fn from_env() -> Result<Self> {
        let config = KeyboxConfig::from_env();
        let svid_cache = SvidCache::from_env().await?;
        Self::new(config, svid_cache)
    }

    /// Create a keybox client for K8s mode with principal info.
    ///
    /// In K8s mode, the client will exchange the `ServiceAccount` JWT for an SVID
    /// by calling the `/auth/token` endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn for_garage(
        config: KeyboxConfig,
        garage_id: impl Into<String>,
        pod_uid: Option<String>,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(Error::Request)?;

        Ok(Self {
            client,
            config,
            svid_cache: Arc::new(SvidCache::new()),
            principal: Some(PrincipalInfo {
                principal_type: PrincipalType::Garage,
                principal_id: garage_id.into(),
                pod_uid,
                service: None,
            }),
        })
    }

    /// Create a keybox client for a bike.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn for_bike(
        config: KeyboxConfig,
        bike_id: impl Into<String>,
        pod_uid: Option<String>,
        service: Option<String>,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(Error::Request)?;

        Ok(Self {
            client,
            config,
            svid_cache: Arc::new(SvidCache::new()),
            principal: Some(PrincipalInfo {
                principal_type: PrincipalType::Bike,
                principal_id: bike_id.into(),
                pod_uid,
                service,
            }),
        })
    }

    /// Create a keybox client for a service.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn for_service(
        config: KeyboxConfig,
        service_name: impl Into<String>,
        pod_uid: Option<String>,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(Error::Request)?;

        Ok(Self {
            client,
            config,
            svid_cache: Arc::new(SvidCache::new()),
            principal: Some(PrincipalInfo {
                principal_type: PrincipalType::Service,
                principal_id: service_name.into(),
                pod_uid,
                service: None,
            }),
        })
    }

    /// Get a secret value.
    ///
    /// # Arguments
    ///
    /// * `scope` - Secret scope (global, service, instance)
    /// * `name` - Secret name. For service/instance scope, use "context/secret-name" format.
    ///
    /// # Returns
    ///
    /// The secret value as a `SecretString` that automatically zeroizes on drop.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - SVID acquisition fails
    /// - Secret is not found
    /// - Access is denied by ABAC policy
    /// - Keybox server is unreachable
    pub async fn get_secret(&self, scope: Scope, name: &str) -> Result<SecretString> {
        let url = format!(
            "{}/secrets/{}/{}",
            self.config.base_url,
            scope_to_str(scope),
            name
        );

        debug!(url = %url, scope = ?scope, name = %name, "fetching secret");

        let response: GetSecretResponse = self.get_json_with_auth(&url).await?;

        // Decode base64 value
        let value_bytes = base64::engine::general_purpose::STANDARD
            .decode(&response.value)
            .map_err(|e| Error::Server {
                code: "INVALID_RESPONSE".to_string(),
                message: format!("invalid base64 in response: {e}"),
            })?;

        let value_str = String::from_utf8(value_bytes).map_err(|e| Error::Server {
            code: "INVALID_RESPONSE".to_string(),
            message: format!("secret is not valid UTF-8: {e}"),
        })?;

        Ok(SecretString::from(value_str))
    }

    /// List secrets in a scope.
    ///
    /// # Arguments
    ///
    /// * `scope` - Secret scope (global, service, instance)
    ///
    /// # Returns
    ///
    /// List of secret metadata (names only, no values).
    ///
    /// # Errors
    ///
    /// Returns an error if SVID acquisition fails or keybox is unreachable.
    pub async fn list_secrets(&self, scope: Scope) -> Result<Vec<SecretMetadataResponse>> {
        let url = format!("{}/secrets/{}", self.config.base_url, scope_to_str(scope));

        debug!(url = %url, scope = ?scope, "listing secrets");

        let response: ListSecretsResponse = self.get_json_with_auth(&url).await?;
        Ok(response.secrets)
    }

    /// List secrets for a specific service.
    ///
    /// # Arguments
    ///
    /// * `service` - Service name
    ///
    /// # Errors
    ///
    /// Returns an error if SVID acquisition fails or keybox is unreachable.
    pub async fn list_service_secrets(&self, service: &str) -> Result<Vec<SecretMetadataResponse>> {
        let url = format!("{}/secrets/service/{}", self.config.base_url, service);

        debug!(url = %url, service = %service, "listing service secrets");

        let response: ListSecretsResponse = self.get_json_with_auth(&url).await?;
        Ok(response.secrets)
    }

    /// List secrets for a specific instance.
    ///
    /// # Arguments
    ///
    /// * `instance_id` - Instance ID (garage-id or bike-id)
    ///
    /// # Errors
    ///
    /// Returns an error if SVID acquisition fails or keybox is unreachable.
    pub async fn list_instance_secrets(
        &self,
        instance_id: &str,
    ) -> Result<Vec<SecretMetadataResponse>> {
        let url = format!("{}/secrets/instance/{}", self.config.base_url, instance_id);

        debug!(url = %url, instance_id = %instance_id, "listing instance secrets");

        let response: ListSecretsResponse = self.get_json_with_auth(&url).await?;
        Ok(response.secrets)
    }

    /// Ensure we have a valid SVID, refreshing if needed.
    ///
    /// In K8s mode, this will call `/auth/token` to get an SVID.
    /// In local mode, the SVID is read from file.
    async fn ensure_svid(&self) -> Result<String> {
        // Check if we need to refresh
        if self.svid_cache.needs_refresh().await {
            // In K8s mode, request a new SVID
            if let Some(ref principal) = self.principal {
                debug!(
                    principal_type = ?principal.principal_type,
                    principal_id = %principal.principal_id,
                    "requesting new SVID"
                );

                let token = self.request_svid(principal).await?;
                self.svid_cache.set(token).await?;
            }
            // In local mode, svid_cache.get() will reload from file
        }

        self.svid_cache.get().await
    }

    /// Request an SVID from the keybox server.
    async fn request_svid(&self, principal: &PrincipalInfo) -> Result<String> {
        let url = format!("{}/auth/token", self.config.base_url);

        let request = TokenRequest {
            principal_type: principal.principal_type,
            principal_id: principal.principal_id.clone(),
            pod_uid: principal.pod_uid.clone(),
            service: principal.service.clone(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            let token_response: TokenResponse =
                response.json().await.map_err(|e| Error::Server {
                    code: "INVALID_RESPONSE".to_string(),
                    message: format!("failed to parse token response: {e}"),
                })?;
            debug!(expires_at = token_response.expires_at, "received SVID");
            Ok(token_response.token)
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Send a GET request with SVID authentication.
    async fn get_json_with_auth<Resp>(&self, url: &str) -> Result<Resp>
    where
        Resp: for<'de> Deserialize<'de>,
    {
        let svid = self.ensure_svid().await?;

        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {svid}"))
            .send()
            .await
            .map_err(|e| self.connection_error(e))?;

        if response.status().is_success() {
            response.json().await.map_err(|e| Error::Server {
                code: "INVALID_RESPONSE".to_string(),
                message: format!("failed to parse response: {e}"),
            })
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Convert a reqwest error to a connection error if appropriate.
    fn connection_error(&self, error: reqwest::Error) -> Error {
        if error.is_connect() || error.is_timeout() {
            Error::Unreachable {
                url: self.config.base_url.clone(),
                reason: error.to_string(),
            }
        } else {
            Error::Request(error)
        }
    }

    /// Handle an error response from the server.
    async fn handle_error_response<T>(&self, response: reqwest::Response) -> Result<T> {
        let status = response.status();

        // Try to parse error response
        let error: ApiErrorResponse = match response.json().await {
            Ok(e) => e,
            Err(_) => {
                return Err(Error::Server {
                    code: "UNKNOWN".to_string(),
                    message: format!("HTTP {status}"),
                });
            }
        };

        let code = error.error.code;
        let message = error.error.message;

        // Map known error codes to specific errors
        // Note: server returns ACCESS_DENIED for both "not found" and "access denied"
        // to prevent secret enumeration (spec v0.4)
        match code.as_str() {
            "ACCESS_DENIED" => Err(Error::AccessDenied { message }),
            "SVID_EXPIRED" => Err(Error::SvidExpired),
            "UNAUTHORIZED" | "INVALID_SVID" => Err(Error::NoSvid { message }),
            _ => Err(Error::Server { code, message }),
        }
    }
}

/// API error response format (matches keybox API).
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

/// Convert scope enum to URL path segment.
const fn scope_to_str(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => "global",
        Scope::Service => "service",
        Scope::Instance => "instance",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybox_config_defaults() {
        let config = KeyboxConfig::new("http://localhost:8080");
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn keybox_config_with_timeout() {
        let config =
            KeyboxConfig::new("http://localhost:8080").with_timeout(Duration::from_secs(60));
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn keybox_config_default() {
        let config = KeyboxConfig::default();
        assert_eq!(config.base_url, DEFAULT_KEYBOX_URL);
    }

    #[test]
    fn scope_to_str_global() {
        assert_eq!(scope_to_str(Scope::Global), "global");
    }

    #[test]
    fn scope_to_str_service() {
        assert_eq!(scope_to_str(Scope::Service), "service");
    }

    #[test]
    fn scope_to_str_instance() {
        assert_eq!(scope_to_str(Scope::Instance), "instance");
    }

    #[test]
    fn api_error_deserialize() {
        // Server returns ACCESS_DENIED for both "not found" and "access denied" (spec v0.4)
        let json = r#"{"error":{"code":"ACCESS_DENIED","message":"Access denied"}}"#;
        let error: ApiErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(error.error.code, "ACCESS_DENIED");
        assert_eq!(error.error.message, "Access denied");
    }

    #[tokio::test]
    async fn client_new_with_cache() {
        let config = KeyboxConfig::new("http://localhost:8080");
        let cache = SvidCache::new();
        let client = KeyboxClient::new(config, cache);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn client_for_garage() {
        let config = KeyboxConfig::new("http://localhost:8080");
        let client = KeyboxClient::for_garage(config, "test-garage", None);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn client_for_bike() {
        let config = KeyboxConfig::new("http://localhost:8080");
        let client = KeyboxClient::for_bike(
            config,
            "test-bike",
            Some("pod-123".to_string()),
            Some("my-service".to_string()),
        );
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn client_for_service() {
        let config = KeyboxConfig::new("http://localhost:8080");
        let client = KeyboxClient::for_service(config, "moto-club", None);
        assert!(client.is_ok());
    }
}
