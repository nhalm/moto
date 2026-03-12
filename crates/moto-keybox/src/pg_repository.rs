//! `PostgreSQL`-backed secret repository.
//!
//! Provides persistent storage for secrets using `PostgreSQL`, with:
//! - ABAC policy enforcement
//! - Envelope encryption (DEK wraps secret, KEK wraps DEK)
//! - Audit logging stored in `PostgreSQL`
//!
//! This replaces the in-memory `SecretRepository` for production use.

// Version numbers in the database are i32, but our API uses u32.
// These casts are safe because version numbers are always positive.
#![allow(clippy::cast_sign_loss)]

use moto_keybox_db::{
    AuditEventType as DbAuditEventType, DbPool, InsertAuditEntry, PrincipalType as DbPrincipalType,
    Scope as DbScope, audit_repo, secret_repo,
};

use crate::abac::{Action, PolicyEngine};
use crate::envelope::{DataEncryptionKey, EncryptedDek, EncryptedSecret, MasterKey};
use crate::svid::SvidClaims;
use crate::types::{AuditEventType, PrincipalType, Scope, SecretMetadata};
use crate::{Error, Result};

/// PostgreSQL-backed secret repository.
///
/// Stores secrets encrypted with envelope encryption in `PostgreSQL`
/// and enforces ABAC policies on all operations.
pub struct PgSecretRepository {
    /// Database connection pool.
    pool: DbPool,
    /// Master key for wrapping/unwrapping DEKs.
    master_key: MasterKey,
    /// Policy engine for access control.
    policy: PolicyEngine,
}

impl PgSecretRepository {
    /// Creates a new repository with the given database pool and master key.
    #[must_use]
    pub const fn new(pool: DbPool, master_key: MasterKey, policy: PolicyEngine) -> Self {
        Self {
            pool,
            master_key,
            policy,
        }
    }

    /// Creates a new global secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption or database operation fails
    pub async fn create(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.create_with_context(claims, scope, None, None, name, value)
            .await
    }

    /// Creates a new service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption or database operation fails
    pub async fn create_service(
        &self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.create_with_context(claims, Scope::Service, Some(service), None, name, value)
            .await
    }

    /// Creates a new instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - A secret with the same name already exists
    /// - Encryption or database operation fails
    pub async fn create_instance(
        &self,
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
        .await
    }

    /// Creates a secret with full context.
    async fn create_with_context(
        &self,
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
        if let Err(e) = self.policy.evaluate(claims, &metadata, Action::Write) {
            self.audit(claims, AuditEventType::AccessDenied, Some(&metadata))
                .await;
            return Err(e);
        }

        // Check if secret already exists
        let existing =
            secret_repo::get_secret(&self.pool, to_db_scope(scope), service, instance_id, name)
                .await
                .map_err(|e| db_error(&e))?;

        if existing.is_some() {
            return Err(Error::SecretExists {
                scope: scope.to_string(),
                name: name.to_string(),
            });
        }

        // Generate DEK and encrypt
        let dek = DataEncryptionKey::generate();
        let encrypted_value = dek.encrypt(value)?;
        let encrypted_dek = self.master_key.wrap_dek(&dek)?;

        // Store encrypted DEK
        let db_dek = secret_repo::create_encrypted_dek(
            &self.pool,
            &encrypted_dek.encrypted_key,
            &encrypted_dek.nonce,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Store secret metadata
        let db_secret =
            secret_repo::create_secret(&self.pool, to_db_scope(scope), service, instance_id, name)
                .await
                .map_err(|e| db_error(&e))?;

        // Store secret version with encrypted value
        secret_repo::create_secret_version(
            &self.pool,
            db_secret.id,
            1,
            &encrypted_value.ciphertext,
            &encrypted_value.nonce,
            db_dek.id,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Audit
        self.audit(claims, AuditEventType::SecretCreated, Some(&metadata))
            .await;

        // Build result metadata
        let result = SecretMetadata {
            id: db_secret.id,
            scope,
            service: service.map(String::from),
            instance_id: instance_id.map(String::from),
            name: name.to_string(),
            version: 1,
            created_at: db_secret.created_at,
            updated_at: db_secret.updated_at,
        };

        Ok(result)
    }

    /// Retrieves a secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub async fn get(&self, claims: &SvidClaims, scope: Scope, name: &str) -> Result<Vec<u8>> {
        self.get_with_context(claims, scope, None, None, name).await
    }

    /// Retrieves a service-scoped secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub async fn get_service(
        &self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
    ) -> Result<Vec<u8>> {
        self.get_with_context(claims, Scope::Service, Some(service), None, name)
            .await
    }

