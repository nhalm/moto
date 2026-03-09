//! Core types for moto-keybox.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// The scope level of a secret.
///
/// Secrets exist at three levels, checked in order:
/// - Instance: Per-garage or per-bike secrets
/// - Service: Per-engine/service type secrets
/// - Global: Platform-wide secrets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Instance-scoped secret (per-garage or per-bike).
    Instance,
    /// Service-scoped secret (per-engine/service type).
    Service,
    /// Global platform-wide secret.
    Global,
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instance => write!(f, "instance"),
            Self::Service => write!(f, "service"),
            Self::Global => write!(f, "global"),
        }
    }
}

impl std::str::FromStr for Scope {
    type Err = ParseScopeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "instance" => Ok(Self::Instance),
            "service" => Ok(Self::Service),
            "global" => Ok(Self::Global),
            _ => Err(ParseScopeError(s.to_string())),
        }
    }
}

/// Error returned when parsing an invalid scope string.
#[derive(Debug, Clone)]
pub struct ParseScopeError(String);

impl fmt::Display for ParseScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid scope '{}': expected 'instance', 'service', or 'global'",
            self.0
        )
    }
}

impl std::error::Error for ParseScopeError {}

/// The type of principal (entity) requesting access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalType {
    /// A garage (development environment).
    Garage,
    /// A bike (deployed service instance).
    Bike,
    /// A platform service.
    Service,
}

impl fmt::Display for PrincipalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Garage => write!(f, "garage"),
            Self::Bike => write!(f, "bike"),
            Self::Service => write!(f, "service"),
        }
    }
}

/// A SPIFFE ID identifying an entity in the moto platform.
///
/// Format: `spiffe://moto.local/{type}/{id}`
///
/// Examples:
/// - `spiffe://moto.local/garage/abc12345`
/// - `spiffe://moto.local/bike/def67890`
/// - `spiffe://moto.local/service/ai-proxy`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SpiffeId {
    /// The type of principal.
    pub principal_type: PrincipalType,
    /// The identifier (garage-id, bike-id, or service name).
    pub id: String,
}

impl SpiffeId {
    /// The trust domain for moto.
    pub const TRUST_DOMAIN: &'static str = "moto.local";

    /// Creates a new SPIFFE ID for a garage.
    #[must_use]
    pub fn garage(id: impl Into<String>) -> Self {
        Self {
            principal_type: PrincipalType::Garage,
            id: id.into(),
        }
    }

    /// Creates a new SPIFFE ID for a bike.
    #[must_use]
    pub fn bike(id: impl Into<String>) -> Self {
        Self {
            principal_type: PrincipalType::Bike,
            id: id.into(),
        }
    }

    /// Creates a new SPIFFE ID for a service.
    #[must_use]
    pub fn service(name: impl Into<String>) -> Self {
        Self {
            principal_type: PrincipalType::Service,
            id: name.into(),
        }
    }

    /// Returns the URI representation of this SPIFFE ID.
    #[must_use]
    pub fn to_uri(&self) -> String {
        format!(
            "spiffe://{}/{}/{}",
            Self::TRUST_DOMAIN,
            self.principal_type,
            self.id
        )
    }
}

impl fmt::Display for SpiffeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uri())
    }
}

impl std::str::FromStr for SpiffeId {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let prefix = format!("spiffe://{}/", Self::TRUST_DOMAIN);
        let rest = s
            .strip_prefix(&prefix)
            .ok_or_else(|| crate::Error::InvalidSpiffeId { id: s.to_string() })?;

        let (type_str, id) = rest
            .split_once('/')
            .ok_or_else(|| crate::Error::InvalidSpiffeId { id: s.to_string() })?;

        let principal_type = match type_str {
            "garage" => PrincipalType::Garage,
            "bike" => PrincipalType::Bike,
            "service" => PrincipalType::Service,
            _ => return Err(crate::Error::InvalidSpiffeId { id: s.to_string() }),
        };

        Ok(Self {
            principal_type,
            id: id.to_string(),
        })
    }
}

