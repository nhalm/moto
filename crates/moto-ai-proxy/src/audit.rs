//! Structured audit logging for ai-proxy.
//!
//! Since ai-proxy is stateless (no database), audit events are emitted as
//! newline-delimited JSON to stdout. Each line is a complete audit event
//! matching the unified schema from `moto-audit-types`.

use std::time::Instant;

use axum::http::HeaderMap;
use moto_audit_types::AuditEventBuilder;
use uuid::Uuid;

/// Extracts the client IP from request headers.
///
/// Checks `X-Forwarded-For` (first IP) and `X-Real-Ip` headers.
fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(xff) = headers.get("x-forwarded-for")
        && let Ok(value) = xff.to_str()
    {
        // X-Forwarded-For may contain multiple IPs; take the first.
        return value.split(',').next().map(|ip| ip.trim().to_string());
    }
    if let Some(real_ip) = headers.get("x-real-ip")
        && let Ok(value) = real_ip.to_str()
    {
        return Some(value.to_string());
    }
    None
}

/// Emits an audit event as a JSON line to stdout.
///
/// This is best-effort: serialization failures are logged as warnings
/// but never block the primary operation.
fn emit(event: &moto_audit_types::AuditEvent) {
    match serde_json::to_string(&event) {
        Ok(json) => println!("{json}"),
        Err(e) => tracing::warn!(error = %e, "failed to serialize audit event"),
    }
}

/// Emits an `ai_request` audit event for a successfully proxied request.
#[allow(clippy::too_many_arguments)]
pub fn log_ai_request(
    request_id: &Uuid,
    garage_id: &str,
    provider: &str,
    model: Option<&str>,
    mode: &str,
    upstream_status: Option<u16>,
    start: &Instant,
    headers: &HeaderMap,
) {
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metadata = serde_json::json!({
        "provider": provider,
        "mode": mode,
        "duration_ms": duration_ms,
    });
    if let Some(m) = model {
        metadata["model"] = serde_json::Value::String(m.to_string());
    }
    if let Some(status) = upstream_status {
        metadata["upstream_status"] = serde_json::json!(status);
    }

    let mut builder = AuditEventBuilder::new("ai_request", "ai-proxy", "proxy")
        .principal("garage", format!("spiffe://moto.local/garage/{garage_id}"))
        .resource("ai_request", request_id.to_string())
        .outcome("success")
        .metadata(metadata);

    if let Some(ip) = extract_client_ip(headers) {
        builder = builder.client_ip(ip);
    }

    emit(&builder.build());
}

/// Emits an `ai_request_denied` audit event when a request is blocked.
pub fn log_ai_request_denied(
    request_id: &Uuid,
    garage_id: &str,
    reason: &str,
    provider: Option<&str>,
    mode: &str,
    start: &Instant,
    headers: &HeaderMap,
) {
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metadata = serde_json::json!({
        "reason": reason,
        "mode": mode,
        "duration_ms": duration_ms,
    });
    if let Some(p) = provider {
        metadata["provider"] = serde_json::Value::String(p.to_string());
    }

    let principal_id = if garage_id.is_empty() {
        String::new()
    } else {
        format!("spiffe://moto.local/garage/{garage_id}")
    };

    let principal_type = if garage_id.is_empty() {
        "anonymous"
    } else {
        "garage"
    };

    let mut builder = AuditEventBuilder::new("ai_request_denied", "ai-proxy", "deny")
        .principal(principal_type, principal_id)
        .resource("ai_request", request_id.to_string())
        .outcome("denied")
        .metadata(metadata);

    if let Some(ip) = extract_client_ip(headers) {
        builder = builder.client_ip(ip);
    }

    emit(&builder.build());
}

/// Emits a `provider_error` audit event when the upstream provider returns an error.
#[allow(clippy::too_many_arguments)]
pub fn log_provider_error(
    request_id: &Uuid,
    garage_id: &str,
    provider: &str,
    model: Option<&str>,
    mode: &str,
    upstream_status: u16,
    start: &Instant,
    headers: &HeaderMap,
) {
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metadata = serde_json::json!({
        "provider": provider,
        "mode": mode,
        "upstream_status": upstream_status,
        "duration_ms": duration_ms,
    });
    if let Some(m) = model {
        metadata["model"] = serde_json::Value::String(m.to_string());
    }

    let mut builder = AuditEventBuilder::new("provider_error", "ai-proxy", "error")
        .principal("garage", format!("spiffe://moto.local/garage/{garage_id}"))
        .resource("ai_request", request_id.to_string())
        .outcome("error")
        .metadata(metadata);

    if let Some(ip) = extract_client_ip(headers) {
        builder = builder.client_ip(ip);
    }

    emit(&builder.build());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_client_ip_from_xff() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.1, 10.0.0.2".parse().unwrap());
        assert_eq!(extract_client_ip(&headers), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn extract_client_ip_from_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "10.0.0.5".parse().unwrap());
        assert_eq!(extract_client_ip(&headers), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn extract_client_ip_prefers_xff() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.1".parse().unwrap());
        headers.insert("x-real-ip", "10.0.0.5".parse().unwrap());
        assert_eq!(extract_client_ip(&headers), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn extract_client_ip_returns_none_without_headers() {
        let headers = HeaderMap::new();
        assert_eq!(extract_client_ip(&headers), None);
    }
}
