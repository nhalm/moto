use serde_json::json;

use crate::{AuditEvent, AuditEventBuilder};

#[test]
fn builder_creates_event_with_defaults() {
    let event = AuditEventBuilder::new("secret_accessed", "keybox", "read")
        .principal("garage", "spiffe://moto.local/garage/abc123")
        .resource("secret", "global/ai/anthropic")
        .build();

    assert_eq!(event.event_type, "secret_accessed");
    assert_eq!(event.service, "keybox");
    assert_eq!(event.action, "read");
    assert_eq!(event.principal_type, "garage");
    assert_eq!(event.principal_id, "spiffe://moto.local/garage/abc123");
    assert_eq!(event.resource_type, "secret");
    assert_eq!(event.resource_id, "global/ai/anthropic");
    assert_eq!(event.outcome, "success");
    assert_eq!(event.metadata, json!({}));
    assert!(event.client_ip.is_none());
    assert!(!event.id.is_nil());
}

#[test]
fn builder_with_all_fields() {
    let event = AuditEventBuilder::new("auth_failed", "moto-club", "auth_fail")
        .principal("anonymous", "")
        .resource("request", "/api/v1/garages")
        .outcome("denied")
        .metadata(json!({"reason": "invalid token"}))
        .client_ip("10.42.0.15")
        .build();

    assert_eq!(event.outcome, "denied");
    assert_eq!(event.metadata["reason"], "invalid token");
    assert_eq!(event.client_ip.as_deref(), Some("10.42.0.15"));
}

#[test]
fn serde_roundtrip() {
    let event = AuditEventBuilder::new("garage_created", "moto-club", "create")
        .principal("service", "moto-club")
        .resource("garage", "019595a0-1234-7000-8000-000000000001")
        .metadata(json!({"ttl_hours": 24}))
        .client_ip("10.0.0.1")
        .build();

    let json = serde_json::to_string(&event).unwrap();
    let parsed: AuditEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, event.id);
    assert_eq!(parsed.event_type, event.event_type);
    assert_eq!(parsed.service, event.service);
    assert_eq!(parsed.principal_type, event.principal_type);
    assert_eq!(parsed.principal_id, event.principal_id);
    assert_eq!(parsed.action, event.action);
    assert_eq!(parsed.resource_type, event.resource_type);
    assert_eq!(parsed.resource_id, event.resource_id);
    assert_eq!(parsed.outcome, event.outcome);
    assert_eq!(parsed.metadata, event.metadata);
    assert_eq!(parsed.client_ip, event.client_ip);
    assert_eq!(parsed.timestamp, event.timestamp);
}

#[test]
fn json_output_matches_spec_format() {
    let event = AuditEventBuilder::new("ai_request", "ai-proxy", "proxy")
        .principal("garage", "spiffe://moto.local/garage/abc123")
        .resource("ai_request", "req-uuid")
        .metadata(json!({
            "provider": "anthropic",
            "model": "claude-sonnet-4-20250514",
            "mode": "passthrough",
            "upstream_status": 200,
            "duration_ms": 1523
        }))
        .client_ip("10.42.0.15")
        .build();

    let json_val: serde_json::Value = serde_json::to_value(&event).unwrap();

    // All spec-required fields are present.
    assert!(json_val.get("id").is_some());
    assert!(json_val.get("event_type").is_some());
    assert!(json_val.get("service").is_some());
    assert!(json_val.get("principal_type").is_some());
    assert!(json_val.get("principal_id").is_some());
    assert!(json_val.get("action").is_some());
    assert!(json_val.get("resource_type").is_some());
    assert!(json_val.get("resource_id").is_some());
    assert!(json_val.get("outcome").is_some());
    assert!(json_val.get("metadata").is_some());
    assert!(json_val.get("client_ip").is_some());
    assert!(json_val.get("timestamp").is_some());
}
