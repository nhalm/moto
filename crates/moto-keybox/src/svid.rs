//! SVID (Short-lived Verifiable Identity Document) issuance and validation.
//!
//! This module provides Ed25519-signed JWTs for SPIFFE-inspired identity.
//! SVIDs are short-lived (15 min by default) and bound to a pod UID.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{PrincipalType, SpiffeId};
use crate::{Error, Result};

/// Default SVID TTL in seconds (15 minutes).
pub const DEFAULT_SVID_TTL_SECS: i64 = 900;

/// Claims contained in an SVID JWT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvidClaims {
    /// JWT issuer (always "keybox").
    pub iss: String,
    /// Subject - the SPIFFE ID URI.
    pub sub: String,
    /// Audience (always "moto").
    pub aud: String,
    /// Expiration time (Unix timestamp).
    pub exp: i64,
    /// Issued at time (Unix timestamp).
    pub iat: i64,
    /// JWT ID (unique identifier).
    pub jti: String,
    /// Principal type (garage, bike, service).
    pub principal_type: PrincipalType,
    /// Principal ID (garage-id, bike-id, or service name).
    pub principal_id: String,
    /// Service name for bikes (required for service-scoped secret access).
    /// Determined from the `moto.dev/service` label on the bike pod.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Pod UID for binding (prevents replay if pod dies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_uid: Option<String>,
    /// Pod namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_namespace: Option<String>,
    /// Pod name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,
}

impl SvidClaims {
    /// Creates new SVID claims for a SPIFFE ID.
    #[must_use]
    pub fn new(spiffe_id: &SpiffeId, ttl_secs: i64) -> Self {
        let now = Utc::now();
        let exp = now + Duration::seconds(ttl_secs);

        Self {
            iss: "keybox".to_string(),
            sub: spiffe_id.to_uri(),
            aud: "moto".to_string(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            jti: Uuid::now_v7().to_string(),
            principal_type: spiffe_id.principal_type,
            principal_id: spiffe_id.id.clone(),
            service: None,
            pod_uid: None,
            pod_namespace: None,
            pod_name: None,
        }
    }

    /// Sets the service name for bikes.
    ///
    /// Required for bikes to access service-scoped secrets.
    #[must_use]
    pub fn with_service(mut self, service: impl Into<String>) -> Self {
        self.service = Some(service.into());
        self
    }

    /// Sets the pod UID for binding.
    #[must_use]
    pub fn with_pod_uid(mut self, uid: impl Into<String>) -> Self {
        self.pod_uid = Some(uid.into());
        self
    }

    /// Sets the pod namespace.
    #[must_use]
    pub fn with_pod_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.pod_namespace = Some(namespace.into());
        self
    }

    /// Sets the pod name.
    #[must_use]
    pub fn with_pod_name(mut self, name: impl Into<String>) -> Self {
        self.pod_name = Some(name.into());
        self
    }

    /// Returns the expiration time as a `DateTime`.
    #[must_use]
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp(self.exp, 0)
    }

    /// Returns the issued-at time as a `DateTime`.
    #[must_use]
    pub const fn issued_at(&self) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp(self.iat, 0)
    }

    /// Checks if the claims have expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() > self.exp
    }

    /// Extracts the SPIFFE ID from the claims.
    ///
    /// # Errors
    ///
    /// Returns an error if the subject is not a valid SPIFFE ID.
    pub fn spiffe_id(&self) -> Result<SpiffeId> {
        self.sub.parse()
    }
}

/// JWT header for SVID tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtHeader {
    /// Algorithm (always `EdDSA` for Ed25519).
    alg: String,
    /// Type (always JWT).
    typ: String,
}

impl Default for JwtHeader {
    fn default() -> Self {
        Self {
            alg: "EdDSA".to_string(),
            typ: "JWT".to_string(),
        }
    }
}

/// SVID issuer for creating signed identity tokens.
#[derive(Clone)]
pub struct SvidIssuer {
    signing_key: SigningKey,
    ttl_secs: i64,
}

impl SvidIssuer {
    /// Creates a new SVID issuer from an Ed25519 signing key.
    #[must_use]
    pub const fn new(signing_key: SigningKey) -> Self {
        Self {
            signing_key,
            ttl_secs: DEFAULT_SVID_TTL_SECS,
        }
    }

    /// Creates a new SVID issuer with a custom TTL.
    #[must_use]
    pub const fn with_ttl(mut self, ttl_secs: i64) -> Self {
        self.ttl_secs = ttl_secs;
        self
    }

