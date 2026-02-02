//! Garage `LimitRange` management.
//!
//! Creates the garage-limits `LimitRange` per garage-isolation.md spec:
//! - type: Container
//! - default: cpu "1", memory 1Gi
//! - defaultRequest: cpu 100m, memory 256Mi
//! - max: cpu "4", memory 8Gi

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{LimitRange, LimitRangeItem, LimitRangeSpec};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::ObjectMeta;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{LimitRangeOps, Result};

use crate::GarageK8s;

/// Name of the garage `LimitRange`.
pub const GARAGE_LIMITS_NAME: &str = "garage-limits";

/// Trait for garage `LimitRange` operations.
pub trait GarageLimitRangeOps {
    /// Creates the garage-limits `LimitRange` in the garage namespace.
    ///
    /// The limits (per garage-isolation.md spec lines 282-304):
    /// - type: Container
    /// - default: cpu "1", memory 1Gi
    /// - defaultRequest: cpu 100m, memory 256Mi
    /// - max: cpu "4", memory 8Gi
    ///
    /// # Errors
    ///
    /// Returns an error if the `LimitRange` already exists or creation fails.
    fn create_garage_limit_range(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<LimitRange>> + Send;

    /// Checks if the garage-limits `LimitRange` exists.
    fn garage_limit_range_exists(&self, id: &GarageId)
    -> impl Future<Output = Result<bool>> + Send;
}

impl GarageLimitRangeOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id))]
    async fn create_garage_limit_range(&self, id: &GarageId) -> Result<LimitRange> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "creating garage-limits LimitRange");

        let limit_range = build_garage_limits(&namespace);

        self.client()
            .create_limit_range(&namespace, &limit_range)
            .await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn garage_limit_range_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace = format!("moto-garage-{}", id.short());
        self.client()
            .limit_range_exists(&namespace, GARAGE_LIMITS_NAME)
            .await
    }
}

/// Builds the garage-limits `LimitRange` per garage-isolation.md spec.
fn build_garage_limits(namespace: &str) -> LimitRange {
    // Default limits per spec lines 295-297
    let mut default_limits = BTreeMap::new();
    default_limits.insert("cpu".to_string(), Quantity("1".to_string()));
    default_limits.insert("memory".to_string(), Quantity("1Gi".to_string()));

    // Default requests per spec lines 298-300
    let mut default_request = BTreeMap::new();
    default_request.insert("cpu".to_string(), Quantity("100m".to_string()));
    default_request.insert("memory".to_string(), Quantity("256Mi".to_string()));

    // Max limits per spec lines 301-303
    let mut max = BTreeMap::new();
    max.insert("cpu".to_string(), Quantity("4".to_string()));
    max.insert("memory".to_string(), Quantity("8Gi".to_string()));

    LimitRange {
        metadata: ObjectMeta {
            name: Some(GARAGE_LIMITS_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(LimitRangeSpec {
            limits: vec![LimitRangeItem {
                type_: "Container".to_string(),
                default: Some(default_limits),
                default_request: Some(default_request),
                max: Some(max),
                ..Default::default()
            }],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_garage_limits_structure() {
        let limit_range = build_garage_limits("moto-garage-abc12345");

        // Check metadata
        assert_eq!(
            limit_range.metadata.name,
            Some(GARAGE_LIMITS_NAME.to_string())
        );
        assert_eq!(
            limit_range.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check spec
        let spec = limit_range.spec.as_ref().unwrap();
        assert_eq!(spec.limits.len(), 1);

        let item = &spec.limits[0];
        assert_eq!(item.type_, "Container".to_string());

        // Check default limits
        let default = item.default.as_ref().unwrap();
        assert_eq!(default.get("cpu"), Some(&Quantity("1".to_string())));
        assert_eq!(default.get("memory"), Some(&Quantity("1Gi".to_string())));

        // Check default requests
        let default_request = item.default_request.as_ref().unwrap();
        assert_eq!(
            default_request.get("cpu"),
            Some(&Quantity("100m".to_string()))
        );
        assert_eq!(
            default_request.get("memory"),
            Some(&Quantity("256Mi".to_string()))
        );

        // Check max limits
        let max = item.max.as_ref().unwrap();
        assert_eq!(max.get("cpu"), Some(&Quantity("4".to_string())));
        assert_eq!(max.get("memory"), Some(&Quantity("8Gi".to_string())));
    }

    #[test]
    fn garage_limits_has_all_required_fields() {
        let limit_range = build_garage_limits("test-ns");
        let spec = limit_range.spec.as_ref().unwrap();
        let item = &spec.limits[0];

        // Per garage-isolation.md spec, all these fields must be present
        assert!(item.default.is_some());
        assert!(item.default_request.is_some());
        assert!(item.max.is_some());

        let default = item.default.as_ref().unwrap();
        assert!(default.contains_key("cpu"));
        assert!(default.contains_key("memory"));
        assert_eq!(default.len(), 2);

        let default_request = item.default_request.as_ref().unwrap();
        assert!(default_request.contains_key("cpu"));
        assert!(default_request.contains_key("memory"));
        assert_eq!(default_request.len(), 2);

        let max = item.max.as_ref().unwrap();
        assert!(max.contains_key("cpu"));
        assert!(max.contains_key("memory"));
        assert_eq!(max.len(), 2);
    }
}
