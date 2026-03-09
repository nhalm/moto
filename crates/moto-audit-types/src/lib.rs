//! Shared audit event schema for moto services.
//!
//! Defines the canonical audit event structure used across all moto services
//! (keybox, moto-club, ai-proxy). Each service stores audit events in its own
//! database or logs, but the schema is shared.

mod event;

#[cfg(test)]
mod event_test;

pub use event::{AuditEvent, AuditEventBuilder, is_sensitive_key, sanitize_metadata};
