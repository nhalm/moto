//! Database models for moto-keybox.
//!
//! These models map directly to the `PostgreSQL` schema defined in the keybox spec.
//! They include `sqlx::FromRow` for database queries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Secret scope in the database.
///
/// Maps to the `scope` TEXT column in the `secrets` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Global platform-wide secret.
    Global,
    /// Service-scoped secret (per-engine/service type).
    Service,
    /// Instance-scoped secret (per-garage or per-bike).
    Instance,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Global => "global",
            Self::Service => "service",
            Self::Instance => "instance",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for Scope {
    type Err = ParseScopeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "global" => Ok(Self::Global),
            "service" => Ok(Self::Service),
            "instance" => Ok(Self::Instance),
            _ => Err(ParseScopeError(s.to_string())),
        }
    }
}

/// Error parsing a scope from a string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid scope: {0}")]
pub struct ParseScopeError(String);

/// Audit event type in the database.
///
/// Maps to the `event_type` TEXT column in the `audit_log` table.
/// Uses unified naming convention from the audit-logging spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
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

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::SecretAccessed => "secret_accessed",
            Self::SecretCreated => "secret_created",
            Self::SecretUpdated => "secret_updated",
            Self::SecretDeleted => "secret_deleted",
            Self::SvidIssued => "svid_issued",
            Self::AuthFailed => "auth_failed",
            Self::AccessDenied => "access_denied",
            Self::DekRotated => "dek_rotated",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for AuditEventType {
    type Err = ParseAuditEventTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "secret_accessed" => Ok(Self::SecretAccessed),
            "secret_created" => Ok(Self::SecretCreated),
            "secret_updated" => Ok(Self::SecretUpdated),
            "secret_deleted" => Ok(Self::SecretDeleted),
            "svid_issued" => Ok(Self::SvidIssued),
            "auth_failed" => Ok(Self::AuthFailed),
            "access_denied" => Ok(Self::AccessDenied),
            "dek_rotated" => Ok(Self::DekRotated),
            _ => Err(ParseAuditEventTypeError(s.to_string())),
        }
    }
}

/// Error parsing an audit event type from a string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid audit event type: {0}")]
pub struct ParseAuditEventTypeError(String);

/// Principal type in the database.
///
/// Maps to the `principal_type` TEXT column in the `audit_log` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PrincipalType {
    /// A garage (development environment).
    Garage,
    /// A bike (deployed service instance).
    Bike,
    /// A platform service.
    Service,
    /// An unauthenticated caller.
    Anonymous,
}

impl std::fmt::Display for PrincipalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Garage => "garage",
            Self::Bike => "bike",
            Self::Service => "service",
            Self::Anonymous => "anonymous",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for PrincipalType {
    type Err = ParsePrincipalTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "garage" => Ok(Self::Garage),
            "bike" => Ok(Self::Bike),
            "service" => Ok(Self::Service),
            "anonymous" => Ok(Self::Anonymous),
            _ => Err(ParsePrincipalTypeError(s.to_string())),
        }
    }
}

/// Error parsing a principal type from a string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid principal type: {0}")]
pub struct ParsePrincipalTypeError(String);

/// A secret record from the database.
///
/// Maps to the `secrets` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct Secret {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// The scope level (global, service, instance).
    pub scope: Scope,
    /// Service name (null for global).
    pub service: Option<String>,
    /// Instance ID (null for global/service).
    pub instance_id: Option<String>,
    /// The secret name/path.
    pub name: String,
    /// Current version number.
    pub current_version: i32,
    /// When the secret was created.
    pub created_at: DateTime<Utc>,
    /// When the secret was last updated.
    pub updated_at: DateTime<Utc>,
    /// When the secret was soft-deleted (if applicable).
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A secret version record from the database.
///
/// Maps to the `secret_versions` table schema.
/// Contains the encrypted secret value for a specific version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct SecretVersion {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// Reference to the secret.
    pub secret_id: Uuid,
    /// Version number.
    pub version: i32,
    /// Encrypted secret value (AES-256-GCM ciphertext).
    pub ciphertext: Vec<u8>,
    /// Nonce used for encryption.
    pub nonce: Vec<u8>,
    /// Reference to the DEK used for encryption.
    pub dek_id: Uuid,
    /// When this version was created.
    pub created_at: DateTime<Utc>,
}

/// An encrypted DEK (Data Encryption Key) record from the database.
///
/// Maps to the `encrypted_deks` table schema.
/// The DEK is encrypted with the master KEK (Key Encryption Key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct EncryptedDek {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// The DEK encrypted with the master KEK.
    pub encrypted_key: Vec<u8>,
    /// Nonce used for KEK encryption.
    pub nonce: Vec<u8>,
    /// When the DEK was created.
    pub created_at: DateTime<Utc>,
}

/// An audit log entry from the database.
///
/// Maps to the unified `audit_log` table schema.
/// Contains access and security events (no secret values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct AuditLogEntry {
    /// Unique identifier (UUID).
    pub id: Uuid,
    /// Event category (e.g. `secret_accessed`, `svid_issued`).
    pub event_type: AuditEventType,
    /// Which service produced the event.
    pub service: String,
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
    /// Service-specific additional context (no sensitive data).
    pub metadata: serde_json::Value,
    /// Source IP from request headers or socket addr.
    pub client_ip: Option<String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_display() {
        assert_eq!(Scope::Global.to_string(), "global");
        assert_eq!(Scope::Service.to_string(), "service");
        assert_eq!(Scope::Instance.to_string(), "instance");
    }

    #[test]
    fn scope_parse() {
        assert_eq!("global".parse::<Scope>().unwrap(), Scope::Global);
        assert_eq!("service".parse::<Scope>().unwrap(), Scope::Service);
        assert_eq!("instance".parse::<Scope>().unwrap(), Scope::Instance);
        assert_eq!("GLOBAL".parse::<Scope>().unwrap(), Scope::Global);
        assert!("invalid".parse::<Scope>().is_err());
    }

    #[test]
    fn scope_serde_roundtrip() {
        for scope in [Scope::Global, Scope::Service, Scope::Instance] {
            let json = serde_json::to_string(&scope).unwrap();
            let parsed: Scope = serde_json::from_str(&json).unwrap();
            assert_eq!(scope, parsed);
        }
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
    fn audit_event_type_parse() {
        assert_eq!(
            "secret_accessed".parse::<AuditEventType>().unwrap(),
            AuditEventType::SecretAccessed
        );
        assert_eq!(
            "svid_issued".parse::<AuditEventType>().unwrap(),
            AuditEventType::SvidIssued
        );
        assert!("invalid".parse::<AuditEventType>().is_err());
    }

    #[test]
    fn principal_type_display() {
        assert_eq!(PrincipalType::Garage.to_string(), "garage");
        assert_eq!(PrincipalType::Bike.to_string(), "bike");
        assert_eq!(PrincipalType::Service.to_string(), "service");
        assert_eq!(PrincipalType::Anonymous.to_string(), "anonymous");
    }

    #[test]
    fn principal_type_parse() {
        assert_eq!(
            "garage".parse::<PrincipalType>().unwrap(),
            PrincipalType::Garage
        );
        assert_eq!(
            "bike".parse::<PrincipalType>().unwrap(),
            PrincipalType::Bike
        );
        assert_eq!(
            "service".parse::<PrincipalType>().unwrap(),
            PrincipalType::Service
        );
        assert_eq!(
            "anonymous".parse::<PrincipalType>().unwrap(),
            PrincipalType::Anonymous
        );
        assert!("invalid".parse::<PrincipalType>().is_err());
    }
}
