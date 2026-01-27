//! ABAC (Attribute-Based Access Control) policy engine.
//!
//! Evaluates access requests based on principal and resource attributes.
//! Policies are hardcoded in Rust for MVP.
//!
//! # Principal Attributes (from SVID)
//!
//! - `type`: garage | bike | service
//! - `id`: garage-id, bike-id, or service name
//! - `pod_namespace` (optional)
//! - `pod_name` (optional)
//!
//! # Resource Attributes (from secret)
//!
//! - `scope`: global | service | instance
//! - `service`: which service it belongs to (for service-scoped)
//! - `instance_id`: garage-id or bike-id (for instance-scoped)
//! - `name`: secret name/path
//!
//! # Example Policies
//!
//! ```text
//! # Garage can access its own instance secrets
//! principal.type == "garage" AND
//! principal.id == resource.instance_id AND
//! resource.scope == "instance"
//!
//! # Service can access global secrets it's allowed
//! principal.type == "service" AND
//! principal.id == "ai-proxy" AND
//! resource.scope == "global" AND
//! resource.name STARTS_WITH "ai/"
//! ```

use crate::svid::SvidClaims;
use crate::types::{PrincipalType, Scope, SecretMetadata};
use crate::{Error, Result};

/// Action being requested on a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read a secret value.
    Read,
    /// Write (create or update) a secret.
    Write,
    /// Delete a secret.
    Delete,
    /// List secrets in a scope.
    List,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Delete => write!(f, "delete"),
            Self::List => write!(f, "list"),
        }
    }
}

/// ABAC policy engine for evaluating access requests.
///
/// Contains hardcoded rules for MVP. Future versions may load rules
/// from configuration.
#[derive(Debug, Clone, Default)]
pub struct PolicyEngine {
    /// Service tokens that bypass normal ABAC checks.
    /// Used for moto-club admin access.
    admin_service_ids: Vec<String>,
}

impl PolicyEngine {
    /// Creates a new policy engine with default rules.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an admin service ID that bypasses normal checks.
    ///
    /// Admin services can read/write/delete any secret.
    #[must_use]
    pub fn with_admin_service(mut self, service_id: impl Into<String>) -> Self {
        self.admin_service_ids.push(service_id.into());
        self
    }

    /// Evaluates an access request and returns Ok if allowed.
    ///
    /// # Errors
    ///
    /// Returns `Error::AccessDenied` if the policy denies the request.
    pub fn evaluate(
        &self,
        claims: &SvidClaims,
        secret: &SecretMetadata,
        action: Action,
    ) -> Result<()> {
        // Check admin bypass first
        if self.is_admin(claims) {
            return Ok(());
        }

        // Evaluate based on secret scope
        match secret.scope {
            Scope::Global => self.evaluate_global(claims, secret, action),
            Scope::Service => self.evaluate_service(claims, secret, action),
            Scope::Instance => self.evaluate_instance(claims, secret, action),
        }
    }

    /// Evaluates read-only access (convenience method for most common case).
    ///
    /// # Errors
    ///
    /// Returns `Error::AccessDenied` if the policy denies the read request.
    pub fn can_read(&self, claims: &SvidClaims, secret: &SecretMetadata) -> Result<()> {
        self.evaluate(claims, secret, Action::Read)
    }

    /// Checks if the principal is an admin service.
    fn is_admin(&self, claims: &SvidClaims) -> bool {
        claims.principal_type == PrincipalType::Service
            && self.admin_service_ids.contains(&claims.principal_id)
    }