    /// Retrieves an instance-scoped secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Decryption fails
    pub async fn get_instance(
        &self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
    ) -> Result<Vec<u8>> {
        self.get_with_context(claims, Scope::Instance, None, Some(instance_id), name)
            .await
    }

    /// Gets a secret with full context.
    async fn get_with_context(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
    ) -> Result<Vec<u8>> {
        let secret_with_value = secret_repo::get_secret_with_value(
            &self.pool,
            to_db_scope(scope),
            service,
            instance_id,
            name,
        )
        .await
        .map_err(|e| db_error(&e))?
        .ok_or_else(|| Error::SecretNotFound {
            scope: scope.to_string(),
            name: name.to_string(),
        })?;

        // Build metadata for policy check
        let metadata = SecretMetadata {
            id: secret_with_value.secret.id,
            scope,
            service: secret_with_value.secret.service.clone(),
            instance_id: secret_with_value.secret.instance_id.clone(),
            name: secret_with_value.secret.name.clone(),
            version: secret_with_value.secret.current_version as u32,
            created_at: secret_with_value.secret.created_at,
            updated_at: secret_with_value.secret.updated_at,
        };

        // Check ABAC policy
        if let Err(e) = self.policy.can_read(claims, &metadata) {
            self.audit(claims, AuditEventType::AccessDenied, Some(&metadata))
                .await;
            return Err(e);
        }

        // Decrypt
        let encrypted_dek = EncryptedDek {
            encrypted_key: secret_with_value.encrypted_dek_key,
            nonce: secret_with_value.dek_nonce,
        };
        let dek = self.master_key.unwrap_dek(&encrypted_dek)?;

        let encrypted_value = EncryptedSecret {
            ciphertext: secret_with_value.ciphertext,
            nonce: secret_with_value.value_nonce,
        };
        let plaintext = dek.decrypt(&encrypted_value)?;

        // Audit
        self.audit(claims, AuditEventType::SecretAccessed, Some(&metadata))
            .await;

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
    /// - Encryption or database operation fails
    pub async fn update(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.update_with_context(claims, scope, None, None, name, value)
            .await
    }

    /// Updates an existing service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Encryption or database operation fails
    pub async fn update_service(
        &self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        self.update_with_context(claims, Scope::Service, Some(service), None, name, value)
            .await
    }

    /// Updates an existing instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    /// - Encryption or database operation fails
    pub async fn update_instance(
        &self,
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
        .await
    }

    /// Updates a secret with full context.
    async fn update_with_context(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
        value: &[u8],
    ) -> Result<SecretMetadata> {
        // Get existing secret
        let existing =
            secret_repo::get_secret(&self.pool, to_db_scope(scope), service, instance_id, name)
                .await
                .map_err(|e| db_error(&e))?
                .ok_or_else(|| Error::SecretNotFound {
                    scope: scope.to_string(),
                    name: name.to_string(),
                })?;

        // Build metadata for policy check
        let metadata = SecretMetadata {
            id: existing.id,
            scope,
            service: existing.service.clone(),
            instance_id: existing.instance_id.clone(),
            name: existing.name.clone(),
            version: existing.current_version as u32,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
        };

        // Check ABAC policy
        if let Err(e) = self.policy.evaluate(claims, &metadata, Action::Write) {
            self.audit(claims, AuditEventType::AccessDenied, Some(&metadata))
                .await;
            return Err(e);
        }

        // Generate new DEK and encrypt
        let dek = DataEncryptionKey::generate();
        let encrypted_value = dek.encrypt(value)?;
        let encrypted_dek = self.master_key.wrap_dek(&dek)?;

        // Store new encrypted DEK
        let db_dek = secret_repo::create_encrypted_dek(
            &self.pool,
            &encrypted_dek.encrypted_key,
            &encrypted_dek.nonce,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Calculate new version
        let new_version = existing.current_version + 1;

        // Store new secret version
        secret_repo::create_secret_version(
            &self.pool,
            existing.id,
            new_version,
            &encrypted_value.ciphertext,
            &encrypted_value.nonce,
            db_dek.id,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Update secret's current version
        let updated = secret_repo::update_secret_version(&self.pool, existing.id, new_version)
            .await
            .map_err(|e| db_error(&e))?;

        // Build result metadata
        let result = SecretMetadata {
            id: updated.id,
            scope,
            service: updated.service,
            instance_id: updated.instance_id,
            name: updated.name,
            version: new_version as u32,
            created_at: updated.created_at,
            updated_at: updated.updated_at,
        };

        // Audit
        self.audit(claims, AuditEventType::SecretUpdated, Some(&result))
            .await;

        Ok(result)
    }

    /// Deletes a secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub async fn delete(&self, claims: &SvidClaims, scope: Scope, name: &str) -> Result<()> {
        self.delete_with_context(claims, scope, None, None, name)
            .await
    }

    /// Deletes a service-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub async fn delete_service(
        &self,
        claims: &SvidClaims,
        service: &str,
        name: &str,
    ) -> Result<()> {
        self.delete_with_context(claims, Scope::Service, Some(service), None, name)
            .await
    }

    /// Deletes an instance-scoped secret.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Access is denied by ABAC policy
    /// - The secret does not exist
    pub async fn delete_instance(
        &self,
        claims: &SvidClaims,
        instance_id: &str,
        name: &str,
    ) -> Result<()> {
        self.delete_with_context(claims, Scope::Instance, None, Some(instance_id), name)
            .await
    }

    /// Deletes a secret with full context.
    async fn delete_with_context(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
    ) -> Result<()> {
        // Get existing secret
        let existing =
            secret_repo::get_secret(&self.pool, to_db_scope(scope), service, instance_id, name)
                .await
                .map_err(|e| db_error(&e))?
                .ok_or_else(|| Error::SecretNotFound {
                    scope: scope.to_string(),
                    name: name.to_string(),
                })?;

        // Build metadata for policy check
        let metadata = SecretMetadata {
            id: existing.id,
            scope,
            service: existing.service.clone(),
            instance_id: existing.instance_id.clone(),
            name: existing.name.clone(),
            version: existing.current_version as u32,
            created_at: existing.created_at,
            updated_at: existing.updated_at,
        };

        // Check ABAC policy
        if let Err(e) = self.policy.evaluate(claims, &metadata, Action::Delete) {
            self.audit(claims, AuditEventType::AccessDenied, Some(&metadata))
                .await;
            return Err(e);
        }

        // Soft-delete the secret
        secret_repo::delete_secret(&self.pool, existing.id)
            .await
            .map_err(|e| db_error(&e))?;

        // Audit
        self.audit(claims, AuditEventType::SecretDeleted, Some(&metadata))
            .await;

        Ok(())
    }

    /// Lists all secrets in a scope.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    pub async fn list(&self, claims: &SvidClaims, scope: Scope) -> Vec<SecretMetadata> {
        self.list_with_context(claims, scope, None, None).await
    }