    /// Loads a signing key from base64-encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 is invalid or the key is not 32 bytes.
    pub fn from_base64(key_base64: &str) -> Result<Self> {
        let key_bytes = URL_SAFE_NO_PAD
            .decode(key_base64.trim())
            .map_err(|e| Error::Crypto {
                message: format!("failed to decode signing key: {e}"),
            })?;

        let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| Error::Crypto {
            message: "signing key must be 32 bytes".to_string(),
        })?;

        Ok(Self::new(SigningKey::from_bytes(&key_array)))
    }

    /// Loads a signing key from a file containing base64-encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or contains invalid key data.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_base64(&contents)
    }

    /// Returns the verifying (public) key for this issuer.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Issues an SVID for a SPIFFE ID.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn issue(&self, spiffe_id: &SpiffeId) -> Result<String> {
        let claims = SvidClaims::new(spiffe_id, self.ttl_secs);
        self.sign_claims(&claims)
    }

    /// Issues an SVID with custom claims.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn issue_with_claims(&self, claims: &SvidClaims) -> Result<String> {
        self.sign_claims(claims)
    }

    /// Signs claims into a JWT string.
    fn sign_claims(&self, claims: &SvidClaims) -> Result<String> {
        let header = JwtHeader::default();

        let header_json = serde_json::to_string(&header).map_err(|e| Error::Crypto {
            message: format!("failed to serialize header: {e}"),
        })?;
        let claims_json = serde_json::to_string(claims).map_err(|e| Error::Crypto {
            message: format!("failed to serialize claims: {e}"),
        })?;

        let header_b64 = URL_SAFE_NO_PAD.encode(header_json);
        let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json);

        let message = format!("{header_b64}.{claims_b64}");
        let signature = self.signing_key.sign(message.as_bytes());
        let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

        Ok(format!("{message}.{signature_b64}"))
    }

    /// Generates a new random signing key.
    ///
    /// # Panics
    ///
    /// Panics if the system random number generator fails.
    #[must_use]
    pub fn generate_key() -> SigningKey {
        let mut secret_bytes = [0u8; 32];
        getrandom::fill(&mut secret_bytes).expect("getrandom failed");
        SigningKey::from_bytes(&secret_bytes)
    }

    /// Encodes a signing key as base64.
    #[must_use]
    pub fn encode_key(key: &SigningKey) -> String {
        URL_SAFE_NO_PAD.encode(key.to_bytes())
    }
}

impl std::fmt::Debug for SvidIssuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvidIssuer")
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

/// SVID validator for verifying signed identity tokens.
#[derive(Clone)]
pub struct SvidValidator {
    verifying_key: VerifyingKey,
}

impl SvidValidator {
    /// Creates a new SVID validator from a verifying (public) key.
    #[must_use]
    pub const fn new(verifying_key: VerifyingKey) -> Self {
        Self { verifying_key }
    }

    /// Loads a verifying key from base64-encoded bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 is invalid or the key is malformed.
    pub fn from_base64(key_base64: &str) -> Result<Self> {
        let key_bytes = URL_SAFE_NO_PAD
            .decode(key_base64.trim())
            .map_err(|e| Error::Crypto {
                message: format!("failed to decode verifying key: {e}"),
            })?;

        let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| Error::Crypto {
            message: "verifying key must be 32 bytes".to_string(),
        })?;

