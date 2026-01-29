//! HTTP request metrics middleware.
//!
//! Provides Prometheus-compatible metrics for HTTP requests:
//! - `http_requests_total{method, path, status}` - Total request count
//! - `http_request_duration_seconds{method, path, status}` - Request latency histogram
//!
//! Per moto-bike.md Engine Contract section.

use std::time::Instant;

use axum::{
    body::Body,
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use metrics::{counter, histogram};

/// Records HTTP request metrics.
///
/// This middleware extracts method, matched path, and response status
/// to record both request counts and duration histograms.
pub async fn record_http_metrics(request: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().to_string();

    // Get the matched path pattern (e.g., "/api/v1/garages/{name}")
    // This is preferred over the full URI to avoid high cardinality from path params
    let path = request.extensions().get::<MatchedPath>().map_or_else(
        || request.uri().path().to_string(),
        |p| p.as_str().to_string(),
    );

    let response = next.run(request).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    // Record metrics per moto-bike.md spec:
    // - http_requests_total{method="GET",path="/api/...",status="200"}
    // - http_request_duration_seconds{...}
    counter!(
        "http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status.clone()
    )
    .increment(1);

    histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status
    )
    .record(duration);

    response
}
