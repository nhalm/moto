//! Secret storage repository with CRUD operations.
//!
//! Provides in-memory storage for secrets with:
//! - ABAC policy enforcement
//! - Envelope encryption (DEK wraps secret, KEK wraps DEK)
//! - Audit logging for all operations
//!
//! # Example
//!
//! ```
//! use moto_keybox::{
//!     MasterKey, PolicyEngine, SecretMetadata, Scope,
//!     repository::SecretRepository,
//!     svid::{SvidClaims, DEFAULT_SVID_TTL_SECS},
//!     types::SpiffeId,
//! };
//!
//! // Create repository with a master key
//! let master_key = MasterKey::generate();
//! let policy = PolicyEngine::new().with_admin_service("moto-club");
//! let mut repo = SecretRepository::new(master_key, policy);
//!
//! // Admin can create secrets
//! let admin = SvidClaims::new(&SpiffeId::service("moto-club"), DEFAULT_SVID_TTL_SECS);
//! repo.create(&admin, Scope::Global, "ai/anthropic", b"sk-secret").unwrap();
//!
//! // Other principals can read based on ABAC rules
//! let garage = SvidClaims::new(&SpiffeId::garage("my-garage"), DEFAULT_SVID_TTL_SECS);
//! let value = repo.get(&garage, Scope::Global, "ai/anthropic").unwrap();
//! assert_eq!(&value, b"sk-secret");
//! ```

use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use crate::abac::{Action, PolicyEngine};
use crate::envelope::{DataEncryptionKey, EncryptedDek, EncryptedSecret, MasterKey};
use crate::svid::SvidClaims;
use crate::types::{AuditEntry, AuditEventType, Scope, SecretMetadata};
use crate::{Error, Result};

/// A stored secret with encrypted value and DEK.
#[derive(Debug, Clone)]
struct StoredSecret {
    /// Metadata about the secret.
    metadata: SecretMetadata,
    /// The encrypted DEK for this secret.
    encrypted_dek: EncryptedDek,
    /// The encrypted secret value.
    encrypted_value: EncryptedSecret,
}

/// Composite key for looking up secrets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SecretKey {
    scope: Scope,
    service: Option<String>,
    instance_id: Option<String>,
    name: String,
}

impl SecretKey {
    fn from_metadata(meta: &SecretMetadata) -> Self {
        Self {
            scope: meta.scope,
            service: meta.service.clone(),
            instance_id: meta.instance_id.clone(),
            name: meta.name.clone(),
        }
    }

    const fn new(
        scope: Scope,
        service: Option<String>,
        instance_id: Option<String>,
        name: String,
    ) -> Self {
        Self {
            scope,
            service,
            instance_id,
            name,
        }
    }
}

/// In-memory secret storage repository.
///
/// Stores secrets encrypted with envelope encryption and enforces
/// ABAC policies on all operations.
pub struct SecretRepository {
    /// Master key for wrapping/unwrapping DEKs.
    master_key: MasterKey,
    /// Policy engine for access control.
    policy: PolicyEngine,
    /// Secret storage indexed by composite key.
    secrets: HashMap<SecretKey, StoredSecret>,
    /// Audit log entries.
    audit_log: Vec<AuditEntry>,
}

impl SecretRepository {
    /// Creates a new repository with the given master key and policy engine.
    #[must_use]
    pub fn new(master_key: MasterKey, policy: PolicyEngine) -> Self {
        Self {
            master_key,
            policy,
            secrets: HashMap::new(),
            audit_log: Vec::new(),
        }
    }

    /// Creates a new global secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption fails
    pub fn create(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.create_with_context(claims, scope, None, None, name, value)
    }

    /// Creates a new service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption fails
    pub fn create_service(
        &mut self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.create_with_context(claims, Scope::Service, Some(service), None, name, value)
    }

    /// Creates a new instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption fails
    pub fn create_instance(
        &mut self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.create_with_context(
            claims,
            Scope::Instance,
            None,
            Some(instance_id),
            name,
            value,
        )
    }

    /// Creates a secret with full context.
    fn create_with_context(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        // Build metadata for policy check
        let metadata = match scope {
            Scope::Global => SecretMetadata::global(name),
            Scope::Service => SecretMetadata::service(service.unwrap_or_default(), name),
            Scope::Instance => SecretMetadata::instance(instance_id.unwrap_or_default(), name),
        };

        // Check ABAC policy
        self.policy.evaluate(claims, &metadata, Action::Write)?;

        // Check if secret already exists
        let key = SecretKey::from_metadata(&metadata);
        if self.secrets.contains_key(&key) {
            return Err(Error::SecretExists {
                scope: scope.to_string(),
                name: name.to_string(),
            });
        }

        // Generate DEK and encrypt
        let dek = DataEncryptionKey::generate();
        let encrypted_value = dek.encrypt(value)?;
        let encrypted_dek = self.master_key.wrap_dek(&dek)?;

        let stored = StoredSecret {
            metadata: metadata.clone(),
            encrypted_dek,
            encrypted_value,
        };
        self.secrets.insert(key, stored);

        // Audit
        self.audit(claims, AuditEventType::Created, Some(&metadata));

        Ok(metadata)
    }

