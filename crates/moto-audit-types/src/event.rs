//! Audit event schema shared across all moto services.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single audit event matching the unified schema.
///
/// All moto services (keybox, moto-club, ai-proxy) produce events with this
/// structure. Services store events in their own database tables or emit them
/// as structured log lines (ai-proxy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID (UUID v7, time-ordered).
    pub id: Uuid,
    /// Event category (e.g. `secret_accessed`, `garage_created`, `ai_request`).
    pub event_type: String,
    /// Which service produced the event (`keybox`, `moto-club`, `ai-proxy`).
    pub service: String,
    /// Principal type: `garage`, `bike`, `service`, or `anonymous`.
    pub principal_type: String,
    /// SPIFFE ID or service name.
    pub principal_id: String,
    /// What happened (`create`, `read`, `delete`, `auth_fail`, `proxy`, etc.).
    pub action: String,
    /// What was acted on (`secret`, `garage`, `ai_request`, `svid`, `token`, etc.).
    pub resource_type: String,
    /// Identifier of the resource.
    pub resource_id: String,
    /// Result of the action: `success`, `denied`, or `error`.
    pub outcome: String,
    /// Service-specific additional context. Must never contain sensitive data.
    pub metadata: serde_json::Value,
    /// Source IP from request headers or socket addr, if available.
    pub client_ip: Option<String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

/// Builder for constructing [`AuditEvent`] instances.
///
/// Provides a fluent API for creating audit events with sensible defaults:
/// - `id` is auto-generated as UUID v7
/// - `timestamp` defaults to now
/// - `metadata` defaults to empty object
/// - `client_ip` defaults to `None`
pub struct AuditEventBuilder {
    event_type: String,
    service: String,
    principal_type: String,
    principal_id: String,
    action: String,
    resource_type: String,
    resource_id: String,
    outcome: String,
    metadata: serde_json::Value,
    client_ip: Option<String>,
}

impl AuditEventBuilder {
    /// Start building an audit event.
    #[must_use]
    pub fn new(
        event_type: impl Into<String>,
        service: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            service: service.into(),
            principal_type: "anonymous".to_string(),
            principal_id: String::new(),
            action: action.into(),
            resource_type: String::new(),
            resource_id: String::new(),
            outcome: "success".to_string(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            client_ip: None,
        }
    }

    /// Set the principal (who performed the action).
    #[must_use]
    pub fn principal(
        mut self,
        principal_type: impl Into<String>,
        principal_id: impl Into<String>,
    ) -> Self {
        self.principal_type = principal_type.into();
        self.principal_id = principal_id.into();
        self
    }

    /// Set the resource (what was acted on).
    #[must_use]
    pub fn resource(
        mut self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        self.resource_type = resource_type.into();
        self.resource_id = resource_id.into();
        self
    }

    /// Set the outcome (`success`, `denied`, or `error`).
    #[must_use]
    pub fn outcome(mut self, outcome: impl Into<String>) -> Self {
        self.outcome = outcome.into();
        self
    }

    /// Set service-specific metadata. Must not contain sensitive data.
    #[must_use]
    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set the client IP address.
    #[must_use]
    pub fn client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Build the audit event with auto-generated ID and current timestamp.
    ///
    /// Metadata is sanitized before inclusion — any keys matching known sensitive
    /// patterns (secrets, tokens, API keys, request/response bodies) are redacted.
    #[must_use]
    pub fn build(self) -> AuditEvent {
        AuditEvent {
            id: Uuid::now_v7(),
            event_type: self.event_type,
            service: self.service,
            principal_type: self.principal_type,
            principal_id: self.principal_id,
            action: self.action,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            outcome: self.outcome,
            metadata: sanitize_metadata(self.metadata),
            client_ip: self.client_ip,
            timestamp: Utc::now(),
        }
    }
}

/// Keys that must never appear in audit metadata because they may contain
/// sensitive data (secret values, API keys, tokens, request/response bodies,
/// passwords, or credential material).
const SENSITIVE_KEY_PATTERNS: &[&str] = &[
    "secret",
    "password",
    "passwd",
    "credential",
    "api_key",
    "apikey",
    "token",
    "bearer",
    "authorization",
    "body",
    "request_body",
    "response_body",
    "prompt",
    "completion",
    "content",
    "plaintext",
    "private_key",
    "key_material",
];

/// Sanitize metadata by redacting values for keys that match sensitive patterns.
///
/// Operates recursively on nested objects. Array elements that are objects are
/// also sanitized. Non-object metadata is returned as-is.
pub fn sanitize_metadata(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sanitized = map
                .into_iter()
                .map(|(key, val)| {
                    if is_sensitive_key(&key) {
                        (key, serde_json::Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, sanitize_metadata(val))
                    }
                })
                .collect();
            serde_json::Value::Object(sanitized)
        }
        serde_json::Value::Array(arr) => {
            let sanitized = arr.into_iter().map(sanitize_metadata).collect();
            serde_json::Value::Array(sanitized)
        }
        other => other,
    }
}

/// Check if a key name matches any sensitive pattern (case-insensitive).
#[must_use]
pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_KEY_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}
