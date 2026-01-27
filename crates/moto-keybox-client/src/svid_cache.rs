//! SVID caching with automatic refresh.
//!
//! This module provides thread-safe SVID caching that:
//! - Caches the current SVID token
//! - Tracks expiration time
//! - Supports refresh before expiry (at 14 minutes for 15-minute TTL)
//! - Supports both K8s mode (fetch via API) and local mode (read from file)

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use moto_keybox::SvidClaims;

use crate::{Error, Result};

/// Default refresh buffer in seconds (1 minute before expiry).
const REFRESH_BUFFER_SECS: i64 = 60;

/// Cached SVID with metadata.
#[derive(Debug, Clone)]
struct CachedSvid {
    /// The JWT token string.
    token: String,
    /// Parsed claims from the token.
    claims: SvidClaims,
    /// When the token expires.
    expires_at: DateTime<Utc>,
}

impl CachedSvid {
    /// Creates a new cached SVID from a token and claims.
    fn new(token: String, claims: SvidClaims) -> Self {
        let expires_at = claims.expires_at().unwrap_or_else(Utc::now);
        Self {
            token,
            claims,
            expires_at,
        }
    }

    /// Returns true if the SVID needs refresh (within buffer period of expiry).
    fn needs_refresh(&self) -> bool {
        let refresh_at = self.expires_at - Duration::seconds(REFRESH_BUFFER_SECS);
        Utc::now() >= refresh_at
    }

    /// Returns true if the SVID has fully expired.
    fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}

/// SVID cache that handles both K8s and local development modes.
///
/// In K8s mode, the cache will fetch SVIDs by exchanging K8s `ServiceAccount` JWTs.
/// In local mode, the cache reads a pre-issued dev SVID from a file.
///
/// # Example
///
/// ```rust,no_run
/// use moto_keybox_client::SvidCache;
///
/// # async fn example() -> moto_keybox_client::Result<()> {
/// // Local development mode - read SVID from file
/// let cache = SvidCache::from_file("./dev-svid.jwt").await?;
///
/// // Get the current SVID (refreshes automatically if needed)
/// let token = cache.get().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SvidCache {
    /// The cached SVID (if any).
    cached: Arc<RwLock<Option<CachedSvid>>>,
    /// SVID validator for parsing tokens.
    validator: Option<moto_keybox::SvidValidator>,
    /// Path to SVID file (for local mode).
    svid_file: Option<PathBuf>,
}

