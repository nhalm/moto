//! `NetworkPolicy` operations.
//!
//! Provides low-level CRUD operations for Kubernetes `NetworkPolicy` resources.

use std::future::Future;

use k8s_openapi::api::networking::v1::NetworkPolicy;
use kube::api::{Api, DeleteParams, PostParams};
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Trait for `NetworkPolicy` operations.
pub trait NetworkPolicyOps {
    /// Creates a `NetworkPolicy` in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `NetworkPolicy` already exists or creation fails.
    fn create_network_policy(
        &self,
        namespace: &str,
        network_policy: &NetworkPolicy,
    ) -> impl Future<Output = Result<NetworkPolicy>> + Send;

    /// Gets a `NetworkPolicy` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `NetworkPolicy` doesn't exist or the operation fails.
    fn get_network_policy(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<NetworkPolicy>> + Send;

    /// Deletes a `NetworkPolicy` by name in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `NetworkPolicy` doesn't exist or deletion fails.
    fn delete_network_policy(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Checks if a `NetworkPolicy` exists in the specified namespace.
    fn network_policy_exists(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl Future<Output = Result<bool>> + Send;
}

impl NetworkPolicyOps for K8sClient {
    #[instrument(skip(self, network_policy), fields(namespace = %namespace, policy_name = ?network_policy.metadata.name))]
    async fn create_network_policy(
        &self,
        namespace: &str,
        network_policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy> {
        let api: Api<NetworkPolicy> = Api::namespaced(self.inner().clone(), namespace);
        let name = network_policy.metadata.name.as_deref().unwrap_or("unknown");

        // Check if NetworkPolicy already exists
        if self.network_policy_exists(namespace, name).await? {
            return Err(Error::NetworkPolicyExists(format!("{namespace}/{name}")));
        }

        debug!("creating NetworkPolicy");
        let created = api
            .create(&PostParams::default(), network_policy)
            .await
            .map_err(Error::NetworkPolicyCreate)?;

        Ok(created)
    }

    #[instrument(skip(self), fields(namespace = %namespace, policy_name = %name))]
    async fn get_network_policy(&self, namespace: &str, name: &str) -> Result<NetworkPolicy> {
        let api: Api<NetworkPolicy> = Api::namespaced(self.inner().clone(), namespace);

        debug!("getting NetworkPolicy");
        api.get(name).await.map_err(|e| {
            if is_not_found(&e) {
                Error::NetworkPolicyNotFound(format!("{namespace}/{name}"))
            } else {
                Error::NetworkPolicyGet(e)
            }
        })
    }

    #[instrument(skip(self), fields(namespace = %namespace, policy_name = %name))]
    async fn delete_network_policy(&self, namespace: &str, name: &str) -> Result<()> {
        let api: Api<NetworkPolicy> = Api::namespaced(self.inner().clone(), namespace);

        // Check if NetworkPolicy exists first
        if !self.network_policy_exists(namespace, name).await? {
            return Err(Error::NetworkPolicyNotFound(format!("{namespace}/{name}")));
        }

        debug!("deleting NetworkPolicy");
        api.delete(name, &DeleteParams::default())
            .await
            .map_err(Error::NetworkPolicyDelete)?;

        Ok(())
    }

    #[instrument(skip(self), fields(namespace = %namespace, policy_name = %name))]
    async fn network_policy_exists(&self, namespace: &str, name: &str) -> Result<bool> {
        let api: Api<NetworkPolicy> = Api::namespaced(self.inner().clone(), namespace);

        match api.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(Error::NetworkPolicyGet(e)),
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
    use k8s_openapi::api::networking::v1::{NetworkPolicyEgressRule, NetworkPolicySpec};
    use kube::api::ObjectMeta;

    #[test]
    fn network_policy_ops_trait_defined() {
        // Compile-time check that the trait is properly defined
        fn assert_network_policy_ops<T: NetworkPolicyOps>() {}
        assert_network_policy_ops::<K8sClient>();
    }

    #[test]
    fn build_basic_network_policy() {
        let policy = NetworkPolicy {
            metadata: ObjectMeta {
                name: Some("test-policy".to_string()),
                namespace: Some("test-ns".to_string()),
                ..Default::default()
            },
            spec: Some(NetworkPolicySpec {
                pod_selector: Default::default(),
                policy_types: Some(vec!["Ingress".to_string(), "Egress".to_string()]),
                ingress: Some(vec![]),
                egress: Some(vec![NetworkPolicyEgressRule {
                    to: None,
                    ports: None,
                }]),
            }),
        };

        assert_eq!(policy.metadata.name, Some("test-policy".to_string()));
        assert_eq!(policy.metadata.namespace, Some("test-ns".to_string()));

        let spec = policy.spec.as_ref().unwrap();
        assert_eq!(
            spec.policy_types,
            Some(vec!["Ingress".to_string(), "Egress".to_string()])
        );
    }
}