    /// Retrieves a secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub fn get(&mut self, claims: &SvidClaims, scope: Scope, name: &str) -> Result<Vec<u8>> {
        self.get_with_context(claims, scope, None, None, name)
    }

    /// Retrieves a service-scoped secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub fn get_service(
        &mut self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
    ) -> Result<Vec<u8>> {
        self.get_with_context(claims, Scope::Service, Some(service), None, name)
    }

    /// Retrieves an instance-scoped secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub fn get_instance(
        &mut self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
    ) -> Result<Vec<u8>> {
        self.get_with_context(claims, Scope::Instance, None, Some(instance_id), name)
    }

    /// Gets a secret with full context.
    fn get_with_context(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
    ) -> Result<Vec<u8>> {
        let key = SecretKey::new(
            scope,
            service.map(String::from),
            instance_id.map(String::from),
            name.to_string(),
        );

        let stored = self
            .secrets
            .get(&key)
            .ok_or_else(|| Error::SecretNotFound {
                scope: scope.to_string(),
                name: name.to_string(),
            })?;

        // Check ABAC policy
        self.policy.can_read(claims, &stored.metadata)?;

        // Decrypt
        let dek = self.master_key.unwrap_dek(&stored.encrypted_dek)?;
        let plaintext = dek.decrypt(&stored.encrypted_value)?;

        // Clone metadata for audit (to avoid borrow conflict)
        let metadata = stored.metadata.clone();

        // Audit
        self.audit(claims, AuditEventType::Accessed, Some(&metadata));

        Ok(plaintext)
    }

    /// Updates an existing secret value.
    ///
    /// Creates a new version with a new DEK.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Encryption fails
    pub fn update(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.update_with_context(claims, scope, None, None, name, value)
    }

    /// Updates an existing service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Encryption fails
    pub fn update_service(
        &mut self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.update_with_context(claims, Scope::Service, Some(service), None, name, value)
    }

    /// Updates an existing instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Encryption fails
    pub fn update_instance(
        &mut self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.update_with_context(
            claims,
            Scope::Instance,
            None,
            Some(instance_id),
            name,
            value,
        )
    }

    /// Updates a secret with full context.
    fn update_with_context(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        let key = SecretKey::new(
            scope,
            service.map(String::from),
            instance_id.map(String::from),
            name.to_string(),
        );

        let stored = self
            .secrets
            .get_mut(&key)
            .ok_or_else(|| Error::SecretNotFound {
                scope: scope.to_string(),
                name: name.to_string(),
            })?;

        // Check ABAC policy
        self.policy
            .evaluate(claims, &stored.metadata, Action::Write)?;

        // Generate new DEK and encrypt
        let dek = DataEncryptionKey::generate();
        let encrypted_value = dek.encrypt(value)?;
        let encrypted_dek = self.master_key.wrap_dek(&dek)?;

        // Update stored secret
        stored.metadata.version += 1;
        stored.metadata.updated_at = Utc::now();
        stored.encrypted_dek = encrypted_dek;
        stored.encrypted_value = encrypted_value;

        let metadata = stored.metadata.clone();

        // Audit
        self.audit(claims, AuditEventType::Updated, Some(&metadata));

        Ok(metadata)
    }

    /// Deletes a secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub fn delete(&mut self, claims: &SvidClaims, scope: Scope, name: &str) -> Result<()> {
        self.delete_with_context(claims, scope, None, None, name)
    }

    /// Deletes a service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub fn delete_service(&mut self, claims: &SvidClaims, service: &str, name: &str) -> Result<()> {
        self.delete_with_context(claims, Scope::Service, Some(service), None, name)
    }

    /// Deletes an instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub fn delete_instance(
        &mut self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
    ) -> Result<()> {
        self.delete_with_context(claims, Scope::Instance, None, Some(instance_id), name)
    }

    /// Deletes a secret with full context.
    fn delete_with_context(
        &mut self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
    ) -> Result<()> {
        let key = SecretKey::new(
            scope,
            service.map(String::from),
            instance_id.map(String::from),
            name.to_string(),
        );

        let stored = self
            .secrets
            .get(&key)
            .ok_or_else(|| Error::SecretNotFound {
                scope: scope.to_string(),
                name: name.to_string(),
            })?;

        // Check ABAC policy
        self.policy
            .evaluate(claims, &stored.metadata, Action::Delete)?;

        // Clone metadata for audit before removing
        let metadata = stored.metadata.clone();
        self.secrets.remove(&key);

        // Audit
        self.audit(claims, AuditEventType::Deleted, Some(&metadata));

        Ok(())
    }

    /// Lists all secrets in a scope.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    #[must_use]
    pub fn list(&self, claims: &SvidClaims, scope: Scope) -> Vec<SecretMetadata> {
        self.list_with_context(claims, scope, None, None)
    }

    /// Lists all service-scoped secrets.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    #[must_use]
    pub fn list_service(&self, claims: &SvidClaims, service: &str) -> Vec<SecretMetadata> {
        self.list_with_context(claims, Scope::Service, Some(service), None)
    }

    /// Lists all instance-scoped secrets.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    #[must_use]
    pub fn list_instance(&self, claims: &SvidClaims, instance_id: &str) -> Vec<SecretMetadata> {
        self.list_with_context(claims, Scope::Instance, None, Some(instance_id))
    }

    /// Lists secrets with full context.
    fn list_with_context(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
    ) -> Vec<SecretMetadata> {
        let mut results = Vec::new();

        for stored in self.secrets.values() {
            // Match scope
            if stored.metadata.scope != scope {
                continue;
            }

            // Match service (for service scope)
            if let Some(svc) = service {
                if stored.metadata.service.as_deref() != Some(svc) {
                    continue;
                }
            }

            // Match instance_id (for instance scope)
            if let Some(inst) = instance_id {
                if stored.metadata.instance_id.as_deref() != Some(inst) {
                    continue;
                }
            }

            // Check if principal can read this secret
            if self.policy.can_read(claims, &stored.metadata).is_ok() {
                results.push(stored.metadata.clone());
            }
        }

        // Sort by name for consistent ordering
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results
    }

    /// Returns the audit log entries.
    #[must_use]
    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.audit_log
    }

    /// Clears the audit log.
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Records an audit event.
    fn audit(
        &mut self,
        claims: &SvidClaims,
        event_type: AuditEventType,
        metadata: Option<&SecretMetadata>,
    ) {
        let entry = AuditEntry {
            id: Uuid::now_v7(),
            event_type,
            principal_type: Some(claims.principal_type),
            principal_id: Some(claims.principal_id.clone()),
            spiffe_id: Some(claims.sub.clone()),
            secret_scope: metadata.map(|m| m.scope),
            secret_name: metadata.map(|m| m.name.clone()),
            timestamp: Utc::now(),
        };
        self.audit_log.push(entry);
    }
}

