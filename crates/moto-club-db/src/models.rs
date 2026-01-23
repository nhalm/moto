//! Database models for moto-club.
//!
//! These models map directly to the `PostgreSQL` schema defined in the moto-club spec.
//! They include `sqlx::FromRow` for database queries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Garage status in the database.
///
/// Maps to the `status` TEXT column in the `garages` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum GarageStatus {
    /// Pod scheduled, pulling images.
    Pending,
    /// Container started, initializing.
    Running,
    /// Garage ready for use.
    Ready,
    /// User connected via `WireGuard` tunnel.
    Attached,
    /// Closed/cleaned up.
    Terminated,
}

impl std::fmt::Display for GarageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Attached => "attached",
            Self::Terminated => "terminated",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for GarageStatus {
    type Err = ParseGarageStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "attached" => Ok(Self::Attached),
            "terminated" => Ok(Self::Terminated),
            _ => Err(ParseGarageStatusError(s.to_string())),
        }
    }
}

/// Error parsing a garage status from a string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid garage status: {0}")]
pub struct ParseGarageStatusError(String);

/// Reason a garage was terminated.
///
/// Maps to the `termination_reason` TEXT column in the `garages` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    /// User explicitly closed the garage.
    UserClosed,
    /// TTL expired.
    TtlExpired,
    /// Pod was lost unexpectedly.
    PodLost,
    /// Namespace was missing.
    NamespaceMissing,
    /// An error occurred.
    #[sqlx(rename = "error")]
    #[serde(rename = "error")]
    ErrorReason,
}

impl std::fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::UserClosed => "user_closed",
            Self::TtlExpired => "ttl_expired",
            Self::PodLost => "pod_lost",
            Self::NamespaceMissing => "namespace_missing",
            Self::ErrorReason => "error",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for TerminationReason {
    type Err = ParseTerminationReasonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_closed" => Ok(Self::UserClosed),
            "ttl_expired" => Ok(Self::TtlExpired),
            "pod_lost" => Ok(Self::PodLost),
            "namespace_missing" => Ok(Self::NamespaceMissing),
            "error" => Ok(Self::ErrorReason),
            _ => Err(ParseTerminationReasonError(s.to_string())),
        }
    }
}

/// Error parsing a termination reason from a string.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid termination reason: {0}")]
pub struct ParseTerminationReasonError(String);

/// A garage record from the database.
///
/// Maps to the `garages` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct Garage {
    /// Unique identifier (UUID v7).
    pub id: Uuid,
    /// Human-friendly name (unique, immutable).
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Git branch being worked on.
    pub branch: String,
    /// Current status.
    pub status: GarageStatus,
    /// Time-to-live in seconds.
    pub ttl_seconds: i32,
    /// When the garage expires.
    pub expires_at: DateTime<Utc>,
    /// Kubernetes namespace name.
    pub namespace: String,
    /// Kubernetes pod name.
    pub pod_name: String,
    /// When the garage was created.
    pub created_at: DateTime<Utc>,
    /// When the garage was last updated.
    pub updated_at: DateTime<Utc>,
    /// When the garage was terminated (if applicable).
    pub terminated_at: Option<DateTime<Utc>>,
    /// Why the garage was terminated (if applicable).
    pub termination_reason: Option<TerminationReason>,
}

/// A `WireGuard` device (client device) from the database.
///
/// Maps to the `wg_devices` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct WgDevice {
    /// Unique identifier.
    pub id: Uuid,
    /// Owner identifier.
    pub owner: String,
    /// Optional friendly name for the device.
    pub device_name: Option<String>,
    /// `WireGuard` public key.
    pub public_key: String,
    /// Assigned overlay IP address.
    pub assigned_ip: String,
    /// When the device was registered.
    pub created_at: DateTime<Utc>,
}

/// A `WireGuard` session (active tunnel) from the database.
///
/// Maps to the `wg_sessions` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct WgSession {
    /// Unique identifier.
    pub id: Uuid,
    /// Device this session belongs to.
    pub device_id: Uuid,
    /// Garage this session connects to.
    pub garage_id: Uuid,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// When the session was closed (if applicable).
    pub closed_at: Option<DateTime<Utc>>,
}