    /// Lists all service-scoped secrets.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    pub async fn list_service(&self, claims: &SvidClaims, service: &str) -> Vec<SecretMetadata> {
        self.list_with_context(claims, Scope::Service, Some(service), None)
            .await
    }

    /// Lists all instance-scoped secrets.
    ///
    /// Returns metadata only (no values). Only secrets the principal
    /// has access to are returned.
    pub async fn list_instance(
        &self,
        claims: &SvidClaims,
        instance_id: &str,
    ) -> Vec<SecretMetadata> {
        self.list_with_context(claims, Scope::Instance, None, Some(instance_id))
            .await
    }

    /// Lists secrets with full context.
    async fn list_with_context(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
    ) -> Vec<SecretMetadata> {
        let db_secrets = match (service, instance_id) {
            (Some(svc), _) => secret_repo::list_service_secrets(&self.pool, svc)
                .await
                .unwrap_or_default(),
            (_, Some(inst)) => secret_repo::list_instance_secrets(&self.pool, inst)
                .await
                .unwrap_or_default(),
            _ => secret_repo::list_secrets(&self.pool, to_db_scope(scope))
                .await
                .unwrap_or_default(),
        };

        let mut results = Vec::new();

        for db_secret in db_secrets {
            let metadata = SecretMetadata {
                id: db_secret.id,
                scope: from_db_scope(db_secret.scope),
                service: db_secret.service.clone(),
                instance_id: db_secret.instance_id.clone(),
                name: db_secret.name.clone(),
                version: db_secret.current_version as u32,
                created_at: db_secret.created_at,
                updated_at: db_secret.updated_at,
            };

            // Check if principal can read this secret
            if self.policy.can_read(claims, &metadata).is_ok() {
                results.push(metadata);
            }
        }

        // Sort by name for consistent ordering
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results
    }

    /// Rotates the DEK for a secret.
    ///
    /// The secret value does not change — it is decrypted with the old DEK
    /// and re-encrypted with a new DEK. Creates a new version.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The secret does not exist
    /// - Decryption or re-encryption fails
    /// - Database operation fails
    pub async fn rotate_dek(
        &self,
        claims: &SvidClaims,
        scope: Scope,
        service: Option<&str>,
        instance_id: Option<&str>,
        name: &str,
    ) -> Result<SecretMetadata> {
        // Step 2-3: Look up secret and fetch current version with encrypted value and DEK
        let secret_with_value = secret_repo::get_secret_with_value(
            &self.pool,
            to_db_scope(scope),
            service,
            instance_id,
            name,
        )
        .await
        .map_err(|e| db_error(&e))?
        .ok_or_else(|| Error::SecretNotFound {
            scope: scope.to_string(),
            name: name.to_string(),
        })?;

        // Step 4: Decrypt - unwrap old DEK with master key, decrypt ciphertext
        let old_encrypted_dek = EncryptedDek {
            encrypted_key: secret_with_value.encrypted_dek_key,
            nonce: secret_with_value.dek_nonce,
        };
        let old_dek = self.master_key.unwrap_dek(&old_encrypted_dek)?;

        let old_encrypted_value = EncryptedSecret {
            ciphertext: secret_with_value.ciphertext,
            nonce: secret_with_value.value_nonce,
        };
        let plaintext = old_dek.decrypt(&old_encrypted_value)?;

        // Step 5-7: Generate new DEK, re-encrypt value, wrap new DEK
        let new_dek = DataEncryptionKey::generate();
        let new_encrypted_value = new_dek.encrypt(&plaintext)?;
        let new_encrypted_dek = self.master_key.wrap_dek(&new_dek)?;

        // Step 8: Store new encrypted DEK
        let db_dek = secret_repo::create_encrypted_dek(
            &self.pool,
            &new_encrypted_dek.encrypted_key,
            &new_encrypted_dek.nonce,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Step 9: Create new secret_versions row
        let new_version = secret_with_value.secret.current_version + 1;
        secret_repo::create_secret_version(
            &self.pool,
            secret_with_value.secret.id,
            new_version,
            &new_encrypted_value.ciphertext,
            &new_encrypted_value.nonce,
            db_dek.id,
        )
        .await
        .map_err(|e| db_error(&e))?;

        // Step 10: Update secret's current_version and updated_at
        let updated = secret_repo::update_secret_version(
            &self.pool,
            secret_with_value.secret.id,
            new_version,
        )
        .await
        .map_err(|e| db_error(&e))?;

        let result = SecretMetadata {
            id: updated.id,
            scope,
            service: updated.service,
            instance_id: updated.instance_id,
            name: updated.name,
            version: new_version as u32,
            created_at: updated.created_at,
            updated_at: updated.updated_at,
        };

        // Step 11: Log dek_rotated audit event
        self.audit(claims, AuditEventType::DekRotated, Some(&result))
            .await;

        Ok(result)
    }

    /// Records an audit event.
    async fn audit(
        &self,
        claims: &SvidClaims,
        event_type: AuditEventType,
        metadata: Option<&SecretMetadata>,
    ) {
        let db_event_type = to_db_audit_event_type(event_type);
        let (action, resource_type, resource_id) = audit_fields_for_event(event_type, metadata);
        let outcome = outcome_for_event(event_type);

        let entry = InsertAuditEntry {
            event_type: db_event_type,
            service: "keybox",
            principal_type: to_db_principal_type(claims.principal_type),
            principal_id: &claims.sub,
            action,
            resource_type,
            resource_id: &resource_id,
            outcome,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            client_ip: None,
        };

        let result = audit_repo::insert_audit_entry(&self.pool, &entry).await;

        if let Err(e) = result {
            tracing::warn!(error = %e, "failed to insert audit log entry");
        }
    }

    /// Adds an audit entry directly (for events outside CRUD operations).
    pub async fn add_audit_entry(&self, entry: &crate::types::AuditEntry) {
        let insert = InsertAuditEntry {
            event_type: to_db_audit_event_type(entry.event_type),
            service: "keybox",
            principal_type: to_db_principal_type(entry.principal_type),
            principal_id: &entry.principal_id,
            action: &entry.action,
            resource_type: &entry.resource_type,
            resource_id: &entry.resource_id,
            outcome: &entry.outcome,
            metadata: entry.metadata.clone(),
            client_ip: entry.client_ip.as_deref(),
        };

        let result = audit_repo::insert_audit_entry(&self.pool, &insert).await;

        if let Err(e) = result {
            tracing::warn!(error = %e, "failed to insert audit log entry");
        }
    }

    /// Returns a reference to the database pool for health checks.
    #[must_use]
    pub const fn pool(&self) -> &DbPool {
        &self.pool
    }
}