impl TryFrom<String> for SpiffeId {
    type Error = crate::Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<SpiffeId> for String {
    fn from(id: SpiffeId) -> Self {
        id.to_uri()
    }
}

/// Metadata about a secret (without the actual value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMetadata {
    /// Unique identifier.
    pub id: Uuid,
    /// The scope level.
    pub scope: Scope,
    /// Service name (for service-scoped secrets).
    pub service: Option<String>,
    /// Instance ID (for instance-scoped secrets).
    pub instance_id: Option<String>,
    /// The secret name/path.
    pub name: String,
    /// Current version number.
    pub version: u32,
    /// When the secret was created.
    pub created_at: DateTime<Utc>,
    /// When the secret was last updated.
    pub updated_at: DateTime<Utc>,
}

impl SecretMetadata {
    /// Creates new metadata for a global secret.
    #[must_use]
    pub fn global(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            scope: Scope::Global,
            service: None,
            instance_id: None,
            name: name.into(),
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates new metadata for a service-scoped secret.
    #[must_use]
    pub fn service(service: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            scope: Scope::Service,
            service: Some(service.into()),
            instance_id: None,
            name: name.into(),
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates new metadata for an instance-scoped secret.
    #[must_use]
    pub fn instance(instance_id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            scope: Scope::Instance,
            service: None,
            instance_id: Some(instance_id.into()),
            name: name.into(),
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }
}

/// An audit log event type using unified naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// A secret was accessed (read).
    SecretAccessed,
    /// A secret was created.
    SecretCreated,
    /// A secret was updated.
    SecretUpdated,
    /// A secret was deleted.
    SecretDeleted,
    /// An SVID was issued.
    SvidIssued,
    /// Authentication failed.
    AuthFailed,
    /// Access was denied.
    AccessDenied,
    /// A secret's DEK was rotated.
    DekRotated,
}

impl fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecretAccessed => write!(f, "secret_accessed"),
            Self::SecretCreated => write!(f, "secret_created"),
            Self::SecretUpdated => write!(f, "secret_updated"),
            Self::SecretDeleted => write!(f, "secret_deleted"),
            Self::SvidIssued => write!(f, "svid_issued"),
            Self::AuthFailed => write!(f, "auth_failed"),
            Self::AccessDenied => write!(f, "access_denied"),
            Self::DekRotated => write!(f, "dek_rotated"),
        }
    }
}

/// An entry in the audit log (unified schema).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique identifier.
    pub id: Uuid,
    /// Event category.
    pub event_type: AuditEventType,
    /// Principal type: garage, bike, service, or anonymous.
    pub principal_type: PrincipalType,
    /// SPIFFE ID or service name.
    pub principal_id: String,
    /// What happened (create, read, delete, `auth_fail`, etc.).
    pub action: String,
    /// What was acted on (secret, svid, token, etc.).
    pub resource_type: String,
    /// Identifier of the resource.
    pub resource_id: String,
    /// Result: success, denied, or error.
    pub outcome: String,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