/// A user SSH key from the database.
///
/// Maps to the `user_ssh_keys` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct UserSshKey {
    /// Unique identifier.
    pub id: Uuid,
    /// Owner identifier.
    pub owner: String,
    /// SSH public key.
    pub public_key: String,
    /// Key fingerprint.
    pub fingerprint: String,
    /// When the key was registered.
    pub created_at: DateTime<Utc>,
}

/// A DERP server from the database.
///
/// Maps to the `derp_servers` table schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct DerpServer {
    /// Unique identifier.
    pub id: Uuid,
    /// Region ID.
    pub region_id: i32,
    /// Region name.
    pub region_name: String,
    /// Server hostname.
    pub host: String,
    /// DERP port (default 443).
    pub port: i32,
    /// STUN port (default 3478).
    pub stun_port: i32,
    /// Whether the server is healthy.
    pub healthy: bool,
    /// When health was last checked.
    pub last_check_at: Option<DateTime<Utc>>,
    /// When the server was registered.
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garage_status_display() {
        assert_eq!(GarageStatus::Pending.to_string(), "pending");
        assert_eq!(GarageStatus::Running.to_string(), "running");
        assert_eq!(GarageStatus::Ready.to_string(), "ready");
        assert_eq!(GarageStatus::Attached.to_string(), "attached");
        assert_eq!(GarageStatus::Terminated.to_string(), "terminated");
    }

    #[test]
    fn garage_status_parse() {
        assert_eq!("pending".parse::<GarageStatus>().unwrap(), GarageStatus::Pending);
        assert_eq!("running".parse::<GarageStatus>().unwrap(), GarageStatus::Running);
        assert_eq!("ready".parse::<GarageStatus>().unwrap(), GarageStatus::Ready);
        assert_eq!("attached".parse::<GarageStatus>().unwrap(), GarageStatus::Attached);
        assert_eq!("terminated".parse::<GarageStatus>().unwrap(), GarageStatus::Terminated);
        assert!("invalid".parse::<GarageStatus>().is_err());
    }

    #[test]
    fn garage_status_serde_roundtrip() {
        for status in [
            GarageStatus::Pending,
            GarageStatus::Running,
            GarageStatus::Ready,
            GarageStatus::Attached,
            GarageStatus::Terminated,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: GarageStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn termination_reason_display() {
        assert_eq!(TerminationReason::UserClosed.to_string(), "user_closed");
        assert_eq!(TerminationReason::TtlExpired.to_string(), "ttl_expired");
        assert_eq!(TerminationReason::PodLost.to_string(), "pod_lost");
        assert_eq!(TerminationReason::NamespaceMissing.to_string(), "namespace_missing");
        assert_eq!(TerminationReason::ErrorReason.to_string(), "error");
    }

    #[test]
    fn termination_reason_parse() {
        assert_eq!("user_closed".parse::<TerminationReason>().unwrap(), TerminationReason::UserClosed);
        assert_eq!("ttl_expired".parse::<TerminationReason>().unwrap(), TerminationReason::TtlExpired);
        assert_eq!("pod_lost".parse::<TerminationReason>().unwrap(), TerminationReason::PodLost);
        assert_eq!("namespace_missing".parse::<TerminationReason>().unwrap(), TerminationReason::NamespaceMissing);
        assert_eq!("error".parse::<TerminationReason>().unwrap(), TerminationReason::ErrorReason);
        assert!("invalid".parse::<TerminationReason>().is_err());
    }

    #[test]
    fn termination_reason_serde_roundtrip() {
        for reason in [
            TerminationReason::UserClosed,
            TerminationReason::TtlExpired,
            TerminationReason::PodLost,
            TerminationReason::NamespaceMissing,
            TerminationReason::ErrorReason,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let parsed: TerminationReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, parsed);
        }
    }
}
