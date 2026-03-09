//! Tower middleware layer for rate limiting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use tower::{Layer, Service};

use crate::config::{PrincipalType, ThrottleConfig};
use crate::token_bucket::{CheckResult, TokenBucket};

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

            // Extract principal from request (basic: Unknown with IP fallback).
            // Full extraction (JWT, service token) is implemented separately.
            let principal = extract_principal(&request);

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
/// This is the basic fallback: returns Unknown with the client IP as key.
/// Full JWT and service token extraction is added in the principal extraction work item.
fn extract_principal(request: &Request<Body>) -> Principal {
    // Try X-Forwarded-For first, then fall back to "unknown".
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    Principal {
        principal_type: PrincipalType::Unknown,
        key: ip,
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
