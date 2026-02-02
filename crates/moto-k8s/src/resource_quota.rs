//! `ResourceQuota` operations.
//!
//! Provides low-level CRUD operations for Kubernetes `ResourceQuota` resources.

use std::future::Future;

use k8s_openapi::api::core::v1::ResourceQuota;
use kube::api::{Api, DeleteParams, PostParams};
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Trait for `ResourceQuota` operations.
pub trait ResourceQuotaOps {
    /// Creates a `ResourceQuota` in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `ResourceQuota` already exists or creation fails.
    fn create_resource_quota(
        &self,
        namespace: &str,
        resource_quota: &ResourceQuota,
    ) -> impl Future<Output = Result<ResourceQuota>> + Send;

    /// Gets a `ResourceQuota` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `ResourceQuota` doesn't exist or the operation fails.
    fn get_resource_quota(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<ResourceQuota>> + Send;

    /// Deletes a `ResourceQuota` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `ResourceQuota` doesn't exist or deletion fails.
    fn delete_resource_quota(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Checks if a `ResourceQuota` exists in the specified namespace.
    fn resource_quota_exists(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<bool>> + Send;
}

impl ResourceQuotaOps for K8sClient {
    #[instrument(skip(self, resource_quota), fields(namespace = %namespace, quota_name = ?resource_quota.metadata.name))]
    async fn create_resource_quota(
        &self,
        namespace: &str,
        resource_quota: &ResourceQuota,
    ) -> Result<ResourceQuota> {
        let api: Api<ResourceQuota> = Api::namespaced(self.inner().clone(), namespace);
        let name = resource_quota.metadata.name.as_deref().unwrap_or("unknown");

        // Check if ResourceQuota already exists
        if self.resource_quota_exists(namespace, name).await? {
            return Err(Error::ResourceQuotaExists(format!("{namespace}/{name}")));
        }

        debug!("creating ResourceQuota");
        let created = api
            .create(&PostParams::default(), resource_quota)
            .await
            .map_err(Error::ResourceQuotaCreate)?;

        Ok(created)
    }

    #[instrument(skip(self), fields(namespace = %namespace, quota_name = %name))]
    async fn get_resource_quota(&self, namespace: &str, name: &str) -> Result<ResourceQuota> {
        let api: Api<ResourceQuota> = Api::namespaced(self.inner().clone(), namespace);

        debug!("getting ResourceQuota");
        api.get(name).await.map_err(|e| {
            if is_not_found(&e) {
                Error::ResourceQuotaNotFound(format!("{namespace}/{name}"))
            } else {
                Error::ResourceQuotaGet(e)
            }
        })
    }

    #[instrument(skip(self), fields(namespace = %namespace, quota_name = %name))]
    async fn delete_resource_quota(&self, namespace: &str, name: &str) -> Result<()> {
        let api: Api<ResourceQuota> = Api::namespaced(self.inner().clone(), namespace);

        // Check if ResourceQuota exists first
        if !self.resource_quota_exists(namespace, name).await? {
            return Err(Error::ResourceQuotaNotFound(format!("{namespace}/{name}")));
        }

        debug!("deleting ResourceQuota");
        api.delete(name, &DeleteParams::default())
            .await
            .map_err(Error::ResourceQuotaDelete)?;

        Ok(())
    }

    #[instrument(skip(self), fields(namespace = %namespace, quota_name = %name))]
    async fn resource_quota_exists(&self, namespace: &str, name: &str) -> Result<bool> {
        let api: Api<ResourceQuota> = Api::namespaced(self.inner().clone(), namespace);

        match api.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(Error::ResourceQuotaGet(e)),
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
    use k8s_openapi::api::core::v1::ResourceQuotaSpec;
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use kube::api::ObjectMeta;
    use std::collections::BTreeMap;

    #[test]
    fn resource_quota_ops_trait_defined() {
        // Compile-time check that the trait is properly defined
        fn assert_resource_quota_ops<T: ResourceQuotaOps>() {}
        assert_resource_quota_ops::<K8sClient>();
    }

    #[test]
    fn build_basic_resource_quota() {
        let mut hard = BTreeMap::new();
        hard.insert("pods".to_string(), Quantity("10".to_string()));
        hard.insert("requests.cpu".to_string(), Quantity("4".to_string()));

        let quota = ResourceQuota {
            metadata: ObjectMeta {
                name: Some("test-quota".to_string()),
                namespace: Some("test-ns".to_string()),
                ..Default::default()
            },
            spec: Some(ResourceQuotaSpec {
                hard: Some(hard),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(quota.metadata.name, Some("test-quota".to_string()));
        assert_eq!(quota.metadata.namespace, Some("test-ns".to_string()));

        let spec = quota.spec.as_ref().unwrap();
        let hard = spec.hard.as_ref().unwrap();
        assert_eq!(hard.get("pods"), Some(&Quantity("10".to_string())));
        assert_eq!(hard.get("requests.cpu"), Some(&Quantity("4".to_string())));
    }
}
