//! Garage types: ID and shared structures.
//!
//! NOTE: `GarageStatus` is defined in `moto-club-db/src/models.rs`.
//! Per spec v1.6, we use a single status enum to avoid confusion.

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
}