impl std::fmt::Debug for SecretRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRepository")
            .field("secrets_count", &self.secrets.len())
            .field("audit_log_count", &self.audit_log.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svid::DEFAULT_SVID_TTL_SECS;
    use crate::types::SpiffeId;

    fn admin_claims() -> SvidClaims {
        SvidClaims::new(&SpiffeId::service("moto-club"), DEFAULT_SVID_TTL_SECS)
    }

    fn garage_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::garage(id), DEFAULT_SVID_TTL_SECS)
    }

    fn bike_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::bike(id), DEFAULT_SVID_TTL_SECS)
    }

    fn service_claims(id: &str) -> SvidClaims {
        SvidClaims::new(&SpiffeId::service(id), DEFAULT_SVID_TTL_SECS)
    }

    fn test_repo() -> SecretRepository {
        let master_key = MasterKey::generate();
        let policy = PolicyEngine::new().with_admin_service("moto-club");
        SecretRepository::new(master_key, policy)
    }

    #[test]
    fn create_and_get_global_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        // Create
        let meta = repo
            .create(&admin, Scope::Global, "ai/anthropic", b"sk-secret-key")
            .unwrap();
        assert_eq!(meta.scope, Scope::Global);
        assert_eq!(meta.name, "ai/anthropic");
        assert_eq!(meta.version, 1);

        // Get
        let value = repo.get(&admin, Scope::Global, "ai/anthropic").unwrap();
        assert_eq!(&value, b"sk-secret-key");
    }

    #[test]
    fn create_and_get_service_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        // Create
        let meta = repo
            .create_service(&admin, "tokenization", "db/password", b"supersecret")
            .unwrap();
        assert_eq!(meta.scope, Scope::Service);
        assert_eq!(meta.service, Some("tokenization".to_string()));
        assert_eq!(meta.name, "db/password");

        // Get
        let value = repo
            .get_service(&admin, "tokenization", "db/password")
            .unwrap();
        assert_eq!(&value, b"supersecret");
    }

    #[test]
    fn create_and_get_instance_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        // Create
        let meta = repo
            .create_instance(&admin, "garage-123", "dev/token", b"mytoken")
            .unwrap();
        assert_eq!(meta.scope, Scope::Instance);
        assert_eq!(meta.instance_id, Some("garage-123".to_string()));
        assert_eq!(meta.name, "dev/token");

        // Get
        let value = repo
            .get_instance(&admin, "garage-123", "dev/token")
            .unwrap();
        assert_eq!(&value, b"mytoken");
    }

    #[test]
    fn update_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        // Create
        repo.create(&admin, Scope::Global, "ai/openai", b"v1")
            .unwrap();

        // Update
        let meta = repo
            .update(&admin, Scope::Global, "ai/openai", b"v2")
            .unwrap();
        assert_eq!(meta.version, 2);

        // Verify new value
        let value = repo.get(&admin, Scope::Global, "ai/openai").unwrap();
        assert_eq!(&value, b"v2");
    }

    #[test]
    fn delete_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        // Create
        repo.create(&admin, Scope::Global, "to-delete", b"value")
            .unwrap();

        // Delete
        repo.delete(&admin, Scope::Global, "to-delete").unwrap();

        // Verify gone
        let err = repo.get(&admin, Scope::Global, "to-delete").unwrap_err();
        assert!(matches!(err, Error::SecretNotFound { .. }));
    }

    #[test]
    fn list_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "ai/anthropic", b"a")
            .unwrap();
        repo.create(&admin, Scope::Global, "ai/openai", b"b")
            .unwrap();
        repo.create(&admin, Scope::Global, "db/master", b"c")
            .unwrap();

        let list = repo.list(&admin, Scope::Global);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "ai/anthropic");
        assert_eq!(list[1].name, "ai/openai");
        assert_eq!(list[2].name, "db/master");
    }

    #[test]
    fn garage_can_read_global_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let garage = garage_claims("my-garage");

        repo.create(&admin, Scope::Global, "ai/anthropic", b"key")
            .unwrap();

        // Garage can read
        let value = repo.get(&garage, Scope::Global, "ai/anthropic").unwrap();
        assert_eq!(&value, b"key");
    }

    #[test]
    fn garage_cannot_write_global_secrets() {
        let mut repo = test_repo();
        let garage = garage_claims("my-garage");

        let err = repo
            .create(&garage, Scope::Global, "ai/anthropic", b"key")
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn garage_can_access_own_instance_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let garage = garage_claims("garage-abc");

        // Admin creates
        repo.create_instance(&admin, "garage-abc", "dev/token", b"secret")
            .unwrap();

        // Garage can read its own
        let value = repo
            .get_instance(&garage, "garage-abc", "dev/token")
            .unwrap();
        assert_eq!(&value, b"secret");

        // Garage can also write to its own
        repo.update_instance(&garage, "garage-abc", "dev/token", b"updated")
            .unwrap();
    }

    #[test]
    fn garage_cannot_access_other_instance_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let garage = garage_claims("garage-abc");

        repo.create_instance(&admin, "garage-xyz", "dev/token", b"secret")
            .unwrap();

        let err = repo
            .get_instance(&garage, "garage-xyz", "dev/token")
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn bike_can_read_service_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let bike = bike_claims("bike-123");

        repo.create_service(&admin, "tokenization", "db/password", b"dbpass")
            .unwrap();

        let value = repo
            .get_service(&bike, "tokenization", "db/password")
            .unwrap();
        assert_eq!(&value, b"dbpass");
    }

    #[test]
    fn bike_cannot_write_service_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let bike = bike_claims("bike-123");

        repo.create_service(&admin, "tokenization", "db/password", b"dbpass")
            .unwrap();

        let err = repo
            .update_service(&bike, "tokenization", "db/password", b"new")
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn service_can_access_own_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let service = service_claims("tokenization");

        repo.create_service(&admin, "tokenization", "db/password", b"pass")
            .unwrap();

        // Service can read its own secrets
        let value = repo
            .get_service(&service, "tokenization", "db/password")
            .unwrap();
        assert_eq!(&value, b"pass");

        // Service can update its own secrets
        repo.update_service(&service, "tokenization", "db/password", b"newpass")
            .unwrap();
    }

    #[test]
    fn service_cannot_access_other_service_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let service = service_claims("tokenization");

        repo.create_service(&admin, "other-service", "db/password", b"pass")
            .unwrap();

        let err = repo
            .get_service(&service, "other-service", "db/password")
            .unwrap_err();
        assert!(matches!(err, Error::AccessDenied { .. }));
    }

    #[test]
    fn cannot_create_duplicate_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "unique", b"value")
            .unwrap();

        let err = repo
            .create(&admin, Scope::Global, "unique", b"other")
            .unwrap_err();
        assert!(matches!(err, Error::SecretExists { .. }));
    }

    #[test]
    fn get_nonexistent_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        let err = repo.get(&admin, Scope::Global, "nonexistent").unwrap_err();
        assert!(matches!(err, Error::SecretNotFound { .. }));
    }

    #[test]
    fn update_nonexistent_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        let err = repo
            .update(&admin, Scope::Global, "nonexistent", b"value")
            .unwrap_err();
        assert!(matches!(err, Error::SecretNotFound { .. }));
    }

    #[test]
    fn delete_nonexistent_secret() {
        let mut repo = test_repo();
        let admin = admin_claims();

        let err = repo
            .delete(&admin, Scope::Global, "nonexistent")
            .unwrap_err();
        assert!(matches!(err, Error::SecretNotFound { .. }));
    }

    #[test]
    fn audit_log_captures_events() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "test", b"value")
            .unwrap();
        repo.get(&admin, Scope::Global, "test").unwrap();
        repo.update(&admin, Scope::Global, "test", b"new").unwrap();
        repo.delete(&admin, Scope::Global, "test").unwrap();

        let log = repo.audit_log();
        assert_eq!(log.len(), 4);
        assert_eq!(log[0].event_type, AuditEventType::Created);
        assert_eq!(log[1].event_type, AuditEventType::Accessed);
        assert_eq!(log[2].event_type, AuditEventType::Updated);
        assert_eq!(log[3].event_type, AuditEventType::Deleted);
    }

    #[test]
    fn audit_log_contains_principal_info() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "test", b"value")
            .unwrap();

        let entry = &repo.audit_log()[0];
        assert_eq!(entry.principal_id, Some("moto-club".to_string()));
        assert!(entry.spiffe_id.as_ref().unwrap().contains("moto-club"));
        assert_eq!(entry.secret_scope, Some(Scope::Global));
        assert_eq!(entry.secret_name, Some("test".to_string()));
    }

    #[test]
    fn list_filters_by_scope() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "global-1", b"a")
            .unwrap();
        repo.create_service(&admin, "svc", "service-1", b"b")
            .unwrap();
        repo.create_instance(&admin, "inst", "instance-1", b"c")
            .unwrap();

        let global = repo.list(&admin, Scope::Global);
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].name, "global-1");

        let service = repo.list_service(&admin, "svc");
        assert_eq!(service.len(), 1);
        assert_eq!(service[0].name, "service-1");

        let instance = repo.list_instance(&admin, "inst");
        assert_eq!(instance.len(), 1);
        assert_eq!(instance[0].name, "instance-1");
    }

    #[test]
    fn list_only_shows_accessible_secrets() {
        let mut repo = test_repo();
        let admin = admin_claims();
        let garage = garage_claims("garage-abc");

        // Create instance secrets for different garages
        repo.create_instance(&admin, "garage-abc", "token", b"a")
            .unwrap();
        repo.create_instance(&admin, "garage-xyz", "token", b"b")
            .unwrap();

        // Garage can only see its own
        let list = repo.list_instance(&garage, "garage-abc");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].instance_id, Some("garage-abc".to_string()));
    }

    #[test]
    fn empty_secret_roundtrip() {
        let mut repo = test_repo();
        let admin = admin_claims();

        repo.create(&admin, Scope::Global, "empty", b"").unwrap();
        let value = repo.get(&admin, Scope::Global, "empty").unwrap();
        assert!(value.is_empty());
    }

    #[test]
    fn large_secret_roundtrip() {
        let mut repo = test_repo();
        let admin = admin_claims();

        let large_value = vec![0xABu8; 64 * 1024]; // 64KB
        repo.create(&admin, Scope::Global, "large", &large_value)
            .unwrap();
        let value = repo.get(&admin, Scope::Global, "large").unwrap();
        assert_eq!(value, large_value);
    }

    #[test]
    fn debug_output_does_not_leak() {
        let repo = test_repo();
        let debug = format!("{repo:?}");
        assert!(debug.contains("SecretRepository"));
        assert!(debug.contains("secrets_count"));
        // Should not contain actual key material
        assert!(!debug.contains("key"));
    }
}