impl std::fmt::Debug for PgSecretRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgSecretRepository")
            .field("pool", &"[DbPool]")
            .finish_non_exhaustive()
    }
}

// =============================================================================
// Type conversions between moto-keybox and moto-keybox-db types
// =============================================================================

const fn to_db_scope(scope: Scope) -> DbScope {
    match scope {
        Scope::Global => DbScope::Global,
        Scope::Service => DbScope::Service,
        Scope::Instance => DbScope::Instance,
    }
}

const fn from_db_scope(scope: DbScope) -> Scope {
    match scope {
        DbScope::Global => Scope::Global,
        DbScope::Service => Scope::Service,
        DbScope::Instance => Scope::Instance,
    }
}

const fn to_db_principal_type(pt: PrincipalType) -> DbPrincipalType {
    match pt {
        PrincipalType::Garage => DbPrincipalType::Garage,
        PrincipalType::Bike => DbPrincipalType::Bike,
        PrincipalType::Service => DbPrincipalType::Service,
        PrincipalType::Anonymous => DbPrincipalType::Anonymous,
    }
}

const fn to_db_audit_event_type(et: AuditEventType) -> DbAuditEventType {
    match et {
        AuditEventType::SecretAccessed => DbAuditEventType::SecretAccessed,
        AuditEventType::SecretCreated => DbAuditEventType::SecretCreated,
        AuditEventType::SecretUpdated => DbAuditEventType::SecretUpdated,
        AuditEventType::SecretDeleted => DbAuditEventType::SecretDeleted,
        AuditEventType::SvidIssued => DbAuditEventType::SvidIssued,
        AuditEventType::AuthFailed => DbAuditEventType::AuthFailed,
        AuditEventType::AccessDenied => DbAuditEventType::AccessDenied,
        AuditEventType::DekRotated => DbAuditEventType::DekRotated,
    }
}

