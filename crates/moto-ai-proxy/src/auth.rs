//! Garage identity validation — verifies garages via moto-club API.
//!
//! Garages authenticate to ai-proxy by sending a token in the provider-native
//! auth header. The proxy extracts the garage ID from the token and validates
//! it against moto-club (`GET /api/v1/garages/{id}`), checking that the garage
//! exists and is in `Ready` state. Validation results are cached with a
//! configurable TTL (default 60s).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Errors returned by garage validation.
#[derive(Debug)]
pub enum AuthError {
    /// No auth token found in request headers.
    MissingToken,
    /// Token format is invalid (cannot extract garage ID).
    InvalidToken,
    /// Garage not found or not in `Ready` state.
    GarageNotReady(String),
    /// Failed to reach moto-club for validation.
    ValidationFailed(String),
}

impl AuthError {
    /// HTTP status code for this error.
    #[must_use]
    pub const fn status_code(&self) -> axum::http::StatusCode {
        match self {
            Self::MissingToken | Self::InvalidToken => axum::http::StatusCode::UNAUTHORIZED,
            Self::GarageNotReady(_) => axum::http::StatusCode::FORBIDDEN,
            Self::ValidationFailed(_) => axum::http::StatusCode::BAD_GATEWAY,
        }
    }

    /// Error type string for the `OpenAI` error format.
    #[must_use]
    pub const fn error_type(&self) -> &'static str {
        match self {
            Self::MissingToken | Self::InvalidToken => "authentication_error",
            Self::GarageNotReady(_) => "forbidden",
            Self::ValidationFailed(_) => "server_error",
        }
    }

    /// Human-readable error message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::MissingToken => "missing authentication token".to_string(),
            Self::InvalidToken => "invalid authentication token".to_string(),
            Self::GarageNotReady(id) => format!("garage {id} is not ready"),
            Self::ValidationFailed(msg) => format!("garage validation failed: {msg}"),
        }
    }
}

/// Trait for validating garage identity.
///
/// Abstracted behind a trait so tests can inject a mock validator
/// without requiring a real `moto-club` instance.
pub trait GarageValidator: Send + Sync {
    /// Validates that a garage exists and is in `Ready` state.
    fn validate_garage(
        &self,
        garage_id: &str,
    ) -> impl std::future::Future<Output = Result<(), AuthError>> + Send;
}

/// Garage validator backed by moto-club HTTP API, with caching.
pub struct ClubGarageValidator {
    /// HTTP client for moto-club requests.
    client: reqwest::Client,
    /// Base URL for moto-club (e.g., `http://moto-club.moto-system:8080`).
    club_url: String,
    /// Cached validation results per garage ID.
    cache: Arc<RwLock<HashMap<String, CachedValidation>>>,
    /// Cache TTL.
    ttl: Duration,
}

struct CachedValidation {
    valid: bool,
    fetched_at: Instant,
}

impl ClubGarageValidator {
    /// Creates a new validator with the given moto-club URL and cache TTL.
    #[must_use]
    pub fn new(client: reqwest::Client, club_url: String, ttl: Duration) -> Self {
        Self {
            client,
            club_url,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }
}

impl GarageValidator for ClubGarageValidator {
    async fn validate_garage(&self, garage_id: &str) -> Result<(), AuthError> {
        // Check cache first.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(garage_id)
                && entry.fetched_at.elapsed() < self.ttl
            {
                return if entry.valid {
                    Ok(())
                } else {
                    Err(AuthError::GarageNotReady(garage_id.to_string()))
                };
            }
        }

        // Cache miss or expired — call moto-club.
        let url = format!("{}/api/v1/garages/{garage_id}", self.club_url);
        debug!(garage_id, url = %url, "validating garage via moto-club");

        let resp = self.client.get(&url).send().await.map_err(|e| {
            warn!(garage_id, error = %e, "moto-club request failed");
            AuthError::ValidationFailed(e.to_string())
        })?;