    /// Evaluates access to global secrets.
    ///
    /// Rules:
    /// - Services can access global secrets with matching name prefixes
    /// - Garages and bikes can read global secrets (read-only)
    fn evaluate_global(
        &self,
        claims: &SvidClaims,
        secret: &SecretMetadata,
        action: Action,
    ) -> Result<()> {
        match claims.principal_type {
            PrincipalType::Service => {
                // Services can access global secrets if the name prefix matches
                // e.g., "ai-proxy" can access "ai/*"
                let allowed_prefix = format!("{}/", claims.principal_id);
                if secret.name.starts_with(&allowed_prefix)
                    || secret.name.starts_with(&claims.principal_id)
                {
                    return Ok(());
                }
                Err(Error::AccessDenied {
                    message: format!(
                        "service '{}' cannot access global secret '{}'",
                        claims.principal_id, secret.name
                    ),
                })
            }
            PrincipalType::Garage | PrincipalType::Bike => {
                // Garages and bikes can only read global secrets
                if action == Action::Read {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "{} '{}' can only read global secrets, not {}",
                            claims.principal_type, claims.principal_id, action
                        ),
                    })
                }
            }
        }
    }

    /// Evaluates access to service-scoped secrets.
    ///
    /// Rules:
    /// - Bikes can read secrets belonging to their service
    /// - Services can access their own service secrets
    fn evaluate_service(
        &self,
        claims: &SvidClaims,
        secret: &SecretMetadata,
        action: Action,
    ) -> Result<()> {
        let secret_service = secret.service.as_deref().unwrap_or("");

        match claims.principal_type {
            PrincipalType::Bike => {
                // Bikes can read secrets of the service they belong to
                // The service is determined by the secret's service field
                // For MVP, bikes can read any service secret (they're trusted)
                if action == Action::Read {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "bike '{}' can only read service secrets, not {}",
                            claims.principal_id, action
                        ),
                    })
                }
            }
            PrincipalType::Service => {
                // Services can access their own service secrets
                if claims.principal_id == secret_service {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "service '{}' cannot access service '{}' secrets",
                            claims.principal_id, secret_service
                        ),
                    })
                }
            }
            PrincipalType::Garage => {
                // Garages can read service secrets for development
                if action == Action::Read {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "garage '{}' can only read service secrets, not {}",
                            claims.principal_id, action
                        ),
                    })
                }
            }
        }
    }

    /// Evaluates access to instance-scoped secrets.
    ///
    /// Rules:
    /// - Garages can access their own instance secrets
    /// - Bikes can access their own instance secrets
    /// - Services cannot access instance secrets (use service scope instead)
    fn evaluate_instance(
        &self,
        claims: &SvidClaims,
        secret: &SecretMetadata,
        _action: Action,
    ) -> Result<()> {
        let instance_id = secret.instance_id.as_deref().unwrap_or("");

        match claims.principal_type {
            PrincipalType::Garage => {
                // Garages can access their own instance secrets
                if claims.principal_id == instance_id {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "garage '{}' cannot access instance secrets for '{}'",
                            claims.principal_id, instance_id
                        ),
                    })
                }
            }
            PrincipalType::Bike => {
                // Bikes can access their own instance secrets
                if claims.principal_id == instance_id {
                    Ok(())
                } else {
                    Err(Error::AccessDenied {
                        message: format!(
                            "bike '{}' cannot access instance secrets for '{}'",
                            claims.principal_id, instance_id
                        ),
                    })
                }
            }
            PrincipalType::Service => {
                // Services should use service-scoped secrets, not instance
                Err(Error::AccessDenied {
                    message: format!(
                        "service '{}' cannot access instance-scoped secrets",
                        claims.principal_id
                    ),
                })
            }
        }
    }
}

/// Builder for access request evaluation.
///
/// Provides a fluent API for evaluating access requests.
#[derive(Debug)]
pub struct AccessRequest<'a> {
    claims: &'a SvidClaims,
    secret: Option<&'a SecretMetadata>,
    action: Action,
}

impl<'a> AccessRequest<'a> {
    /// Creates a new access request for a principal.
    #[must_use]
    pub fn new(claims: &'a SvidClaims) -> Self {
        Self {
            claims,
            secret: None,
            action: Action::Read,
        }
    }

    /// Sets the secret being accessed.
    #[must_use]
    pub const fn secret(mut self, secret: &'a SecretMetadata) -> Self {
        self.secret = Some(secret);
        self
    }

    /// Sets the action being performed.
    #[must_use]
    pub const fn action(mut self, action: Action) -> Self {
        self.action = action;
        self
    }

    /// Evaluates the request against the policy engine.
    ///
    /// # Errors
    ///
    /// Returns `Error::AccessDenied` if the policy denies the request.
    ///
    /// # Panics
    ///
    /// Panics if no secret was set.
    pub fn evaluate(self, engine: &PolicyEngine) -> Result<()> {
        let secret = self
            .secret
            .expect("AccessRequest::secret() must be called before evaluate()");
        engine.evaluate(self.claims, secret, self.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svid::DEFAULT_SVID_TTL_SECS;
    use crate::types::SpiffeId;

    fn garage_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::garage(id), DEFAULT_SVID_TTL_SECS)
    }

