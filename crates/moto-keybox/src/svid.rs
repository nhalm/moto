//! SVID (SPIFFE-inspired Verifiable Identity Document) generation and validation.
//!
//! SVIDs are short-lived JWTs that identify garages, bikes, and services.
//! They are signed by the keybox server using Ed25519.

use chrono::{Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// SVID errors.
#[derive(Debug, Error)]
pub enum SvidError {
    /// JWT encoding failed.
    #[error("JWT encoding failed: {0}")]
    EncodingFailed(#[from] jsonwebtoken::errors::Error),

    /// JWT decoding/validation failed.
    #[error("JWT validation failed: {0}")]
    ValidationFailed(String),

    /// SVID has expired.
    #[error("SVID has expired")]
    Expired,

    /// Invalid SPIFFE ID format.
    #[error("invalid SPIFFE ID format: {0}")]
    InvalidSpiffeId(String),

    /// Signature verification failed.
    #[error("signature verification failed")]
    SignatureInvalid,
}

/// Result type for SVID operations.
pub type SvidResult<T> = Result<T, SvidError>;

/// Principal type in a SPIFFE ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalType {
    /// A development garage.
    Garage,
    /// A bike (production workload).
    Bike,
    /// An internal service.
    Service,
}

impl std::fmt::Display for PrincipalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Garage => write!(f, "garage"),
            Self::Bike => write!(f, "bike"),
            Self::Service => write!(f, "service"),
        }
    }
}

/// SVID claims (JWT payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvidClaims {
    /// SPIFFE ID (e.g., "spiffe://moto.local/garage/abc123").
    pub sub: String,
    /// Issuer (always "moto-keybox").
    pub iss: String,
    /// Issued at (Unix timestamp).
    pub iat: i64,
    /// Expiration (Unix timestamp).
    pub exp: i64,
    /// Principal type.
    pub principal_type: PrincipalType,
    /// Principal ID (garage-id, bike-id, or service name).
    pub principal_id: String,
    /// Pod UID (for replay prevention).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_uid: Option<String>,
    /// Pod namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_namespace: Option<String>,
    /// Pod name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,
    /// Associated service (for bikes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

impl SvidClaims {
    /// Create a SPIFFE ID from the principal type and ID.
    pub fn spiffe_id(principal_type: PrincipalType, principal_id: &str) -> String {
        format!("spiffe://moto.local/{principal_type}/{principal_id}")
    }

    /// Parse principal type and ID from a SPIFFE ID.
    pub fn parse_spiffe_id(spiffe_id: &str) -> SvidResult<(PrincipalType, String)> {
        let prefix = "spiffe://moto.local/";
        if !spiffe_id.starts_with(prefix) {
            return Err(SvidError::InvalidSpiffeId(spiffe_id.to_string()));
        }

        let rest = &spiffe_id[prefix.len()..];
        let (type_str, id) = rest
            .split_once('/')
            .ok_or_else(|| SvidError::InvalidSpiffeId(spiffe_id.to_string()))?;

        let principal_type = match type_str {
            "garage" => PrincipalType::Garage,
            "bike" => PrincipalType::Bike,
            "service" => PrincipalType::Service,
            _ => return Err(SvidError::InvalidSpiffeId(spiffe_id.to_string())),
        };

        Ok((principal_type, id.to_string()))
    }

    /// Check if the SVID is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() > self.exp
    }
}

/// Input for issuing a new SVID.
pub struct IssueSvidInput {
    /// Principal type.
    pub principal_type: PrincipalType,
    /// Principal ID.
    pub principal_id: String,
    /// Pod UID (optional, for K8s pods).
    pub pod_uid: Option<String>,
    /// Pod namespace (optional).
    pub pod_namespace: Option<String>,
    /// Pod name (optional).
    pub pod_name: Option<String>,
    /// Associated service (optional, for bikes).
    pub service: Option<String>,
}

/// SVID issuer.
#[derive(Clone)]
pub struct SvidIssuer {
    /// TTL for issued SVIDs.
    ttl: std::time::Duration,
}

impl SvidIssuer {
    /// Create a new SVID issuer with the given TTL.
    #[must_use]
    pub const fn new(ttl: std::time::Duration) -> Self {
        Self { ttl }
    }

    /// Issue a new SVID.
    pub fn issue(&self, signing_key: &SigningKey, input: IssueSvidInput) -> SvidResult<String> {
        let now = Utc::now();
        #[allow(clippy::cast_possible_wrap)]
        let ttl_secs = self.ttl.as_secs() as i64;
        let exp = now + Duration::seconds(ttl_secs);

        let claims = SvidClaims {
            sub: SvidClaims::spiffe_id(input.principal_type, &input.principal_id),
            iss: "moto-keybox".to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            principal_type: input.principal_type,
            principal_id: input.principal_id,
            pod_uid: input.pod_uid,
            pod_namespace: input.pod_namespace,
            pod_name: input.pod_name,
            service: input.service,
        };

        // Encode claims as JSON, then sign with Ed25519
        let claims_json = serde_json::to_string(&claims).map_err(|e| {
            SvidError::EncodingFailed(jsonwebtoken::errors::Error::from(
                jsonwebtoken::errors::ErrorKind::Json(e.into()),
            ))
        })?;

        // Create JWT-like structure: base64(header).base64(payload).base64(signature)
        let header = r#"{"alg":"EdDSA","typ":"JWT"}"#;
        let header_b64 = base64_url_encode(header.as_bytes());
        let payload_b64 = base64_url_encode(claims_json.as_bytes());

        let signing_input = format!("{header_b64}.{payload_b64}");
        let signature: Signature = signing_key.sign(signing_input.as_bytes());
        let signature_b64 = base64_url_encode(&signature.to_bytes());

        Ok(format!("{signing_input}.{signature_b64}"))
    }