        let status = resp.status();
        let valid = if status.is_success() {
            // Parse response to check garage status.
            let body: serde_json::Value = resp.json().await.map_err(|e| {
                AuthError::ValidationFailed(format!("invalid response from moto-club: {e}"))
            })?;
            body.get("status")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s == "ready")
        } else if status.as_u16() == 404 {
            false
        } else {
            return Err(AuthError::ValidationFailed(format!(
                "moto-club returned {status}"
            )));
        };

        // Update cache.
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                garage_id.to_string(),
                CachedValidation {
                    valid,
                    fetched_at: Instant::now(),
                },
            );
        }

        if valid {
            Ok(())
        } else {
            Err(AuthError::GarageNotReady(garage_id.to_string()))
        }
    }
}

/// Extracts the garage token from request headers.
///
/// Looks for the token in provider-native auth headers:
/// - `Authorization: Bearer {token}` (`OpenAI`, Gemini, unified)
/// - `x-api-key: {token}` (Anthropic passthrough)
#[must_use]
pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    // Try Authorization: Bearer first.
    if let Some(auth) = headers.get("authorization")
        && let Ok(value) = auth.to_str()
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        return Some(token.to_string());
    }
    // Try x-api-key (Anthropic-style).
    if let Some(key) = headers.get("x-api-key")
        && let Ok(value) = key.to_str()
    {
        return Some(value.to_string());
    }
    None
}

/// Extracts the garage ID from a token.
///
/// For v0.2, the token format is `garage-{id}` where `{id}` is the garage UUID.
#[must_use]
pub fn extract_garage_id(token: &str) -> Option<String> {
    token.strip_prefix("garage-").map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_from_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer garage-abc123".parse().unwrap());
        assert_eq!(extract_token(&headers), Some("garage-abc123".to_string()));
    }

    #[test]
    fn extract_token_from_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "garage-abc123".parse().unwrap());
        assert_eq!(extract_token(&headers), Some("garage-abc123".to_string()));
    }

    #[test]
    fn extract_token_prefers_bearer_over_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer garage-bearer".parse().unwrap());
        headers.insert("x-api-key", "garage-apikey".parse().unwrap());
        assert_eq!(extract_token(&headers), Some("garage-bearer".to_string()));
    }

    #[test]
    fn extract_token_returns_none_without_auth() {
        let headers = HeaderMap::new();
        assert_eq!(extract_token(&headers), None);
    }

    #[test]
    fn extract_token_ignores_non_bearer_auth() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic abc123".parse().unwrap());
        assert_eq!(extract_token(&headers), None);
    }

    #[test]
    fn extract_garage_id_from_token() {
        assert_eq!(
            extract_garage_id("garage-abc123"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extract_garage_id_invalid_format() {
        assert_eq!(extract_garage_id("not-a-garage-token"), None);
    }

    #[test]
    fn extract_garage_id_empty_prefix() {
        assert_eq!(extract_garage_id("garage-"), Some(String::new()));
    }

    struct MockGarageValidator {
        valid_garages: Vec<String>,
    }

    impl MockGarageValidator {
        fn new(valid_garages: Vec<&str>) -> Self {
            Self {
                valid_garages: valid_garages.into_iter().map(String::from).collect(),
            }
        }
    }

    impl GarageValidator for MockGarageValidator {
        async fn validate_garage(&self, garage_id: &str) -> Result<(), AuthError> {
            if self.valid_garages.contains(&garage_id.to_string()) {
                Ok(())
            } else {
                Err(AuthError::GarageNotReady(garage_id.to_string()))
            }
        }
    }

    #[tokio::test]
    async fn mock_validator_accepts_valid_garage() {
        let validator = MockGarageValidator::new(vec!["abc123"]);
        assert!(validator.validate_garage("abc123").await.is_ok());
    }

    #[tokio::test]
    async fn mock_validator_rejects_unknown_garage() {
        let validator = MockGarageValidator::new(vec!["abc123"]);
        assert!(validator.validate_garage("unknown").await.is_err());
    }
}
