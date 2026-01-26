//! Kubernetes client wrapper.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::Namespace;
use kube::{
    Client, Config,
    api::{Api, ListParams, ObjectMeta, PostParams},
    config::{KubeConfigOptions, Kubeconfig},
};
use tracing::{debug, instrument};

use crate::{Error, NamespaceOps, Result};

/// A wrapper around `kube::Client` providing moto-specific operations.
///
/// This client handles:
/// - Namespace CRUD operations
/// - Pod CRUD operations (future)
/// - Label-based filtering for moto resources
#[derive(Clone)]
pub struct K8sClient {
    client: Client,
}

impl K8sClient {
    /// Creates a new client from the default kubeconfig.
    ///
    /// This uses the standard kubeconfig discovery:
    /// 1. `MOTOCONFIG` environment variable (moto-specific override)
    /// 2. `KUBECONFIG` environment variable
    /// 3. `~/.kube/config`
    /// 4. In-cluster config (when running inside K8s)
    ///
    /// # Errors
    ///
    /// Returns an error if the kubeconfig cannot be loaded or is invalid.
    pub async fn new() -> Result<Self> {
        // Check for MOTOCONFIG first, then fall back to standard discovery
        if let Ok(path) = std::env::var("MOTOCONFIG") {
            return Self::from_kubeconfig_path(&path).await;
        }

        let client = Client::try_default().await.map_err(Error::ClientCreate)?;
        Ok(Self { client })
    }

    /// Creates a new client from a specific kubeconfig file path.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubeconfig file cannot be read or is invalid.
    pub async fn from_kubeconfig_path(path: &str) -> Result<Self> {
        let kubeconfig = Kubeconfig::read_from(path).map_err(Error::KubeconfigRead)?;
        let config = Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
            .await
            .map_err(Error::KubeconfigRead)?;
        let client = Client::try_from(config).map_err(Error::ClientCreate)?;
        Ok(Self { client })
    }

    /// Creates a new client for a specific kubeconfig context.
    ///
    /// This respects the `MOTOCONFIG` environment variable for the kubeconfig path.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubeconfig cannot be loaded, the context doesn't
    /// exist, or the client cannot be created.
    pub async fn with_context(context: &str) -> Result<Self> {
        let options = KubeConfigOptions {
            context: Some(context.to_string()),
            ..Default::default()
        };

        // Check for MOTOCONFIG first
        let config = if let Ok(path) = std::env::var("MOTOCONFIG") {
            let kubeconfig = Kubeconfig::read_from(path).map_err(Error::KubeconfigRead)?;
            Config::from_custom_kubeconfig(kubeconfig, &options)
                .await
                .map_err(Error::KubeconfigRead)?
        } else {
            Config::from_kubeconfig(&options)
                .await
                .map_err(Error::KubeconfigRead)?
        };

        let client = Client::try_from(config).map_err(Error::ClientCreate)?;
        Ok(Self { client })
    }

    /// Creates a new client from an existing `kube::Client`.
    ///
    /// Useful for testing or when you already have a configured client.
    #[must_use]
    pub fn from_client(client: Client) -> Self {
        Self { client }
    }

    /// Returns a reference to the underlying `kube::Client`.
    #[must_use]
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Returns a list of available kubeconfig context names.
    ///
    /// This respects the `MOTOCONFIG` environment variable for the kubeconfig path.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubeconfig cannot be read.
    pub fn list_contexts() -> Result<Vec<String>> {
        let kubeconfig = Self::read_kubeconfig()?;
        Ok(kubeconfig.contexts.into_iter().map(|c| c.name).collect())
    }

    /// Returns the current kubeconfig context name.
    ///
    /// This respects the `MOTOCONFIG` environment variable for the kubeconfig path.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubeconfig cannot be read.
    pub fn current_context() -> Result<Option<String>> {
        let kubeconfig = Self::read_kubeconfig()?;
        Ok(kubeconfig.current_context)
    }

    /// Reads the kubeconfig, respecting `MOTOCONFIG` env var.
    fn read_kubeconfig() -> Result<Kubeconfig> {
        if let Ok(path) = std::env::var("MOTOCONFIG") {
            Kubeconfig::read_from(path).map_err(Error::KubeconfigRead)
        } else {
            Kubeconfig::read().map_err(Error::KubeconfigRead)
        }
    }
}

impl NamespaceOps for K8sClient {
    #[instrument(skip(self, labels), fields(namespace = %name))]
    async fn create_namespace(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<Namespace> {
        let api: Api<Namespace> = Api::all(self.client.clone());

        // Check if namespace already exists
        if self.namespace_exists(name).await? {
            return Err(Error::NamespaceExists(name.to_string()));
        }

        let ns = Namespace {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        };

        debug!("creating namespace");
        let created = api
            .create(&PostParams::default(), &ns)
            .await
            .map_err(Error::NamespaceCreate)?;

        Ok(created)
    }

    #[instrument(skip(self), fields(namespace = %name))]
    async fn delete_namespace(&self, name: &str) -> Result<()> {
        let api: Api<Namespace> = Api::all(self.client.clone());

        // Check if namespace exists first
        if !self.namespace_exists(name).await? {
            return Err(Error::NamespaceNotFound(name.to_string()));
        }

        debug!("deleting namespace");
        api.delete(name, &Default::default())
            .await
            .map_err(Error::NamespaceDelete)?;

        Ok(())
    }

    #[instrument(skip(self), fields(namespace = %name))]
    async fn get_namespace(&self, name: &str) -> Result<Namespace> {
        let api: Api<Namespace> = Api::all(self.client.clone());

        debug!("getting namespace");
        api.get(name).await.map_err(|e| {
            if is_not_found(&e) {
                Error::NamespaceNotFound(name.to_string())
            } else {
                Error::NamespaceGet(e)
            }
        })
    }

    #[instrument(skip(self))]
    async fn list_namespaces(&self, label_selector: Option<&str>) -> Result<Vec<Namespace>> {
        let api: Api<Namespace> = Api::all(self.client.clone());

        let mut params = ListParams::default();
        if let Some(selector) = label_selector {
            params = params.labels(selector);
        }

        debug!("listing namespaces");
        let list = api.list(&params).await.map_err(Error::NamespaceList)?;

        Ok(list.items)
    }

    #[instrument(skip(self), fields(namespace = %name))]
    async fn namespace_exists(&self, name: &str) -> Result<bool> {
        let api: Api<Namespace> = Api::all(self.client.clone());

        match api.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(Error::NamespaceGet(e)),
        }
    }
}

/// Checks if a kube error is a "not found" error.
fn is_not_found(e: &kube::Error) -> bool {
    matches!(
        e,
        kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running K8s cluster.
    // They are marked #[ignore] by default and can be run with:
    // cargo test --package moto-k8s -- --ignored

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn client_creation() {
        let client = K8sClient::new().await;
        assert!(
            client.is_ok(),
            "Failed to create client: {:?}",
            client.err()
        );
    }

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn list_namespaces() {
        let client = K8sClient::new().await.expect("client creation failed");
        let namespaces = client.list_namespaces(None).await;
        assert!(
            namespaces.is_ok(),
            "Failed to list namespaces: {:?}",
            namespaces.err()
        );
        // Should at least have 'default' and 'kube-system'
        let ns = namespaces.unwrap();
        assert!(ns.len() >= 2, "Expected at least 2 namespaces");
    }
}
