//! Kubernetes RBAC operations for Role and `RoleBinding`.

use std::collections::BTreeMap;

use k8s_openapi::api::rbac::v1::{PolicyRule, Role, RoleBinding, RoleRef, Subject};
use kube::{
    Api,
    api::{ObjectMeta, PostParams},
};
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Trait for RBAC operations.
pub trait RbacOps {
    /// Creates a Role in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the Role already exists or creation fails.
    fn create_role(
        &self,
        namespace: &str,
        name: &str,
        rules: Vec<PolicyRule>,
        labels: BTreeMap<String, String>,
    ) -> impl std::future::Future<Output = Result<Role>> + Send;

    /// Creates a `RoleBinding` in the specified namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the `RoleBinding` already exists or creation fails.
    fn create_role_binding(
        &self,
        namespace: &str,
        name: &str,
        role_name: &str,
        subjects: Vec<Subject>,
        labels: BTreeMap<String, String>,
    ) -> impl std::future::Future<Output = Result<RoleBinding>> + Send;
}

impl RbacOps for K8sClient {
    #[instrument(skip(self, rules, labels), fields(namespace = %namespace, role_name = %name))]
    async fn create_role(
        &self,
        namespace: &str,
        name: &str,
        rules: Vec<PolicyRule>,
        labels: BTreeMap<String, String>,
    ) -> Result<Role> {
        let api: Api<Role> = Api::namespaced(self.inner().clone(), namespace);

        let role = Role {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            rules: Some(rules),
        };

        debug!("creating role");
        let created = api
            .create(&PostParams::default(), &role)
            .await
            .map_err(Error::RbacCreate)?;

        Ok(created)
    }

    #[instrument(skip(self, subjects, labels), fields(namespace = %namespace, binding_name = %name))]
    async fn create_role_binding(
        &self,
        namespace: &str,
        name: &str,
        role_name: &str,
        subjects: Vec<Subject>,
        labels: BTreeMap<String, String>,
    ) -> Result<RoleBinding> {
        let api: Api<RoleBinding> = Api::namespaced(self.inner().clone(), namespace);

        let role_binding = RoleBinding {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "Role".to_string(),
                name: role_name.to_string(),
            },
            subjects: Some(subjects),
        };

        debug!("creating role binding");
        let created = api
            .create(&PostParams::default(), &role_binding)
            .await
            .map_err(Error::RbacCreate)?;

        Ok(created)
    }
}