impl SvidCache {
    /// Creates a new empty SVID cache.
    ///
    /// Use this when you'll acquire the SVID via API (K8s mode).
    /// Call `set` to cache an SVID after fetching it.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cached: Arc::new(RwLock::new(None)),
            validator: None,
            svid_file: None,
        }
    }

    /// Creates an SVID cache with a validator for parsing tokens.
    ///
    /// The validator is used to extract claims from SVID tokens.
    #[must_use]
    pub fn with_validator(validator: moto_keybox::SvidValidator) -> Self {
        Self {
            cached: Arc::new(RwLock::new(None)),
            validator: Some(validator),
            svid_file: None,
        }
    }

    /// Creates an SVID cache that reads from a file (local development mode).
    ///
    /// This is used when `MOTO_KEYBOX_SVID_FILE` is set, allowing local
    /// development without K8s.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or contains an invalid SVID.
    pub async fn from_file(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let token = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| Error::SvidLoad {
                message: format!("failed to read {}: {e}", path.display()),
            })?;
        let token = token.trim().to_string();

        // Parse claims without validation (we don't have the signing key)
        let claims = Self::parse_claims_unverified(&token)?;

        let cached = CachedSvid::new(token, claims);

        if cached.is_expired() {
            warn!(path = %path.display(), "loaded SVID is expired");
        }

        debug!(
            path = %path.display(),
            expires_at = %cached.expires_at,
            "loaded SVID from file"
        );

        Ok(Self {
            cached: Arc::new(RwLock::new(Some(cached))),
            validator: None,
            svid_file: Some(path),
        })
    }

    /// Creates an SVID cache from environment configuration.
    ///
    /// Checks `MOTO_KEYBOX_SVID_FILE` for local development mode.
    /// If not set, creates an empty cache for K8s mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the SVID file is configured but cannot be loaded.
    pub async fn from_env() -> Result<Self> {
        if let Ok(path) = std::env::var("MOTO_KEYBOX_SVID_FILE") {
            Self::from_file(path).await
        } else {
            Ok(Self::new())
        }
    }

    /// Gets the current SVID token.
    ///
    /// In file mode, this will reload the file if the SVID needs refresh.
    /// In API mode, callers should check `needs_refresh` and call `set`
    /// with a new token.
    ///
    /// # Errors
    ///
    /// Returns an error if no SVID is cached or it has expired.
    pub async fn get(&self) -> Result<String> {
        // Try to refresh from file if needed
        if let Some(ref path) = self.svid_file {
            let needs_refresh = {
                let cached = self.cached.read().await;
                cached.as_ref().is_some_and(CachedSvid::needs_refresh)
            };

            if needs_refresh {
                debug!(path = %path.display(), "refreshing SVID from file");
                if let Err(e) = self.reload_from_file().await {
                    warn!(error = %e, "failed to refresh SVID from file");
                }
            }
        }

        let cached = self.cached.read().await;
        match cached.as_ref() {
            Some(svid) if svid.is_expired() => Err(Error::SvidExpired),
            Some(svid) => Ok(svid.token.clone()),
            None => Err(Error::NoSvid {
                message: "no SVID cached".to_string(),
            }),
        }
    }

    /// Sets a new SVID in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be parsed.
    pub async fn set(&self, token: String) -> Result<()> {
        let claims = if let Some(ref validator) = self.validator {
            validator
                .verify_signature(&token)
                .map_err(|e| Error::SvidLoad {
                    message: format!("invalid SVID: {e}"),
                })?
        } else {
            Self::parse_claims_unverified(&token)?
        };

        let cached = CachedSvid::new(token, claims);
        debug!(expires_at = %cached.expires_at, "cached new SVID");

        *self.cached.write().await = Some(cached);
        Ok(())
    }

    /// Returns true if the cached SVID needs refresh.
    ///
    /// Returns true if:
    /// - No SVID is cached
    /// - The SVID is within the refresh buffer period
    /// - The SVID has expired
    pub async fn needs_refresh(&self) -> bool {
        let cached = self.cached.read().await;
        cached.as_ref().is_none_or(CachedSvid::needs_refresh)
    }

    /// Returns the cached SVID claims (if any).
    pub async fn claims(&self) -> Option<SvidClaims> {
        let cached = self.cached.read().await;
        cached.as_ref().map(|c| c.claims.clone())
    }

    /// Returns the expiration time of the cached SVID (if any).
    pub async fn expires_at(&self) -> Option<DateTime<Utc>> {
        let cached = self.cached.read().await;
        cached.as_ref().map(|c| c.expires_at)
    }

    /// Clears the cached SVID.
    pub async fn clear(&self) {
        *self.cached.write().await = None;
    }

    /// Reloads the SVID from the configured file.
    async fn reload_from_file(&self) -> Result<()> {
        let path = self.svid_file.as_ref().ok_or_else(|| Error::Config {
            message: "no SVID file configured".to_string(),
        })?;

        let token = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::SvidLoad {
                message: format!("failed to read {}: {e}", path.display()),
            })?;
        let token = token.trim().to_string();

        let claims = Self::parse_claims_unverified(&token)?;
        let cached = CachedSvid::new(token, claims);

        debug!(
            path = %path.display(),
            expires_at = %cached.expires_at,
            "reloaded SVID from file"
        );

        *self.cached.write().await = Some(cached);
        Ok(())
    }

    /// Parses claims from a JWT without verifying the signature.
    ///
    /// Used when we don't have the signing key (e.g., client-side).
    fn parse_claims_unverified(token: &str) -> Result<SvidClaims> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::SvidLoad {
                message: "invalid JWT format".to_string(),
            });
        }

        let claims_b64 = parts[1];
        let claims_json = URL_SAFE_NO_PAD
            .decode(claims_b64)
            .map_err(|e| Error::SvidLoad {
                message: format!("invalid base64 in claims: {e}"),
            })?;

        serde_json::from_slice(&claims_json).map_err(|e| Error::SvidLoad {
            message: format!("invalid claims JSON: {e}"),
        })
    }
}

