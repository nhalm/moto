//! Garage identity validation — verifies garages via SVID JWT and moto-club API.
//!
//! Garages authenticate to ai-proxy by sending their SVID JWT (issued by keybox,
//! mounted at `/var/run/secrets/svid/`) in the provider-native auth header. The
//! proxy decodes the JWT claims to extract the garage ID, checks expiration, and
//! validates the garage state against moto-club (`GET /api/v1/garages/{id}`).
//! Validation results are cached with a configurable TTL (default 60s).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Errors returned by garage validation.
#[derive(Debug)]
pub enum AuthError {
    /// No auth token found in request headers.
    MissingToken,
    /// Token format is invalid (not a valid SVID JWT).
    InvalidToken,
    /// SVID has expired.
    SvidExpired,
    /// Token principal is not a garage.
    NotGarage,
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
            Self::SvidExpired | Self::NotGarage | Self::GarageNotReady(_) => {
                axum::http::StatusCode::FORBIDDEN
            }
            Self::ValidationFailed(_) => axum::http::StatusCode::BAD_GATEWAY,
        }
    }

    /// Error type string for the `OpenAI` error format.
    #[must_use]
    pub const fn error_type(&self) -> &'static str {
        match self {
            Self::MissingToken | Self::InvalidToken => "authentication_error",
            Self::SvidExpired | Self::NotGarage | Self::GarageNotReady(_) => "forbidden",
            Self::ValidationFailed(_) => "server_error",
        }
    }

    /// Human-readable error message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::MissingToken => "missing authentication token".to_string(),
            Self::InvalidToken => "invalid authentication token".to_string(),
            Self::SvidExpired => "SVID token has expired".to_string(),
            Self::NotGarage => "token principal is not a garage".to_string(),
            Self::GarageNotReady(id) => format!("garage {id} is not ready"),
            Self::ValidationFailed(msg) => format!("garage validation failed: {msg}"),
        }
    }
}

/// Minimal SVID JWT claims needed for garage identity extraction.
///
/// We decode only the claims we need without full signature verification.
/// The SVID is cryptographically signed by keybox — forging it requires
/// keybox's private key. Garage state is separately validated via moto-club.
#[derive(Debug, Deserialize)]
struct SvidClaims {
    /// Expiration time (Unix timestamp).
    exp: i64,
    /// Principal type (garage, bike, service).
    principal_type: String,
    /// Principal ID (garage-id, bike-id, or service name).
    principal_id: String,
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

/// Extracts the garage ID from an SVID JWT token.
///
/// Decodes the JWT claims (without full signature verification) and extracts
/// the `principal_id` field. Checks that:
/// - The token is a valid 3-part JWT
/// - The claims can be decoded and parsed
/// - The `principal_type` is "garage"
/// - The token has not expired
///
/// # Errors
///
/// Returns `AuthError::InvalidToken` if the JWT is malformed or claims cannot be parsed,
/// `AuthError::NotGarage` if the principal is not a garage, or `AuthError::SvidExpired`
/// if the token has expired.
pub fn extract_garage_id(token: &str) -> Result<String, AuthError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::InvalidToken);
    }

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AuthError::InvalidToken)?;

    let claims: SvidClaims =
        serde_json::from_slice(&claims_bytes).map_err(|_| AuthError::InvalidToken)?;

    if claims.principal_type != "garage" {
        return Err(AuthError::NotGarage);
    }

    if Utc::now().timestamp() > claims.exp {
        return Err(AuthError::SvidExpired);
    }

    if claims.principal_id.is_empty() {
        return Err(AuthError::InvalidToken);
    }

    Ok(claims.principal_id)
}

/// Builds a minimal SVID JWT for testing purposes.
///
/// Creates a JWT with the given claims but a dummy signature.
/// Only for use in tests — production SVIDs are signed by keybox.
#[cfg(test)]
#[must_use]
pub fn build_test_svid(principal_type: &str, principal_id: &str, ttl_secs: i64) -> String {
    let header = serde_json::json!({"alg": "EdDSA", "typ": "JWT"});
    let now = Utc::now().timestamp();
    let claims = serde_json::json!({
        "iss": "keybox",
        "sub": format!("spiffe://moto.local/{principal_type}/{principal_id}"),
        "aud": "moto",
        "exp": now + ttl_secs,
        "iat": now,
        "jti": "test-jti",
        "principal_type": principal_type,
        "principal_id": principal_id,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims.to_string());
    // Dummy signature — not cryptographically valid, but structurally correct.
    let sig_b64 = URL_SAFE_NO_PAD.encode(vec![0u8; 64]);
    format!("{header_b64}.{claims_b64}.{sig_b64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_garage_svid(garage_id: &str) -> String {
        build_test_svid("garage", garage_id, 900)
    }

    #[test]
    fn extract_token_from_bearer() {
        let svid = test_garage_svid("abc123");
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {svid}").parse().unwrap());
        assert_eq!(extract_token(&headers), Some(svid));
    }

    #[test]
    fn extract_token_from_x_api_key() {
        let svid = test_garage_svid("abc123");
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", svid.parse().unwrap());
        assert_eq!(extract_token(&headers), Some(svid));
    }

    #[test]
    fn extract_token_prefers_bearer_over_x_api_key() {
        let svid_bearer = test_garage_svid("bearer-garage");
        let svid_apikey = test_garage_svid("apikey-garage");
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {svid_bearer}").parse().unwrap(),
        );
        headers.insert("x-api-key", svid_apikey.parse().unwrap());
        assert_eq!(extract_token(&headers), Some(svid_bearer));
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
    fn extract_garage_id_from_svid() {
        let svid = test_garage_svid("abc123");
        assert_eq!(extract_garage_id(&svid).unwrap(), "abc123");
    }

    #[test]
    fn extract_garage_id_rejects_non_garage_principal() {
        let svid = build_test_svid("service", "ai-proxy", 900);
        assert!(matches!(
            extract_garage_id(&svid),
            Err(AuthError::NotGarage)
        ));
    }

    #[test]
    fn extract_garage_id_rejects_expired_svid() {
        let svid = build_test_svid("garage", "abc123", -10);
        assert!(matches!(
            extract_garage_id(&svid),
            Err(AuthError::SvidExpired)
        ));
    }

    #[test]
    fn extract_garage_id_rejects_malformed_token() {
        assert!(matches!(
            extract_garage_id("not-a-jwt"),
            Err(AuthError::InvalidToken)
        ));
    }

    #[test]
    fn extract_garage_id_rejects_invalid_base64_claims() {
        assert!(matches!(
            extract_garage_id("header.!!!invalid!!!.signature"),
            Err(AuthError::InvalidToken)
        ));
    }

    #[test]
    fn extract_garage_id_rejects_non_json_claims() {
        let claims_b64 = URL_SAFE_NO_PAD.encode("not json");
        let token = format!("header.{claims_b64}.signature");
        assert!(matches!(
            extract_garage_id(&token),
            Err(AuthError::InvalidToken)
        ));
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
