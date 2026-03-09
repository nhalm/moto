//! Tower middleware layer for rate limiting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use tower::{Layer, Service};

use crate::config::{PrincipalType, ThrottleConfig, env_u64};
use crate::token_bucket::{CheckResult, TokenBucket};

/// Default bucket TTL: evict buckets not accessed within this duration.
const DEFAULT_BUCKET_TTL: Duration = Duration::from_secs(600);

/// Default cleanup interval: how often to sweep for expired buckets.
const DEFAULT_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

/// Extracted principal identity used as the rate limit key.
#[derive(Debug, Clone)]
pub struct Principal {
    /// The type of principal (determines which tier applies).
    pub principal_type: PrincipalType,
    /// Unique key for this principal's token bucket.
    pub key: String,
}

/// Shared bucket store: maps principal keys to their token buckets.
type BucketStore = Arc<Mutex<HashMap<String, TokenBucket>>>;

/// Tower layer that adds rate limiting to a service.
///
/// Wraps an inner service with [`ThrottleService`] which checks a per-principal
/// token bucket before forwarding requests.
#[derive(Clone)]
pub struct ThrottleLayer {
    config: Arc<ThrottleConfig>,
    buckets: BucketStore,
}

impl ThrottleLayer {
    /// Create a new throttle layer with the given configuration.
    #[must_use]
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config: Arc::new(config),
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a reference to the shared bucket store (for cleanup tasks).
    #[must_use]
    pub fn bucket_store(&self) -> Arc<Mutex<HashMap<String, TokenBucket>>> {
        Arc::clone(&self.buckets)
    }

    /// Spawn a background task that periodically evicts idle buckets.
    ///
    /// Buckets not accessed within `ttl` are removed. The sweep runs every
    /// `interval`. Reads `MOTO_THROTTLE_BUCKET_TTL_SECS` and
    /// `MOTO_THROTTLE_CLEANUP_INTERVAL_SECS` env vars, falling back to
    /// defaults of 10 min TTL and 60 sec interval.
    #[must_use]
    pub fn spawn_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let ttl = env_u64("MOTO_THROTTLE_BUCKET_TTL_SECS")
            .map_or(DEFAULT_BUCKET_TTL, Duration::from_secs);
        let interval = env_u64("MOTO_THROTTLE_CLEANUP_INTERVAL_SECS")
            .map_or(DEFAULT_CLEANUP_INTERVAL, Duration::from_secs);
        self.spawn_cleanup_with(ttl, interval)
    }

    /// Spawn a cleanup task with custom TTL and sweep interval.
    #[must_use]
    pub fn spawn_cleanup_with(
        &self,
        ttl: Duration,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let buckets = Arc::clone(&self.buckets);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                evict_expired_buckets(&buckets, ttl);
            }
        })
    }
}

impl<S> Layer<S> for ThrottleLayer {
    type Service = ThrottleService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ThrottleService {
            inner,
            config: Arc::clone(&self.config),
            buckets: Arc::clone(&self.buckets),
        }
    }
}

/// Tower service that enforces rate limits on incoming requests.
#[derive(Clone)]
pub struct ThrottleService<S> {
    inner: S,
    config: Arc<ThrottleConfig>,
    buckets: BucketStore,
}