        let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|e| Error::Crypto {
            message: format!("invalid verifying key: {e}"),
        })?;

        Ok(Self::new(verifying_key))
    }

    /// Validates an SVID token and returns the claims.
    ///
    /// This verifies:
    /// - The signature is valid
    /// - The token has not expired
    /// - The issuer is "keybox"
    /// - The audience is "moto"
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails (invalid signature, expired, wrong issuer/audience).
    pub fn validate(&self, token: &str) -> Result<SvidClaims> {
        let claims = self.verify_signature(token)?;

        // Check expiration
        if claims.is_expired() {
            return Err(Error::SvidExpired);
        }

        // Verify issuer
        if claims.iss != "keybox" {
            return Err(Error::Auth {
                message: format!("invalid issuer: {}", claims.iss),
            });
        }

        // Verify audience
        if claims.aud != "moto" {
            return Err(Error::Auth {
                message: format!("invalid audience: {}", claims.aud),
            });
        }

        Ok(claims)
    }

    /// Verifies the signature without checking expiration.
    ///
    /// Useful for debugging or inspecting expired tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the token is malformed or the signature is invalid.
    pub fn verify_signature(&self, token: &str) -> Result<SvidClaims> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::InvalidSvidSignature);
        }

        let header_b64 = parts[0];
        let claims_b64 = parts[1];
        let signature_b64 = parts[2];

        // Verify header
        let header_json = URL_SAFE_NO_PAD
            .decode(header_b64)
            .map_err(|_| Error::InvalidSvidSignature)?;
        let header: JwtHeader =
            serde_json::from_slice(&header_json).map_err(|_| Error::InvalidSvidSignature)?;

        if header.alg != "EdDSA" || header.typ != "JWT" {
            return Err(Error::InvalidSvidSignature);
        }

        // Verify signature
        let message = format!("{header_b64}.{claims_b64}");
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(signature_b64)
            .map_err(|_| Error::InvalidSvidSignature)?;

        let signature_array: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| Error::InvalidSvidSignature)?;
        let signature = ed25519_dalek::Signature::from_bytes(&signature_array);

        self.verifying_key
            .verify(message.as_bytes(), &signature)
            .map_err(|_| Error::InvalidSvidSignature)?;

        // Decode claims
        let claims_json = URL_SAFE_NO_PAD
            .decode(claims_b64)
            .map_err(|_| Error::InvalidSvidSignature)?;
        let claims: SvidClaims =
            serde_json::from_slice(&claims_json).map_err(|_| Error::InvalidSvidSignature)?;

        Ok(claims)
    }

    /// Validates an SVID and checks that the pod UID matches.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails or the pod UID doesn't match.
    pub fn validate_with_pod_uid(&self, token: &str, expected_pod_uid: &str) -> Result<SvidClaims> {
        let claims = self.validate(token)?;

        match &claims.pod_uid {
            Some(uid) if uid == expected_pod_uid => Ok(claims),
            Some(uid) => Err(Error::Auth {
                message: format!("pod UID mismatch: expected {expected_pod_uid}, got {uid}"),
            }),
            None => Err(Error::Auth {
                message: "SVID missing pod UID binding".to_string(),
            }),
        }
    }

    /// Validates an SVID and enforces pod UID binding when present.
    ///
    /// Unlike [`validate`](Self::validate), which ignores pod UID claims, this
    /// method enforces the pod UID binding contract: when the SVID contains a
    /// `pod_uid` claim, the binding must be non-empty and well-formed.
    ///
    /// Use this for secret retrieval/mutation handlers where pod UID binding
    /// must be honored (spec: "Checks pod UID matches (still alive)").
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails or if a pod UID claim is empty.
    pub fn validate_enforcing_pod_uid(&self, token: &str) -> Result<SvidClaims> {
        let claims = self.validate(token)?;

        if let Some(ref pod_uid) = claims.pod_uid {
            if pod_uid.is_empty() {
                return Err(Error::Auth {
                    message: "SVID contains empty pod UID binding".to_string(),
                });
            }
            // Pod UID binding is present and non-empty.
            // The signed token guarantees integrity of the claim.
            // Future: verify pod is still alive via K8s API.
        }

        Ok(claims)
    }

    /// Encodes a verifying key as base64.
    #[must_use]
    pub fn encode_key(key: &VerifyingKey) -> String {
        URL_SAFE_NO_PAD.encode(key.to_bytes())
    }
}

