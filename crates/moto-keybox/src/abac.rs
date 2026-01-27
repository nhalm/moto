//! Attribute-Based Access Control (ABAC) policy engine.
//!
//! This module evaluates access requests against hardcoded policies.
//! Future: load policies from configuration.

use crate::svid::{PrincipalType, SvidClaims};

/// Secret scope levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Platform-wide secrets (e.g., AI API keys).
    Global,
    /// Per-service secrets (e.g., database passwords).
    Service,
    /// Per-instance secrets (e.g., per-garage dev credentials).
    Instance,
}

impl Scope {
    /// Parse scope from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "global" => Some(Self::Global),
            "service" => Some(Self::Service),
            "instance" => Some(Self::Instance),
            _ => None,
        }
    }

    /// Convert to string.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Service => "service",
            Self::Instance => "instance",
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resource being accessed.
pub struct Resource<'a> {
    /// Secret scope.
    pub scope: Scope,
    /// Service name (for service/instance scoped secrets).
    pub service: Option<&'a str>,
    /// Instance ID (for instance scoped secrets).
    pub instance_id: Option<&'a str>,
    /// Secret name.
    pub name: &'a str,
}

/// Access decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Access allowed.
    Allow,
    /// Access denied with reason.
    Deny(String),
}

impl Decision {
    /// Check if access is allowed.
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// ABAC policy engine.
///
/// Policies are hardcoded for MVP:
/// - Garages can access their own instance secrets
/// - Bikes can access their own instance and service secrets
/// - Services can access global secrets with specific prefixes
#[derive(Clone, Copy)]
pub struct PolicyEngine;

impl PolicyEngine {
    /// Create a new policy engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Evaluate access request.
    ///
    /// Returns `Allow` if the principal can access the resource, `Deny` otherwise.
    #[must_use]
    pub fn evaluate(&self, principal: &SvidClaims, resource: &Resource<'_>) -> Decision {
        match principal.principal_type {
            PrincipalType::Garage => self.evaluate_garage(principal, resource),
            PrincipalType::Bike => self.evaluate_bike(principal, resource),
            PrincipalType::Service => self.evaluate_service(principal, resource),
        }
    }

    /// Evaluate access for a garage.
    ///
    /// Garages can access:
    /// - Their own instance-scoped secrets
    /// - Global AI keys (for development)
    fn evaluate_garage(&self, principal: &SvidClaims, resource: &Resource<'_>) -> Decision {
        match resource.scope {
            Scope::Instance => {
                // Garage can only access secrets for its own instance
                if resource.instance_id == Some(principal.principal_id.as_str()) {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "garage {} cannot access instance secrets for {}",
                        principal.principal_id,
                        resource.instance_id.unwrap_or("unknown")
                    ))
                }
            }
            Scope::Global => {
                // Garages can access global AI keys for development
                if resource.name.starts_with("ai/") {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "garage {} cannot access global secret {}",
                        principal.principal_id, resource.name
                    ))
                }
            }
            Scope::Service => {
                // Garages cannot access service-scoped secrets directly
                Decision::Deny(format!(
                    "garage {} cannot access service-scoped secrets",
                    principal.principal_id
                ))
            }
        }
    }

    /// Evaluate access for a bike.
    ///
    /// Bikes can access:
    /// - Their own instance-scoped secrets
    /// - Service secrets for their associated service
    fn evaluate_bike(&self, principal: &SvidClaims, resource: &Resource<'_>) -> Decision {
        match resource.scope {
            Scope::Instance => {
                // Bike can only access secrets for its own instance
                if resource.instance_id == Some(principal.principal_id.as_str()) {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "bike {} cannot access instance secrets for {}",
                        principal.principal_id,
                        resource.instance_id.unwrap_or("unknown")
                    ))
                }
            }
            Scope::Service => {
                // Bike can access service secrets if the service matches
                match (&principal.service, resource.service) {
                    (Some(bike_service), Some(secret_service))
                        if bike_service == secret_service =>
                    {
                        Decision::Allow
                    }
                    _ => Decision::Deny(format!(
                        "bike {} (service: {:?}) cannot access service secrets for {:?}",
                        principal.principal_id, principal.service, resource.service
                    )),
                }
            }
            Scope::Global => {
                // Bikes cannot access global secrets directly
                Decision::Deny(format!(
                    "bike {} cannot access global secrets",
                    principal.principal_id
                ))
            }
        }
    }

    /// Evaluate access for a service.
    ///
    /// Services can access:
    /// - Global secrets with allowed prefixes (based on service name)
    fn evaluate_service(&self, principal: &SvidClaims, resource: &Resource<'_>) -> Decision {
        match resource.scope {
            Scope::Global => {
                // Service-specific access rules
                let allowed = match principal.principal_id.as_str() {
                    "ai-proxy" => resource.name.starts_with("ai/"),
                    "moto-club" => true, // moto-club has broad access
                    _ => false,
                };

                if allowed {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "service {} cannot access global secret {}",
                        principal.principal_id, resource.name
                    ))
                }
            }
            Scope::Service | Scope::Instance => Decision::Deny(format!(
                "service {} cannot access {} secrets",
                principal.principal_id, resource.scope
            )),
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_claims(
        principal_type: PrincipalType,
        principal_id: &str,
        service: Option<&str>,
    ) -> SvidClaims {
        SvidClaims {
            sub: SvidClaims::spiffe_id(principal_type, principal_id),
            iss: "moto-keybox".to_string(),
            iat: Utc::now().timestamp(),
            exp: Utc::now().timestamp() + 900,
            principal_type,
            principal_id: principal_id.to_string(),
            pod_uid: None,
            pod_namespace: None,
            pod_name: None,
            service: service.map(String::from),
        }
    }

    #[test]
    fn garage_can_access_own_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Garage, "garage-123", None);

        let resource = Resource {
            scope: Scope::Instance,
            service: None,
            instance_id: Some("garage-123"),
            name: "dev/github-token",
        };

        assert!(engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn garage_cannot_access_other_instance_secrets() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Garage, "garage-123", None);

        let resource = Resource {
            scope: Scope::Instance,
            service: None,
            instance_id: Some("garage-456"),
            name: "dev/github-token",
        };

        assert!(!engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn garage_can_access_global_ai_keys() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Garage, "garage-123", None);

        let resource = Resource {
            scope: Scope::Global,
            service: None,
            instance_id: None,
            name: "ai/anthropic",
        };

        assert!(engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn bike_can_access_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Bike, "bike-456", Some("tokenization"));

        let resource = Resource {
            scope: Scope::Service,
            service: Some("tokenization"),
            instance_id: None,
            name: "db/password",
        };

        assert!(engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn bike_cannot_access_other_service_secrets() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Bike, "bike-456", Some("tokenization"));

        let resource = Resource {
            scope: Scope::Service,
            service: Some("payments"),
            instance_id: None,
            name: "db/password",
        };

        assert!(!engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn service_ai_proxy_can_access_ai_keys() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Service, "ai-proxy", None);

        let resource = Resource {
            scope: Scope::Global,
            service: None,
            instance_id: None,
            name: "ai/anthropic",
        };

        assert!(engine.evaluate(&claims, &resource).is_allowed());
    }

    #[test]
    fn service_ai_proxy_cannot_access_other_global_secrets() {
        let engine = PolicyEngine::new();
        let claims = make_claims(PrincipalType::Service, "ai-proxy", None);

        let resource = Resource {
            scope: Scope::Global,
            service: None,
            instance_id: None,
            name: "crypto/master-key",
        };

        assert!(!engine.evaluate(&claims, &resource).is_allowed());
    }
}
