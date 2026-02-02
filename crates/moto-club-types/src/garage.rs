//! Garage types: ID, state, and info structures.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for a garage (UUID v7).
///
/// Wraps a UUID v7 and provides a `.short()` method for display (first 8 chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GarageId(Uuid);

impl GarageId {
    /// Creates a new `GarageId` with a UUID v7 (time-ordered).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Creates a `GarageId` from an existing UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Returns the short form for display (first 8 characters).
    #[must_use]
    pub fn short(&self) -> String {
        self.0.to_string()[..8].to_string()
    }
}

impl Default for GarageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for GarageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for GarageId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// The lifecycle state of a garage.
///
/// Per garage-lifecycle.md v0.3, the 5 states are:
/// Pending → Initializing → Ready/Failed → Terminated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GarageState {
    /// Garage is being created (namespace exists, pods starting, pulling images).
    Pending,
    /// Garage pods are running, initializing (cloning repo, starting services).
    Initializing,
    /// Garage is fully ready for use (all ready criteria met).
    Ready,
    /// Startup failed (clone error, pod failure, etc.).
    Failed,
    /// Garage has been terminated.
    Terminated,
}

impl std::fmt::Display for GarageState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Initializing => "initializing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Terminated => "terminated",
        };
        write!(f, "{s}")
    }
}

/// Information about a garage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GarageInfo {
    /// Unique identifier for this garage.
    pub id: GarageId,
    /// Human-friendly name.
    pub name: String,
    /// Kubernetes namespace name (e.g., `moto-garage-{short_id}`).
    pub namespace: String,
    /// Current lifecycle state.
    pub state: GarageState,
    /// When the garage was created.
    pub created_at: DateTime<Utc>,
    /// When the garage will expire (if TTL is set).
    pub expires_at: Option<DateTime<Utc>>,
    /// Owner identifier (user or team).
    pub owner: Option<String>,
    /// Engine name (what the garage is working on).
    pub engine: Option<String>,
}

impl GarageInfo {
    /// Creates a new `GarageInfo` with the given name.
    ///
    /// Generates a new ID and derives the namespace from it.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let id = GarageId::new();
        let namespace = format!("moto-garage-{}", id.short());
        Self {
            id,
            name: name.into(),
            namespace,
            state: GarageState::Pending,
            created_at: Utc::now(),
            expires_at: None,
            owner: None,
            engine: None,
        }
    }

    /// Sets the owner of this garage.
    #[must_use]
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    /// Sets the expiration time of this garage.
    #[must_use]
    pub const fn with_expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Sets the engine (what this garage is working on).
    #[must_use]
    pub fn with_engine(mut self, engine: impl Into<String>) -> Self {
        self.engine = Some(engine.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garage_id_short_returns_first_8_chars() {
        let id = GarageId::new();
        let short = id.short();
        assert_eq!(short.len(), 8);
        assert!(id.to_string().starts_with(&short));
    }

    #[test]
    fn garage_id_roundtrip() {
        let id = GarageId::new();
        let s = id.to_string();
        let parsed: GarageId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn garage_id_serde_roundtrip() {
        let id = GarageId::new();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: GarageId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn garage_state_display() {
        assert_eq!(GarageState::Pending.to_string(), "pending");
        assert_eq!(GarageState::Initializing.to_string(), "initializing");
        assert_eq!(GarageState::Ready.to_string(), "ready");
        assert_eq!(GarageState::Failed.to_string(), "failed");
        assert_eq!(GarageState::Terminated.to_string(), "terminated");
    }

    #[test]
    fn garage_state_serde_roundtrip() {
        for state in [
            GarageState::Pending,
            GarageState::Initializing,
            GarageState::Ready,
            GarageState::Failed,
            GarageState::Terminated,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let parsed: GarageState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, parsed);
        }
    }

    #[test]
    fn garage_info_new_sets_namespace() {
        let info = GarageInfo::new("my-project");
        assert_eq!(info.name, "my-project");
        assert!(info.namespace.starts_with("moto-garage-"));
        assert_eq!(info.namespace.len(), "moto-garage-".len() + 8);
        assert_eq!(info.state, GarageState::Pending);
        assert!(info.expires_at.is_none());
        assert!(info.owner.is_none());
        assert!(info.engine.is_none());
    }

    #[test]
    fn garage_info_with_engine() {
        let info = GarageInfo::new("my-project").with_engine("moto-club");
        assert_eq!(info.engine, Some("moto-club".to_string()));
    }

    #[test]
    fn garage_info_with_owner() {
        let info = GarageInfo::new("my-project").with_owner("alice");
        assert_eq!(info.owner, Some("alice".to_string()));
    }

    #[test]
    fn garage_info_serde_roundtrip() {
        let info = GarageInfo::new("my-project").with_owner("alice");
        let json = serde_json::to_string(&info).unwrap();
        let parsed: GarageInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, parsed);
    }
}