impl AuditEntry {
    /// Creates a new audit entry for a secret access event.
    #[must_use]
    pub fn secret_accessed(spiffe_id: &SpiffeId, scope: Scope, name: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type: AuditEventType::SecretAccessed,
            principal_type: spiffe_id.principal_type,
            principal_id: spiffe_id.to_uri(),
            action: "read".to_string(),
            resource_type: "secret".to_string(),
            resource_id: format!("{scope}/{}", name.into()),
            outcome: "success".to_string(),
            timestamp: Utc::now(),
        }
    }

    /// Creates a new audit entry for an SVID issuance event.
    #[must_use]
    pub fn svid_issued(spiffe_id: &SpiffeId) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type: AuditEventType::SvidIssued,
            principal_type: spiffe_id.principal_type,
            principal_id: spiffe_id.to_uri(),
            action: "create".to_string(),
            resource_type: "svid".to_string(),
            resource_id: spiffe_id.to_uri(),
            outcome: "success".to_string(),
            timestamp: Utc::now(),
        }
    }

    /// Creates a new audit entry for an authentication failure.
    #[must_use]
    pub fn auth_failed(reason: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type: AuditEventType::AuthFailed,
            principal_type: PrincipalType::Service,
            principal_id: String::new(),
            action: "auth_fail".to_string(),
            resource_type: "token".to_string(),
            resource_id: reason.into(),
            outcome: "denied".to_string(),
            timestamp: Utc::now(),
        }
    }

    /// Creates a new audit entry for an access denied event.
    #[must_use]
    pub fn access_denied(spiffe_id: &SpiffeId, scope: Scope, name: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type: AuditEventType::AccessDenied,
            principal_type: spiffe_id.principal_type,
            principal_id: spiffe_id.to_uri(),
            action: "auth_fail".to_string(),
            resource_type: "secret".to_string(),
            resource_id: format!("{scope}/{}", name.into()),
            outcome: "denied".to_string(),
            timestamp: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_display() {
        assert_eq!(Scope::Instance.to_string(), "instance");
        assert_eq!(Scope::Service.to_string(), "service");
        assert_eq!(Scope::Global.to_string(), "global");
    }

    #[test]
    fn scope_parse() {
        assert_eq!("instance".parse::<Scope>().unwrap(), Scope::Instance);
        assert_eq!("service".parse::<Scope>().unwrap(), Scope::Service);
        assert_eq!("global".parse::<Scope>().unwrap(), Scope::Global);
        assert_eq!("GLOBAL".parse::<Scope>().unwrap(), Scope::Global);
        assert!("invalid".parse::<Scope>().is_err());
    }

    #[test]
    fn scope_serde_roundtrip() {
        for scope in [Scope::Instance, Scope::Service, Scope::Global] {
            let json = serde_json::to_string(&scope).unwrap();
            let parsed: Scope = serde_json::from_str(&json).unwrap();
            assert_eq!(scope, parsed);
        }
    }

    #[test]
    fn principal_type_display() {
        assert_eq!(PrincipalType::Garage.to_string(), "garage");
        assert_eq!(PrincipalType::Bike.to_string(), "bike");
        assert_eq!(PrincipalType::Service.to_string(), "service");
    }

    #[test]
    fn spiffe_id_garage() {
        let id = SpiffeId::garage("abc123");
        assert_eq!(id.principal_type, PrincipalType::Garage);
        assert_eq!(id.id, "abc123");
        assert_eq!(id.to_uri(), "spiffe://moto.local/garage/abc123");
    }

    #[test]
    fn spiffe_id_bike() {
        let id = SpiffeId::bike("def456");
        assert_eq!(id.principal_type, PrincipalType::Bike);
        assert_eq!(id.id, "def456");
        assert_eq!(id.to_uri(), "spiffe://moto.local/bike/def456");
    }

    #[test]
    fn spiffe_id_service() {
        let id = SpiffeId::service("ai-proxy");
        assert_eq!(id.principal_type, PrincipalType::Service);
        assert_eq!(id.id, "ai-proxy");
        assert_eq!(id.to_uri(), "spiffe://moto.local/service/ai-proxy");
    }

    #[test]
    fn spiffe_id_parse() {
        let id: SpiffeId = "spiffe://moto.local/garage/abc123".parse().unwrap();
        assert_eq!(id.principal_type, PrincipalType::Garage);
        assert_eq!(id.id, "abc123");
    }

    #[test]
    fn spiffe_id_parse_invalid() {
        assert!("invalid".parse::<SpiffeId>().is_err());
        assert!(
            "spiffe://other.domain/garage/abc"
                .parse::<SpiffeId>()
                .is_err()
        );
        assert!(
            "spiffe://moto.local/unknown/abc"
                .parse::<SpiffeId>()
                .is_err()
        );
    }

    #[test]
    fn spiffe_id_serde_roundtrip() {
        let id = SpiffeId::garage("test123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"spiffe://moto.local/garage/test123\"");
        let parsed: SpiffeId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn secret_metadata_global() {
        let meta = SecretMetadata::global("ai/anthropic");
        assert_eq!(meta.scope, Scope::Global);
        assert_eq!(meta.name, "ai/anthropic");
        assert!(meta.service.is_none());
        assert!(meta.instance_id.is_none());
        assert_eq!(meta.version, 1);
    }

    #[test]
    fn secret_metadata_service() {
        let meta = SecretMetadata::service("tokenization", "db/password");
        assert_eq!(meta.scope, Scope::Service);
        assert_eq!(meta.service, Some("tokenization".to_string()));
        assert_eq!(meta.name, "db/password");
        assert!(meta.instance_id.is_none());
    }

    #[test]
    fn secret_metadata_instance() {
        let meta = SecretMetadata::instance("garage-abc", "dev/token");
        assert_eq!(meta.scope, Scope::Instance);
        assert_eq!(meta.instance_id, Some("garage-abc".to_string()));
        assert_eq!(meta.name, "dev/token");
        assert!(meta.service.is_none());
    }

    #[test]
    fn audit_event_type_display() {
        assert_eq!(
            AuditEventType::SecretAccessed.to_string(),
            "secret_accessed"
        );
        assert_eq!(AuditEventType::SecretCreated.to_string(), "secret_created");
        assert_eq!(AuditEventType::SecretUpdated.to_string(), "secret_updated");
        assert_eq!(AuditEventType::SecretDeleted.to_string(), "secret_deleted");
        assert_eq!(AuditEventType::SvidIssued.to_string(), "svid_issued");
        assert_eq!(AuditEventType::AuthFailed.to_string(), "auth_failed");
        assert_eq!(AuditEventType::AccessDenied.to_string(), "access_denied");
        assert_eq!(AuditEventType::DekRotated.to_string(), "dek_rotated");
    }

    #[test]
    fn audit_entry_secret_accessed() {
        let spiffe = SpiffeId::garage("test123");
        let entry = AuditEntry::secret_accessed(&spiffe, Scope::Global, "ai/anthropic");
        assert_eq!(entry.event_type, AuditEventType::SecretAccessed);
        assert_eq!(entry.principal_type, PrincipalType::Garage);
        assert_eq!(entry.principal_id, "spiffe://moto.local/garage/test123");
        assert_eq!(entry.resource_type, "secret");
        assert_eq!(entry.resource_id, "global/ai/anthropic");
        assert_eq!(entry.outcome, "success");
    }

    #[test]
    fn audit_entry_svid_issued() {
        let spiffe = SpiffeId::bike("bike-456");
        let entry = AuditEntry::svid_issued(&spiffe);
        assert_eq!(entry.event_type, AuditEventType::SvidIssued);
        assert_eq!(entry.principal_type, PrincipalType::Bike);
        assert_eq!(entry.principal_id, "spiffe://moto.local/bike/bike-456");
        assert_eq!(entry.resource_type, "svid");
        assert_eq!(entry.action, "create");
    }

    #[test]
    fn audit_entry_auth_failed() {
        let entry = AuditEntry::auth_failed("Invalid token signature");
        assert_eq!(entry.event_type, AuditEventType::AuthFailed);
        assert_eq!(entry.action, "auth_fail");
        assert_eq!(entry.resource_type, "token");
        assert_eq!(entry.resource_id, "Invalid token signature");
        assert_eq!(entry.outcome, "denied");
    }

    #[test]
    fn audit_entry_access_denied() {
        let spiffe = SpiffeId::service("untrusted-service");
        let entry = AuditEntry::access_denied(&spiffe, Scope::Global, "crypto/master-key");
        assert_eq!(entry.event_type, AuditEventType::AccessDenied);
        assert_eq!(entry.principal_type, PrincipalType::Service);
        assert_eq!(
            entry.principal_id,
            "spiffe://moto.local/service/untrusted-service"
        );
        assert_eq!(entry.resource_type, "secret");
        assert_eq!(entry.resource_id, "global/crypto/master-key");
        assert_eq!(entry.outcome, "denied");
    }
}
