//! SSH keys Secret management for garage pods.
//!
//! Provides operations for creating and managing SSH keys secrets
//! that are mounted into garage pods for user authentication.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::ByteString;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, DeleteParams, ObjectMeta, PostParams};
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{Labels, Result};

use crate::GarageK8s;

/// Name of the SSH keys Secret in each garage namespace.
pub const SSH_KEYS_SECRET_NAME: &str = "ssh-keys";

/// Key in the Secret data for the `authorized_keys` file.
pub const AUTHORIZED_KEYS_KEY: &str = "authorized_keys";

/// Input for creating an SSH keys Secret.
#[derive(Debug, Clone)]
pub struct SshKeysSecretInput {
    /// Unique garage identifier.
    pub id: GarageId,
    /// Human-friendly garage name.
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// SSH public keys to include (one per line in `authorized_keys`).
    pub ssh_public_keys: Vec<String>,
}

impl SshKeysSecretInput {
    /// Returns the K8s namespace name for this garage.
    #[must_use]
    pub fn namespace_name(&self) -> String {
        format!("moto-garage-{}", self.id.short())
    }
}

/// Trait for SSH keys Secret operations.
pub trait SshKeysSecretOps {
    /// Creates an SSH keys Secret in the garage namespace.
    ///
    /// The Secret contains:
    /// - `authorized_keys`: concatenated SSH public keys (one per line)
    ///
    /// This Secret is intended to be mounted at `/home/moto/.ssh/authorized_keys`
    /// in the garage pod with mode 0600.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace doesn't exist or Secret creation fails.
    fn create_ssh_keys_secret(
        &self,
        input: &SshKeysSecretInput,
    ) -> impl Future<Output = Result<Secret>> + Send;

    /// Deletes the SSH keys Secret from a garage namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the Secret doesn't exist or deletion fails.
    fn delete_ssh_keys_secret(&self, id: &GarageId) -> impl Future<Output = Result<()>> + Send;

    /// Gets the SSH keys Secret from a garage namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the Secret doesn't exist or the operation fails.
    fn get_ssh_keys_secret(&self, id: &GarageId) -> impl Future<Output = Result<Secret>> + Send;
}

impl SshKeysSecretOps for GarageK8s {
    #[instrument(skip(self, input), fields(garage_id = %input.id, garage_name = %input.name, key_count = input.ssh_public_keys.len()))]
    async fn create_ssh_keys_secret(&self, input: &SshKeysSecretInput) -> Result<Secret> {
        let namespace = input.namespace_name();

        debug!(
            namespace = %namespace,
            key_count = input.ssh_public_keys.len(),
            "creating SSH keys secret"
        );

        let labels = Labels::for_garage(
            &input.id.to_string(),
            &input.name,
            Some(&input.owner),
            None,
            None,
        );

        let secret = build_ssh_keys_secret(&namespace, &input.ssh_public_keys, labels);

        let api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);
        let created = api
            .create(&PostParams::default(), &secret)
            .await
            .map_err(moto_k8s::Error::NamespaceCreate)?;

        debug!(secret = %SSH_KEYS_SECRET_NAME, "SSH keys secret created");
        Ok(created)
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn delete_ssh_keys_secret(&self, id: &GarageId) -> Result<()> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "deleting SSH keys secret");
        let api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);

        api.delete(SSH_KEYS_SECRET_NAME, &DeleteParams::default())
            .await
            .map_err(|e| {
                if is_not_found(&e) {
                    moto_k8s::Error::PodNotFound(format!(
                        "{SSH_KEYS_SECRET_NAME} in namespace {namespace}"
                    ))
                } else {
                    moto_k8s::Error::NamespaceDelete(e)
                }
            })?;

        Ok(())
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn get_ssh_keys_secret(&self, id: &GarageId) -> Result<Secret> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "getting SSH keys secret");
        let api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);

        api.get(SSH_KEYS_SECRET_NAME).await.map_err(|e| {
            if is_not_found(&e) {
                moto_k8s::Error::PodNotFound(format!(
                    "{SSH_KEYS_SECRET_NAME} in namespace {namespace}"
                ))
            } else {
                moto_k8s::Error::NamespaceGet(e)
            }
        })
    }
}

/// Builds an SSH keys Secret spec.
fn build_ssh_keys_secret(
    namespace: &str,
    ssh_public_keys: &[String],
    labels: BTreeMap<String, String>,
) -> Secret {
    // Concatenate all SSH public keys, one per line
    let authorized_keys = ssh_public_keys.join("\n");

    let mut data = BTreeMap::new();
    data.insert(
        AUTHORIZED_KEYS_KEY.to_string(),
        ByteString(authorized_keys.into_bytes()),
    );

    Secret {
        metadata: ObjectMeta {
            name: Some(SSH_KEYS_SECRET_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        type_: Some("Opaque".to_string()),
        ..Default::default()
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

    #[test]
    fn ssh_keys_secret_input_namespace_name() {
        let input = SshKeysSecretInput {
            id: GarageId::new(),
            name: "my-project".to_string(),
            owner: "alice".to_string(),
            ssh_public_keys: vec!["ssh-ed25519 AAAA... alice@host".to_string()],
        };

        let ns = input.namespace_name();
        assert!(ns.starts_with("moto-garage-"));
        assert_eq!(ns.len(), "moto-garage-".len() + 8);
    }

    #[test]
    fn build_secret_has_correct_structure() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let keys = vec![
            "ssh-ed25519 AAAA... alice@host".to_string(),
            "ssh-rsa BBBB... alice@work".to_string(),
        ];
        let secret = build_ssh_keys_secret("moto-garage-abc12345", &keys, labels);

        // Check metadata
        assert_eq!(secret.metadata.name, Some(SSH_KEYS_SECRET_NAME.to_string()));
        assert_eq!(
            secret.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check type
        assert_eq!(secret.type_, Some("Opaque".to_string()));

        // Check data
        let data = secret.data.as_ref().unwrap();
        let authorized_keys = data.get(AUTHORIZED_KEYS_KEY).unwrap();
        let content = String::from_utf8(authorized_keys.0.clone()).unwrap();
        assert_eq!(
            content,
            "ssh-ed25519 AAAA... alice@host\nssh-rsa BBBB... alice@work"
        );
    }

    #[test]
    fn build_secret_with_empty_keys() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let keys: Vec<String> = vec![];
        let secret = build_ssh_keys_secret("moto-garage-abc12345", &keys, labels);

        let data = secret.data.as_ref().unwrap();
        let authorized_keys = data.get(AUTHORIZED_KEYS_KEY).unwrap();
        let content = String::from_utf8(authorized_keys.0.clone()).unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn build_secret_with_single_key() {
        let labels = Labels::for_garage("abc-123", "test", Some("alice"), None, None);
        let keys = vec!["ssh-ed25519 AAAA... alice@host".to_string()];
        let secret = build_ssh_keys_secret("moto-garage-abc12345", &keys, labels);

        let data = secret.data.as_ref().unwrap();
        let authorized_keys = data.get(AUTHORIZED_KEYS_KEY).unwrap();
        let content = String::from_utf8(authorized_keys.0.clone()).unwrap();
        assert_eq!(content, "ssh-ed25519 AAAA... alice@host");
    }
}