impl Default for SvidCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_keybox::{SpiffeId, SvidIssuer};

    fn test_issuer() -> SvidIssuer {
        let key = SvidIssuer::generate_key();
        SvidIssuer::new(key)
    }

    #[tokio::test]
    async fn cache_new_empty() {
        let cache = SvidCache::new();
        assert!(cache.needs_refresh().await);

        let result = cache.get().await;
        assert!(matches!(result, Err(Error::NoSvid { .. })));
    }

    #[tokio::test]
    async fn cache_set_and_get() {
        let issuer = test_issuer();
        let validator = moto_keybox::SvidValidator::new(issuer.verifying_key());
        let cache = SvidCache::with_validator(validator);

        let spiffe_id = SpiffeId::garage("test-garage");
        let token = issuer.issue(&spiffe_id).unwrap();

        cache.set(token.clone()).await.unwrap();

        let cached_token = cache.get().await.unwrap();
        assert_eq!(cached_token, token);
        assert!(!cache.needs_refresh().await);
    }

    #[tokio::test]
    async fn cache_claims() {
        let issuer = test_issuer();
        let cache = SvidCache::new();

        let spiffe_id = SpiffeId::bike("test-bike");
        let token = issuer.issue(&spiffe_id).unwrap();

        cache.set(token).await.unwrap();

        let claims = cache.claims().await.unwrap();
        assert_eq!(claims.principal_id, "test-bike");
        assert_eq!(
            claims.principal_type,
            moto_keybox::types::PrincipalType::Bike
        );
    }

    #[tokio::test]
    async fn cache_expires_at() {
        let issuer = test_issuer();
        let cache = SvidCache::new();

        assert!(cache.expires_at().await.is_none());

        let spiffe_id = SpiffeId::service("test-service");
        let token = issuer.issue(&spiffe_id).unwrap();
        cache.set(token).await.unwrap();

        let expires = cache.expires_at().await.unwrap();
        assert!(expires > Utc::now());
    }

    #[tokio::test]
    async fn cache_clear() {
        let issuer = test_issuer();
        let cache = SvidCache::new();

        let spiffe_id = SpiffeId::garage("test");
        let token = issuer.issue(&spiffe_id).unwrap();
        cache.set(token).await.unwrap();

        assert!(cache.claims().await.is_some());

        cache.clear().await;

        assert!(cache.claims().await.is_none());
        assert!(cache.needs_refresh().await);
    }

    #[tokio::test]
    async fn cache_expired_token() {
        let key = SvidIssuer::generate_key();
        let issuer = SvidIssuer::new(key).with_ttl(-1); // Expired immediately
        let cache = SvidCache::new();

        let spiffe_id = SpiffeId::garage("expired");
        let token = issuer.issue(&spiffe_id).unwrap();
        cache.set(token).await.unwrap();

        // Should return expired error
        let result = cache.get().await;
        assert!(matches!(result, Err(Error::SvidExpired)));
    }

    #[tokio::test]
    async fn cache_needs_refresh_near_expiry() {
        let key = SvidIssuer::generate_key();
        // 30 seconds TTL - less than refresh buffer (60s)
        let issuer = SvidIssuer::new(key).with_ttl(30);
        let cache = SvidCache::new();

        let spiffe_id = SpiffeId::garage("refresh-test");
        let token = issuer.issue(&spiffe_id).unwrap();
        cache.set(token).await.unwrap();

        // Should need refresh because we're within the buffer period
        assert!(cache.needs_refresh().await);
    }

    #[test]
    fn parse_claims_unverified() {
        let issuer = test_issuer();
        let spiffe_id = SpiffeId::garage("parse-test");
        let token = issuer.issue(&spiffe_id).unwrap();

        let claims = SvidCache::parse_claims_unverified(&token).unwrap();
        assert_eq!(claims.principal_id, "parse-test");
        assert_eq!(claims.sub, "spiffe://moto.local/garage/parse-test");
    }

    #[test]
    fn parse_claims_invalid_format() {
        let result = SvidCache::parse_claims_unverified("not.valid");
        assert!(matches!(result, Err(Error::SvidLoad { .. })));

        let result = SvidCache::parse_claims_unverified("notajwt");
        assert!(matches!(result, Err(Error::SvidLoad { .. })));
    }
}