    fn bike_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::bike(id), DEFAULT_SVID_TTL_SECS)
    }

    fn service_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::service(id), DEFAULT_SVID_TTL_SECS)
    }

    #[test]
    fn garage_can_read_own_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::instance("garage-123", "dev/token");

        engine.can_read(&claims, &secret).unwrap();
    }

    #[test]
    fn garage_can_write_own_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::instance("garage-123", "dev/token");

        engine.evaluate(&claims, &secret, Action::Write).unwrap();
    }

    #[test]
    fn garage_cannot_access_other_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::instance("garage-456", "dev/token");

        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn garage_can_read_global_secrets() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::global("ai/anthropic");

        engine.can_read(&claims, &secret).unwrap();
    }

    #[test]
    fn garage_cannot_write_global_secrets() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::global("ai/anthropic");

        let err = engine
            .evaluate(&claims, &secret, Action::Write)
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn bike_can_read_own_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = bike_claims("bike-abc");
        let secret = SecretMetadata::instance("bike-abc", "auth/token");

        engine.can_read(&claims, &secret).unwrap();
    }

    #[test]
    fn bike_cannot_access_other_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = bike_claims("bike-abc");
        let secret = SecretMetadata::instance("bike-xyz", "auth/token");

        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn bike_can_read_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = bike_claims("bike-abc");
        let secret = SecretMetadata::service("tokenization", "db/password");

        engine.can_read(&claims, &secret).unwrap();
    }

    #[test]
    fn bike_cannot_write_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = bike_claims("bike-abc");
        let secret = SecretMetadata::service("tokenization", "db/password");

        let err = engine
            .evaluate(&claims, &secret, Action::Write)
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn service_can_access_own_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = service_claims("tokenization");
        let secret = SecretMetadata::service("tokenization", "db/password");

        engine.can_read(&claims, &secret).unwrap();
        engine.evaluate(&claims, &secret, Action::Write).unwrap();
    }

    #[test]
    fn service_cannot_access_other_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = service_claims("tokenization");
        let secret = SecretMetadata::service("other-service", "db/password");

        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn service_can_access_matching_global_secrets() {
        let engine = PolicyEngine::new();
        let claims = service_claims("ai-proxy");
        let secret = SecretMetadata::global("ai-proxy/anthropic");

        engine.can_read(&claims, &secret).unwrap();
    }

    #[test]
    fn service_cannot_access_unrelated_global_secrets() {
        let engine = PolicyEngine::new();
        let claims = service_claims("ai-proxy");
        let secret = SecretMetadata::global("db/master-password");

        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn service_cannot_access_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = service_claims("some-service");
        let secret = SecretMetadata::instance("garage-123", "dev/token");

        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn admin_service_bypasses_all_checks() {
        let engine = PolicyEngine::new().with_admin_service("moto-club");
        let claims = service_claims("moto-club");

        // Can access any global secret
        let global_secret = SecretMetadata::global("anything/secret");
        engine.can_read(&claims, &global_secret).unwrap();
        engine
            .evaluate(&claims, &global_secret, Action::Write)
            .unwrap();
        engine
            .evaluate(&claims, &global_secret, Action::Delete)
            .unwrap();

        // Can access any service secret
        let service_secret = SecretMetadata::service("other", "secret");
        engine.can_read(&claims, &service_secret).unwrap();

        // Can access any instance secret
        let instance_secret = SecretMetadata::instance("someone-else", "secret");
        engine.can_read(&claims, &instance_secret).unwrap();
    }

    #[test]
    fn non_admin_service_has_normal_restrictions() {
        let engine = PolicyEngine::new().with_admin_service("moto-club");
        let claims = service_claims("other-service");

        let secret = SecretMetadata::instance("garage-123", "dev/token");
        let err = engine.can_read(&claims, &secret).unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn access_request_fluent_api() {
        let engine = PolicyEngine::new();
        let claims = garage_claims("garage-123");
        let secret = SecretMetadata::instance("garage-123", "dev/token");

        AccessRequest::new(&claims)
            .secret(&secret)
            .action(Action::Read)
            .evaluate(&engine)
            .unwrap();
    }

    #[test]
    fn action_display() {
        assert_eq!(Action::Read.to_string(), "read");
        assert_eq!(Action::Write.to_string(), "write");
        assert_eq!(Action::Delete.to_string(), "delete");
        assert_eq!(Action::List.to_string(), "list");
    }
}