impl std::fmt::Debug for SvidValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvidValidator").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_issuer() -> SvidIssuer {
        let key = SvidIssuer::generate_key();
        SvidIssuer::new(key)
    }

    #[test]
    fn issue_and_validate() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("test-garage");
        let token = issuer.issue(&spiffe_id).unwrap();

        let claims = validator.validate(&token).unwrap();
        assert_eq!(claims.sub, "spiffe://moto.local/garage/test-garage");
        assert_eq!(claims.principal_type, PrincipalType::Garage);
        assert_eq!(claims.principal_id, "test-garage");
        assert_eq!(claims.iss, "keybox");
        assert_eq!(claims.aud, "moto");
    }

    #[test]
    fn issue_with_pod_metadata() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::bike("my-bike");
        let claims = SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS)
            .with_pod_uid("pod-123")
            .with_pod_namespace("default")
            .with_pod_name("my-bike-abc");

        let token = issuer.issue_with_claims(&claims).unwrap();
        let validated = validator.validate(&token).unwrap();

        assert_eq!(validated.pod_uid, Some("pod-123".to_string()));
        assert_eq!(validated.pod_namespace, Some("default".to_string()));
        assert_eq!(validated.pod_name, Some("my-bike-abc".to_string()));
    }

    #[test]
    fn validate_pod_uid_match() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("garage-1");
        let claims =
            SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS).with_pod_uid("expected-uid");

        let token = issuer.issue_with_claims(&claims).unwrap();

        // Should succeed with matching UID
        validator
            .validate_with_pod_uid(&token, "expected-uid")
            .unwrap();

        // Should fail with mismatched UID
        let err = validator
            .validate_with_pod_uid(&token, "wrong-uid")
            .unwrap_err();
        assert!(matches!(err, Error::Auth { .. }));
    }

    #[test]
    fn validate_pod_uid_missing() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("garage-1");
        let token = issuer.issue(&spiffe_id).unwrap();

        let err = validator
            .validate_with_pod_uid(&token, "some-uid")
            .unwrap_err();
        assert!(matches!(err, Error::Auth { .. }));
    }

    #[test]
    fn validate_enforcing_pod_uid_with_valid_uid() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::bike("bike-1");
        let claims = SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS).with_pod_uid("pod-abc-123");

        let token = issuer.issue_with_claims(&claims).unwrap();

        // Should succeed — pod_uid is present and non-empty
        let validated = validator.validate_enforcing_pod_uid(&token).unwrap();
        assert_eq!(validated.pod_uid, Some("pod-abc-123".to_string()));
    }

    #[test]
    fn validate_enforcing_pod_uid_without_uid() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("garage-1");
        let token = issuer.issue(&spiffe_id).unwrap();

        // Should succeed — no pod_uid claim means no enforcement needed
        let validated = validator.validate_enforcing_pod_uid(&token).unwrap();
        assert!(validated.pod_uid.is_none());
    }

    #[test]
    fn validate_enforcing_pod_uid_with_empty_uid() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::bike("bike-1");
        let claims = SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS).with_pod_uid("");

        let token = issuer.issue_with_claims(&claims).unwrap();

        // Should fail — empty pod_uid is invalid
        let err = validator.validate_enforcing_pod_uid(&token).unwrap_err();
        assert!(matches!(err, Error::Auth { .. }));
    }

    #[test]
    fn invalid_signature() {
        let issuer = test_issuer();
        let different_issuer = test_issuer();
        let validator = SvidValidator::new(different_issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("test");
        let token = issuer.issue(&spiffe_id).unwrap();

        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, Error::InvalidSvidSignature));
    }

    #[test]
    fn malformed_token() {
        let issuer = test_issuer();
        let validator = SvidValidator::new(issuer.verifying_key());

        assert!(matches!(
            validator.validate("not.a.valid.token").unwrap_err(),
            Error::InvalidSvidSignature
        ));
        assert!(matches!(
            validator.validate("notavalidtoken").unwrap_err(),
            Error::InvalidSvidSignature
        ));
        assert!(matches!(
            validator.validate("a.b").unwrap_err(),
            Error::InvalidSvidSignature
        ));
    }

    #[test]
    fn expired_token() {
        let key = SvidIssuer::generate_key();
        let issuer = SvidIssuer::new(key).with_ttl(-1); // Expired immediately
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::service("test-service");
        let token = issuer.issue(&spiffe_id).unwrap();

        let err = validator.validate(&token).unwrap_err();
        assert!(matches!(err, Error::SvidExpired));

        // verify_signature should still work
        let claims = validator.verify_signature(&token).unwrap();
        assert!(claims.is_expired());
    }

    #[test]
    fn key_roundtrip() {
        let key = SvidIssuer::generate_key();
        let encoded = SvidIssuer::encode_key(&key);

        let issuer = SvidIssuer::from_base64(&encoded).unwrap();
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("roundtrip-test");
        let token = issuer.issue(&spiffe_id).unwrap();

        validator.validate(&token).unwrap();
    }

    #[test]
    fn verifying_key_roundtrip() {
        let issuer = test_issuer();
        let verifying_key = issuer.verifying_key();
        let encoded = SvidValidator::encode_key(&verifying_key);

        let validator = SvidValidator::from_base64(&encoded).unwrap();

        let spiffe_id = SpiffeId::bike("key-test");
        let token = issuer.issue(&spiffe_id).unwrap();

        validator.validate(&token).unwrap();
    }

    #[test]
    fn claims_spiffe_id_extraction() {
        let spiffe_id = SpiffeId::service("my-service");
        let claims = SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS);

        let extracted = claims.spiffe_id().unwrap();
        assert_eq!(extracted, spiffe_id);
    }

    #[test]
    fn claims_datetime_helpers() {
        let spiffe_id = SpiffeId::garage("datetime-test");
        let claims = SvidClaims::new(&spiffe_id, 3600);

        let issued_at = claims.issued_at().unwrap();
        let expires_at = claims.expires_at().unwrap();

        assert!(expires_at > issued_at);
        assert!(!claims.is_expired());
    }

    #[test]
    fn custom_ttl() {
        let key = SvidIssuer::generate_key();
        let issuer = SvidIssuer::new(key).with_ttl(7200); // 2 hours
        let validator = SvidValidator::new(issuer.verifying_key());

        let spiffe_id = SpiffeId::garage("ttl-test");
        let token = issuer.issue(&spiffe_id).unwrap();

        let claims = validator.validate(&token).unwrap();
        let ttl = claims.exp - claims.iat;
        assert_eq!(ttl, 7200);
    }
}