    /// Validate an SVID and extract claims.
    pub fn validate(&self, verifying_key: &VerifyingKey, token: &str) -> SvidResult<SvidClaims> {
        // Parse JWT structure
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(SvidError::ValidationFailed(
                "invalid JWT structure".to_string(),
            ));
        }

        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let signature_bytes = base64_url_decode(parts[2])?;
        let signature =
            Signature::from_slice(&signature_bytes).map_err(|_| SvidError::SignatureInvalid)?;

        // Verify signature
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| SvidError::SignatureInvalid)?;

        // Decode and parse claims
        let claims_bytes = base64_url_decode(parts[1])?;
        let claims: SvidClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|e| SvidError::ValidationFailed(e.to_string()))?;

        // Check expiration
        if claims.is_expired() {
            return Err(SvidError::Expired);
        }

        Ok(claims)
    }
}

/// Issue a dev SVID (long-lived, for local development).
pub fn issue_dev_svid(
    signing_key: &SigningKey,
    principal_type: PrincipalType,
    principal_id: &str,
) -> SvidResult<String> {
    // Dev SVIDs have 24-hour TTL
    let issuer = SvidIssuer::new(std::time::Duration::from_secs(24 * 60 * 60));

    issuer.issue(
        signing_key,
        IssueSvidInput {
            principal_type,
            principal_id: principal_id.to_string(),
            pod_uid: None,
            pod_namespace: None,
            pod_name: None,
            service: None,
        },
    )
}

/// Base64 URL-safe encoding without padding.
fn base64_url_encode(data: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(data)
}

/// Base64 URL-safe decoding.
fn base64_url_decode(data: &str) -> SvidResult<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD
        .decode(data)
        .map_err(|e| SvidError::ValidationFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{OsRng, rand_core::RngCore};

    fn generate_signing_key() -> SigningKey {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        SigningKey::from_bytes(&seed)
    }

    #[test]
    fn spiffe_id_generation() {
        let id = SvidClaims::spiffe_id(PrincipalType::Garage, "abc123");
        assert_eq!(id, "spiffe://moto.local/garage/abc123");

        let id = SvidClaims::spiffe_id(PrincipalType::Bike, "bike-456");
        assert_eq!(id, "spiffe://moto.local/bike/bike-456");

        let id = SvidClaims::spiffe_id(PrincipalType::Service, "ai-proxy");
        assert_eq!(id, "spiffe://moto.local/service/ai-proxy");
    }

    #[test]
    fn spiffe_id_parsing() {
        let (ptype, pid) =
            SvidClaims::parse_spiffe_id("spiffe://moto.local/garage/abc123").unwrap();
        assert_eq!(ptype, PrincipalType::Garage);
        assert_eq!(pid, "abc123");

        let (ptype, pid) =
            SvidClaims::parse_spiffe_id("spiffe://moto.local/bike/bike-456").unwrap();
        assert_eq!(ptype, PrincipalType::Bike);
        assert_eq!(pid, "bike-456");
    }

    #[test]
    fn issue_and_validate_svid() {
        let signing_key = generate_signing_key();
        let verifying_key = signing_key.verifying_key();
        let issuer = SvidIssuer::new(std::time::Duration::from_secs(900));

        let input = IssueSvidInput {
            principal_type: PrincipalType::Garage,
            principal_id: "test-garage".to_string(),
            pod_uid: Some("pod-12345".to_string()),
            pod_namespace: Some("moto-garage-test".to_string()),
            pod_name: Some("dev-container".to_string()),
            service: None,
        };

        let token = issuer.issue(&signing_key, input).unwrap();
        let claims = issuer.validate(&verifying_key, &token).unwrap();

        assert_eq!(claims.principal_type, PrincipalType::Garage);
        assert_eq!(claims.principal_id, "test-garage");
        assert_eq!(claims.pod_uid, Some("pod-12345".to_string()));
    }

    #[test]
    fn invalid_signature_rejected() {
        let signing_key = generate_signing_key();
        let other_key = generate_signing_key();
        let verifying_key = other_key.verifying_key();
        let issuer = SvidIssuer::new(std::time::Duration::from_secs(900));

        let input = IssueSvidInput {
            principal_type: PrincipalType::Garage,
            principal_id: "test".to_string(),
            pod_uid: None,
            pod_namespace: None,
            pod_name: None,
            service: None,
        };

        let token = issuer.issue(&signing_key, input).unwrap();
        let result = issuer.validate(&verifying_key, &token);

        assert!(matches!(result, Err(SvidError::SignatureInvalid)));
    }

    #[test]
    fn dev_svid_issue() {
        let signing_key = generate_signing_key();
        let verifying_key = signing_key.verifying_key();

        let token = issue_dev_svid(&signing_key, PrincipalType::Garage, "local-garage").unwrap();

        let issuer = SvidIssuer::new(std::time::Duration::from_secs(24 * 60 * 60));
        let claims = issuer.validate(&verifying_key, &token).unwrap();

        assert_eq!(claims.principal_type, PrincipalType::Garage);
        assert_eq!(claims.principal_id, "local-garage");
    }
}
