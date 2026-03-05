//! Kubernetes `ServiceAccount` token validation via `TokenReview` API.
//!
//! This module provides token validation for K8s `ServiceAccount` tokens,
//! used to authenticate garage pods calling moto-club API endpoints.

use std::future::Future;

use k8s_openapi::api::authentication::v1::{TokenReview, TokenReviewSpec, UserInfo};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Validated token information extracted from a successful `TokenReview`.
#[derive(Debug, Clone)]
pub struct ValidatedToken {
    /// The username from the token (e.g., `system:serviceaccount:namespace:name`).
    pub username: String,
    /// The namespace of the service account.
    pub namespace: Option<String>,
    /// Groups the service account belongs to.
    pub groups: Vec<String>,
}

impl ValidatedToken {
    /// Extracts the service account namespace from the username.
    ///
    /// Service account usernames follow the format:
    /// `system:serviceaccount:<namespace>:<name>`
    ///
    /// Returns `None` if the username doesn't match this format.
    #[must_use]
    pub fn service_account_namespace(&self) -> Option<&str> {
        let parts: Vec<&str> = self.username.split(':').collect();
        if parts.len() >= 3 && parts[0] == "system" && parts[1] == "serviceaccount" {
            Some(parts[2])
        } else {
            None
        }
    }

    /// Extracts the service account name from the username.
    ///
    /// Service account usernames follow the format:
    /// `system:serviceaccount:<namespace>:<name>`
    ///
    /// Returns `None` if the username doesn't match this format.
    #[must_use]
    pub fn service_account_name(&self) -> Option<&str> {
        let parts: Vec<&str> = self.username.split(':').collect();
        if parts.len() >= 4 && parts[0] == "system" && parts[1] == "serviceaccount" {
            Some(parts[3])
        } else {
            None
        }
    }

    /// Checks if this token belongs to a service account in the given namespace.
    #[must_use]
    pub fn is_in_namespace(&self, expected_namespace: &str) -> bool {
        self.service_account_namespace()
            .is_some_and(|ns| ns == expected_namespace)
    }
}

/// Operations for validating K8s `ServiceAccount` tokens.
pub trait TokenReviewOps {
    /// Validates a `ServiceAccount` token using the K8s `TokenReview` API.
    ///
    /// # Arguments
    ///
    /// * `token` - The bearer token to validate
    ///
    /// # Returns
    ///
    /// Returns `ValidatedToken` if the token is valid, or an error if:
    /// - The token is invalid or expired
    /// - The K8s API call fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// use moto_k8s::{K8sClient, TokenReviewOps};
    ///
    /// let client = K8sClient::new().await?;
    /// let token = "eyJhbGciOiJSUzI1NiIs...";
    /// let validated = client.validate_token(token).await?;
    /// println!("Token belongs to: {}", validated.username);
    /// ```
    fn validate_token(&self, token: &str) -> impl Future<Output = Result<ValidatedToken>> + Send;
}

impl TokenReviewOps for K8sClient {
    #[instrument(skip(self, token))]
    async fn validate_token(&self, token: &str) -> Result<ValidatedToken> {
        let client = self.inner().clone();

        // Create the TokenReview request
        let token_review = TokenReview {
            metadata: ObjectMeta::default(),
            spec: TokenReviewSpec {
                token: Some(token.to_string()),
                ..TokenReviewSpec::default()
            },
            status: None,
        };

        debug!("submitting token review request");

        // Submit the TokenReview
        let api: kube::Api<TokenReview> = kube::Api::all(client);
        let result = api
            .create(&PostParams::default(), &token_review)
            .await
            .map_err(Error::TokenReview)?;

        // Check if the token was authenticated
        let status = result.status.ok_or(Error::TokenNotAuthenticated)?;

        if !status.authenticated.unwrap_or(false) {
            debug!("token not authenticated");
            return Err(Error::TokenNotAuthenticated);
        }

        // Extract user info from the status
        let user_info = status.user.ok_or(Error::TokenNotAuthenticated)?;

        debug!(username = %user_info.username.as_deref().unwrap_or("unknown"), "token validated");

        // Extract namespace from extra fields before consuming user_info
        let namespace = extract_namespace_from_user_info(&user_info);

        Ok(ValidatedToken {
            username: user_info.username.unwrap_or_default(),
            namespace,
            groups: user_info.groups.unwrap_or_default(),
        })
    }
}

/// Extracts the namespace from `UserInfo` extra fields if present.
fn extract_namespace_from_user_info(user_info: &UserInfo) -> Option<String> {
    // The namespace might be in the extra fields under various keys
    if let Some(extra) = &user_info.extra {
        // Try common keys for namespace
        for key in &["authentication.kubernetes.io/pod-namespace", "namespace"] {
            if let Some(values) = extra.get(*key)
                && let Some(ns) = values.first()
            {
                return Some(ns.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_token_service_account_namespace() {
        let token = ValidatedToken {
            username: "system:serviceaccount:moto-garage-abc123:default".to_string(),
            namespace: None,
            groups: vec![],
        };

        assert_eq!(
            token.service_account_namespace(),
            Some("moto-garage-abc123")
        );
        assert_eq!(token.service_account_name(), Some("default"));
    }

    #[test]
    fn validated_token_is_in_namespace() {
        let token = ValidatedToken {
            username: "system:serviceaccount:moto-garage-abc123:default".to_string(),
            namespace: None,
            groups: vec![],
        };

        assert!(token.is_in_namespace("moto-garage-abc123"));
        assert!(!token.is_in_namespace("moto-garage-other"));
    }

    #[test]
    fn validated_token_non_service_account() {
        let token = ValidatedToken {
            username: "admin".to_string(),
            namespace: None,
            groups: vec![],
        };

        assert!(token.service_account_namespace().is_none());
        assert!(token.service_account_name().is_none());
    }

    #[test]
    fn validated_token_short_username() {
        let token = ValidatedToken {
            username: "system:serviceaccount".to_string(),
            namespace: None,
            groups: vec![],
        };

        assert!(token.service_account_namespace().is_none());
        assert!(token.service_account_name().is_none());
    }
}
