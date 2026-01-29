//! Kubernetes label constants for moto resources.
//!
//! All moto labels use the `moto.dev/` prefix.

use std::collections::BTreeMap;

/// Constants for moto Kubernetes labels.
pub struct Labels;

impl Labels {
    /// Label key for resource type ("garage" or "bike").
    pub const TYPE: &'static str = "moto.dev/type";

    /// Label key for garage ID (UUID).
    pub const GARAGE_ID: &'static str = "moto.dev/garage-id";

    /// Label key for garage human-friendly name.
    pub const GARAGE_NAME: &'static str = "moto.dev/garage-name";

    /// Label key for bike ID (UUID).
    pub const BIKE_ID: &'static str = "moto.dev/bike-id";

    /// Label key for bike human-friendly name.
    pub const BIKE_NAME: &'static str = "moto.dev/bike-name";

    /// Label key for owner identifier.
    pub const OWNER: &'static str = "moto.dev/owner";

    /// Label key for expiration timestamp (RFC 3339 format).
    pub const EXPIRES_AT: &'static str = "moto.dev/expires-at";

    /// Label key for engine (what the garage is working on).
    pub const ENGINE: &'static str = "moto.dev/engine";

    /// Value for garage type.
    pub const TYPE_GARAGE: &'static str = "garage";

    /// Value for bike type.
    pub const TYPE_BIKE: &'static str = "bike";

    /// Creates a label selector for moto garages.
    #[must_use]
    pub fn garage_selector() -> String {
        format!("{}={}", Self::TYPE, Self::TYPE_GARAGE)
    }

    /// Creates a label selector for moto bikes.
    #[must_use]
    pub fn bike_selector() -> String {
        format!("{}={}", Self::TYPE, Self::TYPE_BIKE)
    }

    /// Creates labels for a garage namespace.
    #[must_use]
    pub fn for_garage(
        id: &str,
        name: &str,
        owner: Option<&str>,
        expires_at: Option<&str>,
        engine: Option<&str>,
    ) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert(Self::TYPE.to_string(), Self::TYPE_GARAGE.to_string());
        labels.insert(Self::GARAGE_ID.to_string(), id.to_string());
        labels.insert(Self::GARAGE_NAME.to_string(), name.to_string());
        if let Some(owner) = owner {
            labels.insert(Self::OWNER.to_string(), owner.to_string());
        }
        if let Some(expires_at) = expires_at {
            labels.insert(Self::EXPIRES_AT.to_string(), expires_at.to_string());
        }
        if let Some(engine) = engine {
            labels.insert(Self::ENGINE.to_string(), engine.to_string());
        }
        labels
    }

    /// Creates labels for a bike namespace.
    #[must_use]
    pub fn for_bike(id: &str, name: &str, owner: Option<&str>) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert(Self::TYPE.to_string(), Self::TYPE_BIKE.to_string());
        labels.insert(Self::BIKE_ID.to_string(), id.to_string());
        labels.insert(Self::BIKE_NAME.to_string(), name.to_string());
        if let Some(owner) = owner {
            labels.insert(Self::OWNER.to_string(), owner.to_string());
        }
        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garage_selector() {
        assert_eq!(Labels::garage_selector(), "moto.dev/type=garage");
    }

    #[test]
    fn bike_selector() {
        assert_eq!(Labels::bike_selector(), "moto.dev/type=bike");
    }

    #[test]
    fn for_garage_without_owner() {
        let labels = Labels::for_garage("abc-123", "my-project", None, None, None);
        assert_eq!(labels.get(Labels::TYPE), Some(&"garage".to_string()));
        assert_eq!(labels.get(Labels::GARAGE_ID), Some(&"abc-123".to_string()));
        assert_eq!(
            labels.get(Labels::GARAGE_NAME),
            Some(&"my-project".to_string())
        );
        assert!(labels.get(Labels::OWNER).is_none());
        assert!(labels.get(Labels::EXPIRES_AT).is_none());
        assert!(labels.get(Labels::ENGINE).is_none());
    }

    #[test]
    fn for_garage_with_owner() {
        let labels = Labels::for_garage("abc-123", "my-project", Some("alice"), None, None);
        assert_eq!(labels.get(Labels::OWNER), Some(&"alice".to_string()));
    }

    #[test]
    fn for_garage_with_expires_at() {
        let labels = Labels::for_garage(
            "abc-123",
            "my-project",
            None,
            Some("2026-01-21T14:00:00Z"),
            None,
        );
        assert_eq!(
            labels.get(Labels::EXPIRES_AT),
            Some(&"2026-01-21T14:00:00Z".to_string())
        );
    }

    #[test]
    fn for_garage_with_engine() {
        let labels = Labels::for_garage("abc-123", "my-project", None, None, Some("moto-club"));
        assert_eq!(labels.get(Labels::ENGINE), Some(&"moto-club".to_string()));
    }

    #[test]
    fn for_bike_without_owner() {
        let labels = Labels::for_bike("def-456", "prod-app", None);
        assert_eq!(labels.get(Labels::TYPE), Some(&"bike".to_string()));
        assert_eq!(labels.get(Labels::BIKE_ID), Some(&"def-456".to_string()));
        assert_eq!(labels.get(Labels::BIKE_NAME), Some(&"prod-app".to_string()));
        assert!(labels.get(Labels::OWNER).is_none());
    }
}
