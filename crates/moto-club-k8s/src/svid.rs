//! Garage SVID Secret management.
//!
//! Per moto-club.md spec v1.3 step 8, this module handles:
//! - Creating the `garage-svid` Secret with the SVID token from keybox
//!
//! The SVID is issued by moto-club calling keybox's `POST /auth/issue-garage-svid`
//! endpoint, then stored in a K8s Secret that the garage pod mounts.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, ObjectMeta, PostParams};
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::Result;

use crate::GarageK8s;

/// Garage SVID Secret name.
pub const GARAGE_SVID_SECRET_NAME: &str = "garage-svid";

/// Result of creating the garage SVID Secret.
#[derive(Debug, Clone)]
pub struct SvidSecret {
    /// The SPIFFE ID for this garage.
    pub spiffe_id: String,
    /// Token expiration time (Unix timestamp).
    pub expires_at: i64,
}

/// Trait for garage SVID Secret operations.
pub trait GarageSvidOps {
    /// Creates the `garage-svid` Secret with the issued SVID token.
    ///
    /// Per moto-club.md spec v1.3 step 8:
    /// - moto-club calls keybox's `POST /auth/issue-garage-svid`
    /// - Creates `garage-svid` Secret with the returned SVID token
    /// - Garage pod mounts this Secret at `/var/run/secrets/svid`
    ///
    /// The Secret contains:
    /// - `token`: The signed SVID JWT (1 hour TTL)
    /// - `spiffe_id`: The SPIFFE ID for this garage
    /// - `expires_at`: Token expiration timestamp
    ///
    /// # Errors
    ///
    /// Returns an error if Secret creation fails.
    fn create_garage_svid_secret(
        &self,
        id: &GarageId,
        token: &str,
        spiffe_id: &str,
        expires_at: i64,
    ) -> impl Future<Output = Result<SvidSecret>> + Send;
}

impl GarageSvidOps for GarageK8s {
    #[instrument(skip(self, token), fields(garage_id = %id, spiffe_id = %spiffe_id))]
    async fn create_garage_svid_secret(
        &self,
        id: &GarageId,
        token: &str,
        spiffe_id: &str,
        expires_at: i64,
    ) -> Result<SvidSecret> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(
            namespace = %namespace,
            spiffe_id = %spiffe_id,
            expires_at = expires_at,
            "creating garage SVID secret"
        );

        let secret = build_garage_svid_secret(&namespace, token, spiffe_id, expires_at);
        let secret_api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);
        secret_api
            .create(&PostParams::default(), &secret)
            .await
            .map_err(moto_k8s::Error::SecretCreate)?;

        debug!(namespace = %namespace, "garage-svid Secret created");

        Ok(SvidSecret {
            spiffe_id: spiffe_id.to_string(),
            expires_at,
        })
    }
}

/// Builds the garage SVID Secret.
///
/// Contains:
/// - `token`: The signed SVID JWT
/// - `spiffe_id`: The SPIFFE ID for this garage
/// - `expires_at`: Token expiration timestamp (as string)
fn build_garage_svid_secret(
    namespace: &str,
    token: &str,
    spiffe_id: &str,
    expires_at: i64,
) -> Secret {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "garage".to_string());
    labels.insert("moto.dev/component".to_string(), "svid".to_string());

    let mut string_data = BTreeMap::new();
    string_data.insert("token".to_string(), token.to_string());
    string_data.insert("spiffe_id".to_string(), spiffe_id.to_string());
    string_data.insert("expires_at".to_string(), expires_at.to_string());

    Secret {
        metadata: ObjectMeta {
            name: Some(GARAGE_SVID_SECRET_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: Some("Opaque".to_string()),
        string_data: Some(string_data),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_garage_svid_secret_structure() {
        let secret = build_garage_svid_secret(
            "moto-garage-abc12345",
            "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9...",
            "spiffe://moto.local/garage/abc123",
            1_700_003_600,
        );

        // Check metadata
        assert_eq!(
            secret.metadata.name,
            Some(GARAGE_SVID_SECRET_NAME.to_string())
        );
        assert_eq!(
            secret.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );
        assert_eq!(secret.type_, Some("Opaque".to_string()));

        // Check labels
        let labels = secret.metadata.labels.as_ref().unwrap();
        assert_eq!(labels.get("app"), Some(&"garage".to_string()));
        assert_eq!(labels.get("moto.dev/component"), Some(&"svid".to_string()));

        // Check string_data
        let data = secret.string_data.as_ref().unwrap();
        assert_eq!(
            data.get("token"),
            Some(&"eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9...".to_string())
        );
        assert_eq!(
            data.get("spiffe_id"),
            Some(&"spiffe://moto.local/garage/abc123".to_string())
        );
        assert_eq!(data.get("expires_at"), Some(&"1700003600".to_string()));
    }

    #[test]
    fn svid_secret_result_contains_correct_fields() {
        let result = SvidSecret {
            spiffe_id: "spiffe://moto.local/garage/abc123".to_string(),
            expires_at: 1_700_003_600,
        };

        assert_eq!(result.spiffe_id, "spiffe://moto.local/garage/abc123");
        assert_eq!(result.expires_at, 1_700_003_600);
    }

    #[test]
    fn garage_svid_secret_name_is_correct() {
        assert_eq!(GARAGE_SVID_SECRET_NAME, "garage-svid");
    }
}