/// Returns (action, `resource_type`, `resource_id`) for an audit event.
fn audit_fields_for_event(
    event_type: AuditEventType,
    metadata: Option<&SecretMetadata>,
) -> (&'static str, &'static str, String) {
    let resource_id = metadata
        .map(|m| format!("{}/{}", m.scope, m.name))
        .unwrap_or_default();

    match event_type {
        AuditEventType::SecretAccessed => ("read", "secret", resource_id),
        AuditEventType::SecretCreated => ("create", "secret", resource_id),
        AuditEventType::SecretUpdated => ("update", "secret", resource_id),
        AuditEventType::SecretDeleted => ("delete", "secret", resource_id),
        AuditEventType::DekRotated => ("rotate", "secret", resource_id),
        AuditEventType::SvidIssued => ("create", "svid", resource_id),
        AuditEventType::AuthFailed => ("auth_fail", "token", resource_id),
        AuditEventType::AccessDenied => ("deny", "secret", resource_id),
    }
}

const fn outcome_for_event(event_type: AuditEventType) -> &'static str {
    match event_type {
        AuditEventType::AccessDenied | AuditEventType::AuthFailed => "denied",
        _ => "success",
    }
}

fn db_error(e: &moto_keybox_db::DbError) -> Error {
    Error::Crypto {
        message: format!("database error: {e}"),
    }
}
