//! Kubernetes label constants for moto resources.
//!
//! All moto labels use the `moto.dev/` prefix.

use std::collections::BTreeMap;

/// Constants for moto Kubernetes labels.
pub struct Labels;

impl Labels {
    /// Label key for resource type ("garage" or "bike").
    pub const TYPE: &'static str = "moto.dev/type";

    /// Label key for resource ID (UUID).
    pub const ID: &'static str = "moto.dev/id";

    /// Label key for human-friendly name.
    pub const NAME: &'static str = "moto.dev/name";

    /// Label key for owner identifier.
    pub const OWNER: &'static str = "moto.dev/owner";

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
    pub fn for_garage(id: &str, name: &str, owner: Option<&str>) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert(Self::TYPE.to_string(), Self::TYPE_GARAGE.to_string());
        labels.insert(Self::ID.to_string(), id.to_string());
        labels.insert(Self::NAME.to_string(), name.to_string());
        if let Some(owner) = owner {
            labels.insert(Self::OWNER.to_string(), owner.to_string());
        }
        labels
    }

    /// Creates labels for a bike namespace.
    #[must_use]
    pub fn for_bike(id: &str, name: &str, owner: Option<&str>) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert(Self::TYPE.to_string(), Self::TYPE_BIKE.to_string());
        labels.insert(Self::ID.to_string(), id.to_string());
        labels.insert(Self::NAME.to_string(), name.to_string());
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
        let labels = Labels::for_garage("abc-123", "my-project", None);
        assert_eq!(labels.get(Labels::TYPE), Some(&"garage".to_string()));
        assert_eq!(labels.get(Labels::ID), Some(&"abc-123".to_string()));
        assert_eq!(labels.get(Labels::NAME), Some(&"my-project".to_string()));
        assert!(labels.get(Labels::OWNER).is_none());
    }

    #[test]
    fn for_garage_with_owner() {
        let labels = Labels::for_garage("abc-123", "my-project", Some("alice"));
        assert_eq!(labels.get(Labels::OWNER), Some(&"alice".to_string()));
    }

    #[test]
    fn for_bike_without_owner() {
        let labels = Labels::for_bike("def-456", "prod-app", None);
        assert_eq!(labels.get(Labels::TYPE), Some(&"bike".to_string()));
        assert_eq!(labels.get(Labels::ID), Some(&"def-456".to_string()));
        assert_eq!(labels.get(Labels::NAME), Some(&"prod-app".to_string()));
        assert!(labels.get(Labels::OWNER).is_none());
    }
}