impl<S> Service<Request<Body>> for ThrottleService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let config = Arc::clone(&self.config);
        let buckets = Arc::clone(&self.buckets);

        // See https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move {
            let path = request.uri().path().to_string();

            // Extract principal from request (JWT, service token, or IP fallback).
            let principal = extract_principal(&request, config.service_token());

            // Look up effective tier config for this principal + path.
            let Some(tier) = config.lookup(principal.principal_type, &path) else {
                // No rate limiting for this path/principal combination.
                return inner.call(request).await;
            };

            // Check the token bucket.
            let (result, remaining, reset_at, rpm_limit) = {
                let mut store = buckets.lock().expect("bucket store lock poisoned");
                let bucket = store
                    .entry(principal.key.clone())
                    .or_insert_with(|| TokenBucket::new(tier.burst, tier.rpm));
                let result = bucket.check();
                let remaining = bucket.remaining();
                let reset_at = bucket.reset_at();
                let rpm_limit = bucket.rpm_limit();
                drop(store);
                (result, remaining, reset_at, rpm_limit)
            };

            match result {
                CheckResult::Allowed { .. } => {
                    let mut response = inner.call(request).await?;
                    inject_rate_limit_headers(
                        response.headers_mut(),
                        rpm_limit,
                        remaining,
                        reset_at,
                    );
                    Ok(response)
                }
                CheckResult::Denied { retry_after_secs } => {
                    tracing::warn!(
                        principal_id = %principal.key,
                        principal_type = ?principal.principal_type,
                        path = %path,
                        rpm_limit = rpm_limit,
                        retry_after_secs = retry_after_secs,
                        "rate limited"
                    );

                    let body = serde_json::json!({
                        "error": {
                            "message": "rate limit exceeded",
                            "type": "rate_limit_error"
                        }
                    });

                    let mut response = Response::builder()
                        .status(429)
                        .header("Content-Type", "application/json")
                        .header("Retry-After", retry_after_secs.to_string())
                        .body(Body::from(body.to_string()))
                        .expect("valid response");

                    inject_rate_limit_headers(response.headers_mut(), rpm_limit, 0, reset_at);

                    Ok(response)
                }
            }
        })
    }
}

/// Extract principal from a request.
///
/// Extraction order per spec:
/// 1. `Authorization: Bearer {token}` — attempt JWT parse for `principal_type` + `principal_id`
/// 2. `x-api-key: {token}` — same JWT parse attempt
/// 3. If token is not a valid JWT, check if it matches the service token
/// 4. Fallback: Unknown tier with client IP as key
pub fn extract_principal(request: &Request<Body>, service_token: Option<&str>) -> Principal {
    // Try Authorization header first, then x-api-key.
    let token = extract_bearer_token(request).or_else(|| extract_api_key(request));

    if let Some(token) = token {
        // Try JWT parse first.
        if let Some(principal) = parse_jwt_principal(token) {
            return principal;
        }

        // Not a valid JWT — check if it's a service token.
        if let Some(svc_token) = service_token
            && token == svc_token
        {
            return Principal {
                principal_type: PrincipalType::Service,
                key: "service-token".to_string(),
            };
        }
    }

    // Fallback: Unknown with client IP.
    let ip = client_ip(request);
    Principal {
        principal_type: PrincipalType::Unknown,
        key: ip,
    }
}

/// Extract bearer token from the Authorization header.
fn extract_bearer_token(request: &Request<Body>) -> Option<&str> {
    request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

/// Extract token from the x-api-key header.
fn extract_api_key(request: &Request<Body>) -> Option<&str> {
    request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
}

/// Attempt to parse a JWT token (without signature validation) and extract
/// `principal_type` and `principal_id` claims.
fn parse_jwt_principal(token: &str) -> Option<Principal> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() < 2 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    let principal_type_str = claims.get("principal_type")?.as_str()?;
    let principal_id = claims.get("principal_id")?.as_str()?;

    let principal_type = match principal_type_str {
        "garage" => PrincipalType::Garage,
        "bike" => PrincipalType::Bike,
        "service" => PrincipalType::Service,
        _ => return None,
    };

    Some(Principal {
        principal_type,
        key: principal_id.to_string(),
    })
}

/// Extract client IP from X-Forwarded-For header or fall back to "unknown".
fn client_ip(request: &Request<Body>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string())
}

/// Evict buckets that haven't been accessed within the given TTL.
pub fn evict_expired_buckets(buckets: &Mutex<HashMap<String, TokenBucket>>, ttl: Duration) {
    let mut store = buckets.lock().expect("bucket store lock poisoned");
    let before = store.len();
    store.retain(|_, bucket| bucket.last_access().elapsed() < ttl);
    let evicted = before - store.len();
    if evicted > 0 {
        tracing::debug!(evicted, remaining = store.len(), "bucket cleanup sweep");
    }
}

/// Inject rate limit headers into a response.
fn inject_rate_limit_headers(
    headers: &mut axum::http::HeaderMap,
    rpm_limit: u32,
    remaining: u64,
    reset_at: u64,
) {
    headers.insert("X-RateLimit-Limit", rpm_limit.into());
    headers.insert(
        "X-RateLimit-Remaining",
        remaining.to_string().parse().expect("valid header value"),
    );
    headers.insert(
        "X-RateLimit-Reset",
        reset_at.to_string().parse().expect("valid header value"),
    );
}
