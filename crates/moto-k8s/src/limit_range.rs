//! `LimitRange` operations.
//!
//! Provides low-level CRUD operations for Kubernetes `LimitRange` resources.

use std::future::Future;

use k8s_openapi::api::core::v1::LimitRange;
use kube::api::{Api, DeleteParams, PostParams};
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Trait for `LimitRange` operations.
pub trait LimitRangeOps {
    /// Creates a `LimitRange` in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `LimitRange` already exists or creation fails.
    fn create_limit_range(
        &self,
        namespace: &str,
        limit_range: &LimitRange,
    ) -> impl Future<Output = Result<LimitRange>> + Send;

    /// Gets a `LimitRange` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `LimitRange` doesn't exist or the operation fails.
    fn get_limit_range(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<LimitRange>> + Send;

    /// Deletes a `LimitRange` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `LimitRange` doesn't exist or deletion fails.
    fn delete_limit_range(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Checks if a `LimitRange` exists in the specified namespace.
    fn limit_range_exists(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<bool>> + Send;
}

impl LimitRangeOps for K8sClient {
    #[instrument(skip(self, limit_range), fields(namespace = %namespace, limit_range_name = ?limit_range.metadata.name))]
    async fn create_limit_range(
        &self,
        namespace: &str,
        limit_range: &LimitRange,
    ) -> Result<LimitRange> {
        let api: Api<LimitRange> = Api::namespaced(self.inner().clone(), namespace);
        let name = limit_range.metadata.name.as_deref().unwrap_or("unknown");

        // Check if LimitRange already exists
        if self.limit_range_exists(namespace, name).await? {
            return Err(Error::LimitRangeExists(format!("{namespace}/{name}")));
        }

        debug!("creating LimitRange");
        let created = api
            .create(&PostParams::default(), limit_range)
            .await
            .map_err(Error::LimitRangeCreate)?;

        Ok(created)
    }

    #[instrument(skip(self), fields(namespace = %namespace, limit_range_name = %name))]
    async fn get_limit_range(&self, namespace: &str, name: &str) -> Result<LimitRange> {
        let api: Api<LimitRange> = Api::namespaced(self.inner().clone(), namespace);

        debug!("getting LimitRange");
        api.get(name).await.map_err(|e| {
            if is_not_found(&e) {
                Error::LimitRangeNotFound(format!("{namespace}/{name}"))
            } else {
                Error::LimitRangeGet(e)
            }
        })
    }

    #[instrument(skip(self), fields(namespace = %namespace, limit_range_name = %name))]
    async fn delete_limit_range(&self, namespace: &str, name: &str) -> Result<()> {
        let api: Api<LimitRange> = Api::namespaced(self.inner().clone(), namespace);

        // Check if LimitRange exists first
        if !self.limit_range_exists(namespace, name).await? {
            return Err(Error::LimitRangeNotFound(format!("{namespace}/{name}")));
        }

        debug!("deleting LimitRange");
        api.delete(name, &DeleteParams::default())
            .await
            .map_err(Error::LimitRangeDelete)?;

        Ok(())
    }

    #[instrument(skip(self), fields(namespace = %namespace, limit_range_name = %name))]
    async fn limit_range_exists(&self, namespace: &str, name: &str) -> Result<bool> {
        let api: Api<LimitRange> = Api::namespaced(self.inner().clone(), namespace);

        match api.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(Error::LimitRangeGet(e)),
        }
    }
}

/// Checks if a kube error is a "not found" error.
const fn is_not_found(e: &kube::Error) -> bool {
    matches!(
        e,
        kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{LimitRangeItem, LimitRangeSpec};
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use kube::api::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn limit_range_ops_trait_defined() {
        // Compile-time check that the trait is properly defined
        fn assert_limit_range_ops<T: LimitRangeOps>() {}
        assert_limit_range_ops::<K8sClient>();
    }

    #[test]
    fn build_basic_limit_range() {
        let mut default_limits = BTreeMap::new();
        default_limits.insert("cpu".to_string(), Quantity("1".to_string()));
        default_limits.insert("memory".to_string(), Quantity("1Gi".to_string()));

        let mut default_request = BTreeMap::new();
        default_request.insert("cpu".to_string(), Quantity("100m".to_string()));
        default_request.insert("memory".to_string(), Quantity("256Mi".to_string()));

        let mut max = BTreeMap::new();
        max.insert("cpu".to_string(), Quantity("4".to_string()));
        max.insert("memory".to_string(), Quantity("8Gi".to_string()));

        let limit_range = LimitRange {
            metadata: ObjectMeta {
                name: Some("test-limits".to_string()),
                namespace: Some("test-ns".to_string()),
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
        };

        assert_eq!(limit_range.metadata.name, Some("test-limits".to_string()));
        assert_eq!(limit_range.metadata.namespace, Some("test-ns".to_string()));

        let spec = limit_range.spec.as_ref().unwrap();
        assert_eq!(spec.limits.len(), 1);

        let item = &spec.limits[0];
        assert_eq!(item.type_, "Container".to_string());

        let default = item.default.as_ref().unwrap();
        assert_eq!(default.get("cpu"), Some(&Quantity("1".to_string())));
        assert_eq!(default.get("memory"), Some(&Quantity("1Gi".to_string())));
    }
}
