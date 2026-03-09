use serde_json::json;

use crate::{AuditEvent, AuditEventBuilder};
use crate::{is_sensitive_key, sanitize_metadata};

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

#[test]
fn sensitive_keys_are_detected() {
    let sensitive = [
        "secret_value",
        "api_key",
        "apiKey",
        "token",
        "bearer_token",
        "authorization",
        "password",
        "request_body",
        "response_body",
        "prompt",
        "completion",
        "content",
        "private_key",
        "key_material",
        "credential",
        "plaintext",
    ];
    for key in sensitive {
        assert!(is_sensitive_key(key), "{key} should be sensitive");
    }
}

#[test]
fn safe_keys_are_not_flagged() {
    let safe = [
        "provider",
        "model",
        "mode",
        "duration_ms",
        "upstream_status",
        "garage_name",
        "branch",
        "ttl_seconds",
        "reason",
        "from",
        "to",
        "owner",
        "previous_status",
        "termination_reason",
    ];
    for key in safe {
        assert!(!is_sensitive_key(key), "{key} should not be sensitive");
    }
}

#[test]
fn sanitize_redacts_sensitive_keys() {
    let metadata = json!({
        "provider": "anthropic",
        "api_key": "sk-ant-12345",
        "model": "claude-sonnet-4-20250514"
    });
    let result = sanitize_metadata(metadata);

    assert_eq!(result["provider"], "anthropic");
    assert_eq!(result["api_key"], "[REDACTED]");
    assert_eq!(result["model"], "claude-sonnet-4-20250514");
}

#[test]
fn sanitize_handles_nested_objects() {
    let metadata = json!({
        "outer": {
            "safe_field": "ok",
            "secret_value": "should-be-redacted"
        }
    });
    let result = sanitize_metadata(metadata);

    assert_eq!(result["outer"]["safe_field"], "ok");
    assert_eq!(result["outer"]["secret_value"], "[REDACTED]");
}

#[test]
fn sanitize_handles_arrays() {
    let metadata = json!({
        "items": [
            {"name": "ok", "token": "abc123"},
            {"name": "also-ok"}
        ]
    });
    let result = sanitize_metadata(metadata);

    assert_eq!(result["items"][0]["name"], "ok");
    assert_eq!(result["items"][0]["token"], "[REDACTED]");
    assert_eq!(result["items"][1]["name"], "also-ok");
}

#[test]
fn sanitize_preserves_non_object_metadata() {
    let metadata = json!("just a string");
    let result = sanitize_metadata(metadata);
    assert_eq!(result, json!("just a string"));
}

#[test]
fn builder_sanitizes_metadata_on_build() {
    let event = AuditEventBuilder::new("test_event", "test-service", "test")
        .metadata(json!({
            "safe": "value",
            "secret_value": "should-be-redacted",
            "api_key": "sk-12345"
        }))
        .build();

    assert_eq!(event.metadata["safe"], "value");
    assert_eq!(event.metadata["secret_value"], "[REDACTED]");
    assert_eq!(event.metadata["api_key"], "[REDACTED]");
}
