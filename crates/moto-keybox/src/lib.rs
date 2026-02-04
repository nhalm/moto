//! Secrets manager for the moto platform.
//!
//! This crate provides:
//! - SPIFFE-inspired identity for authentication
//! - ABAC (Attribute-Based Access Control) for authorization
//! - Envelope encryption for secrets at rest
//! - Audit logging for all access events
//!
//! # Architecture
//!
//! Keybox is an internal service. It is not publicly exposed. All user-facing
//! secret management goes through moto-club, which handles user authentication
//! and proxies requests to keybox. Garages and bikes authenticate directly to
//! keybox via SVID (they're inside the cluster).
//!
//! # Secret Scoping
//!
//! Secrets exist at three levels, checked in order:
//! - **Instance**: Per-garage or per-bike secrets
//! - **Service**: Per-engine/service type secrets
//! - **Global**: Platform-wide secrets (e.g., AI API keys)
//!
//! # SPIFFE ID Format
//!
//! ```text
//! spiffe://moto.local/garage/{garage-id}
//! spiffe://moto.local/bike/{bike-id}
//! spiffe://moto.local/service/{service-name}
//! ```
//!
//! # Example
//!
//! ```
//! use moto_keybox::{SpiffeId, Scope, SecretMetadata, PolicyEngine, Action};
//!
//! // Create a SPIFFE ID for a garage
//! let id = SpiffeId::garage("my-garage-id");
//! assert_eq!(id.to_uri(), "spiffe://moto.local/garage/my-garage-id");
//!
//! // Create secret metadata
//! let meta = SecretMetadata::global("ai/anthropic");
//! assert_eq!(meta.scope, Scope::Global);
//! ```

pub mod abac;
pub mod api;
pub mod envelope;
mod error;
pub mod health;
pub mod repository;
pub mod svid;
pub mod types;

pub use abac::{AccessRequest, Action, PolicyEngine};
pub use api::{AppState, GARAGE_SVID_TTL_SECS, router};
pub use envelope::{DataEncryptionKey, EncryptedDek, EncryptedSecret, MasterKey};
pub use error::{Error, Result};
pub use health::{health_router, is_startup_complete, mark_startup_complete};
pub use svid::{DEFAULT_SVID_TTL_SECS, SvidClaims, SvidIssuer, SvidValidator};
pub use types::{AuditEntry, AuditEventType, PrincipalType, Scope, SecretMetadata, SpiffeId};
