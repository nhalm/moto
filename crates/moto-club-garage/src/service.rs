//! Garage service - business logic for garage management.
//!
//! Coordinates between the database layer and Kubernetes to manage
//! garage lifecycles.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use moto_club_db::{DbError, DbPool, Garage, GarageStatus, TerminationReason, garage_repo};
use moto_club_k8s::{
    DEV_CONTAINER_POD_NAME, GarageK8s, GarageLimitRangeOps, GarageNamespaceInput,
    GarageNamespaceOps, GarageNetworkPolicyOps, GaragePodInput, GaragePodOps, GaragePodStatus,
    GarageResourceQuotaOps,
};
use moto_club_types::GarageId;

use crate::lifecycle::{GarageLifecycle, LifecycleError};
use crate::{DEFAULT_IMAGE, DEFAULT_TTL_SECONDS, MAX_TTL_SECONDS, MIN_TTL_SECONDS};

/// Errors from garage service operations.
#[derive(Debug, Error)]
pub enum GarageServiceError {
    /// Database error.
    #[error("database error: {0}")]
    Database(#[from] DbError),

    /// Kubernetes error.
    #[error("kubernetes error: {0}")]
    Kubernetes(#[from] moto_k8s::Error),

    /// Lifecycle error.
    #[error("lifecycle error: {0}")]
    Lifecycle(#[from] LifecycleError),

    /// Garage not found.
    #[error("garage not found: {0}")]
    NotFound(String),

    /// Garage not owned by the requesting user.
    #[error("garage '{name}' is owned by '{owner}', not '{requester}'")]
    NotOwned {
        /// Garage name.
        name: String,
        /// Actual owner.
        owner: String,
        /// User making the request.
        requester: String,
    },

    /// Garage already exists.
    #[error("garage already exists: {0}")]
    AlreadyExists(String),

    /// Garage is terminated.
    #[error("garage is terminated: {0}")]
    Terminated(String),

    /// Garage has expired.
    #[error("garage has expired: {0}")]
    Expired(String),

    /// Invalid TTL.
    #[error("invalid TTL: {message}")]
    InvalidTtl {
        /// Error message.
        message: String,
    },

    /// Name generation failed after too many attempts.
    #[error("failed to generate unique name after {attempts} attempts")]
    NameGenerationFailed {
        /// Number of attempts made.
        attempts: u32,
    },
}

/// Input for creating a new garage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGarageInput {
    /// Human-friendly name (auto-generated if not provided).
    pub name: Option<String>,
    /// Git branch to work on.
    pub branch: String,
    /// TTL in seconds (default: 4 hours).
    pub ttl_seconds: Option<i32>,
    /// Custom container image (uses default if not provided).
    pub image: Option<String>,
    /// Engine name (what the garage is working on).
    pub engine: Option<String>,
    /// Optional repository to clone on startup.
    pub repo: Option<moto_club_k8s::RepoConfig>,
    /// Include PostgreSQL supporting service.
    #[serde(default)]
    pub with_postgres: bool,
    /// Include Redis supporting service.
    #[serde(default)]
    pub with_redis: bool,
}

/// Input for extending a garage's TTL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendTtlInput {
    /// Seconds to add to the current expiry.
    pub seconds: i32,
}

/// Garage service - orchestrates garage operations.
///
/// This service coordinates between:
/// - Database (garage records)
/// - Kubernetes (namespaces, pods)
///
/// It ensures consistency between these layers during garage
/// creation, updates, and termination.
#[derive(Clone)]
pub struct GarageService {
    db: DbPool,
    k8s: GarageK8s,
}

impl GarageService {
    /// Creates a new garage service.
    #[must_use]
    pub const fn new(db: DbPool, k8s: GarageK8s) -> Self {
        Self { db, k8s }
    }

    /// Creates a new garage.
    ///
    /// # Flow
    ///
    /// 1. Validate TTL
    /// 2. Generate or validate name
    /// 3. Create database record (status: Pending)
    /// 4. Create K8s namespace
    /// 5. Deploy dev container pod
    /// 6. Return garage info
    ///
    /// If K8s operations fail, the database record is cleaned up.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - TTL is out of valid range
    /// - Name is already taken
    /// - Name generation fails
    /// - Database or K8s operations fail
    #[instrument(skip(self), fields(owner = %owner))]
    pub async fn create(
        &self,
        owner: &str,
        input: CreateGarageInput,
    ) -> Result<Garage, GarageServiceError> {
        // Validate TTL
        let ttl_seconds = input.ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS);
        Self::validate_ttl(ttl_seconds)?;

        // Generate or use provided name
        let name = match &input.name {
            Some(n) => {
                // Validate name is available
                if !garage_repo::is_name_available(&self.db, n).await? {
                    return Err(GarageServiceError::AlreadyExists(n.clone()));
                }
                n.clone()
            }
            None => self.generate_unique_name().await?,
        };

        info!(garage_name = %name, branch = %input.branch, ttl_seconds, "creating garage");

        // Generate ID
        let id = Uuid::now_v7();
        let garage_id = GarageId::from_uuid(id);
        let namespace = format!("moto-garage-{}", garage_id.short());
        let pod_name = DEV_CONTAINER_POD_NAME.to_string();

        // Create database record
        let image = input
            .image
            .clone()
            .unwrap_or_else(|| DEFAULT_IMAGE.to_string());
        let db_input = garage_repo::CreateGarage {
            id,
            name: name.clone(),
            owner: owner.to_string(),
            branch: input.branch.clone(),
            image,
            ttl_seconds,
            namespace: namespace.clone(),
            pod_name: pod_name.clone(),
        };

        let garage = garage_repo::create(&self.db, db_input).await.map_err(|e| {
            if matches!(e, DbError::AlreadyExists { .. }) {
                GarageServiceError::AlreadyExists(name.clone())
            } else {
                GarageServiceError::Database(e)
            }
        })?;

        debug!(garage_id = %id, namespace = %namespace, "database record created");

        // Create K8s resources
        let k8s_result = self
            .create_k8s_resources(&garage_id, &name, owner, &input, &namespace)
            .await;

        if let Err(e) = k8s_result {
            // Cleanup database record on K8s failure
            error!(garage_id = %id, error = %e, "K8s resource creation failed, cleaning up");
            if let Err(cleanup_err) = garage_repo::delete(&self.db, id).await {
                warn!(garage_id = %id, error = %cleanup_err, "failed to cleanup database record");
            }
            return Err(e);
        }

        info!(garage_id = %id, garage_name = %name, "garage created successfully");
        Ok(garage)
    }

    /// Gets a garage by name.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` if the garage doesn't exist.
    #[instrument(skip(self))]
    pub async fn get(&self, name: &str) -> Result<Garage, GarageServiceError> {
        garage_repo::get_by_name(&self.db, name)
            .await
            .map_err(|e| match e {
                DbError::NotFound { .. } => GarageServiceError::NotFound(name.to_string()),
                other => GarageServiceError::Database(other),
            })
    }

    /// Gets a garage by name, verifying ownership.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` if the garage doesn't exist.
    /// Returns `NotOwned` if the garage is owned by someone else.
    #[instrument(skip(self))]
    pub async fn get_owned(&self, owner: &str, name: &str) -> Result<Garage, GarageServiceError> {
        let garage = self.get(name).await?;

        if garage.owner != owner {
            return Err(GarageServiceError::NotOwned {
                name: name.to_string(),
                owner: garage.owner,
                requester: owner.to_string(),
            });
        }

        Ok(garage)
    }

    /// Lists garages for an owner.
    ///
    /// # Arguments
    ///
    /// * `owner` - The owner to filter by
    /// * `include_terminated` - Whether to include terminated garages
    ///
    /// # Errors
    ///
    /// Returns a database error if the query fails.
    #[instrument(skip(self))]
    pub async fn list(
        &self,
        owner: &str,
        include_terminated: bool,
    ) -> Result<Vec<Garage>, GarageServiceError> {
        Ok(garage_repo::list_by_owner(&self.db, owner, include_terminated).await?)
    }

    /// Extends a garage's TTL.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Garage doesn't exist
    /// - Garage is not owned by the requester
    /// - Garage is terminated
    /// - Garage has expired
    /// - New TTL would exceed maximum
    #[instrument(skip(self), fields(garage_name = %name))]
    pub async fn extend_ttl(
        &self,
        owner: &str,
        name: &str,
        input: ExtendTtlInput,
    ) -> Result<Garage, GarageServiceError> {
        let garage = self.get_owned(owner, name).await?;

        // Check lifecycle
        if !GarageLifecycle::can_extend_ttl(garage.status) {
            return Err(GarageServiceError::Terminated(name.to_string()));
        }

        // Check if expired
        if garage.expires_at < Utc::now() {
            return Err(GarageServiceError::Expired(name.to_string()));
        }

        // Check new TTL doesn't exceed max
        let new_total_ttl = garage.ttl_seconds + input.seconds;
        if new_total_ttl > MAX_TTL_SECONDS {
            return Err(GarageServiceError::InvalidTtl {
                message: format!(
                    "total TTL would be {new_total_ttl}s, maximum is {MAX_TTL_SECONDS}s"
                ),
            });
        }

        info!(
            garage_id = %garage.id,
            additional_seconds = input.seconds,
            new_total_ttl,
            "extending garage TTL"
        );

        let updated = garage_repo::extend_ttl(&self.db, garage.id, input.seconds).await?;
        Ok(updated)
    }

    /// Closes a garage.
    ///
    /// # Flow
    ///
    /// 1. Verify ownership
    /// 2. Mark as terminated in database
    /// 3. Delete K8s namespace (cascades to all resources)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Garage doesn't exist
    /// - Garage is not owned by the requester
    /// - Garage is already terminated
    #[instrument(skip(self), fields(garage_name = %name))]
    pub async fn close(&self, owner: &str, name: &str) -> Result<Garage, GarageServiceError> {
        let garage = self.get_owned(owner, name).await?;

        // Check lifecycle
        if !GarageLifecycle::can_close(garage.status) {
            return Err(GarageServiceError::Terminated(name.to_string()));
        }

        info!(garage_id = %garage.id, garage_name = %name, "closing garage");

        // Mark as terminated in database first
        let terminated =
            garage_repo::terminate(&self.db, garage.id, TerminationReason::UserClosed).await?;

        // Delete K8s namespace (this cascades to all resources)
        let garage_id = GarageId::from_uuid(garage.id);
        if let Err(e) = self.k8s.delete_garage_namespace(&garage_id).await {
            warn!(
                garage_id = %garage.id,
                error = %e,
                "failed to delete K8s namespace (may already be deleted)"
            );
            // Don't fail the close operation - the garage is marked as terminated
            // Reconciliation will eventually clean up orphaned resources
        }

        info!(garage_id = %garage.id, garage_name = %name, "garage closed");
        Ok(terminated)
    }

    /// Updates a garage's status.
    ///
    /// Used by reconciliation to sync status from K8s.
    ///
    /// # Errors
    ///
    /// Returns an error if the transition is invalid or the database update fails.
    #[instrument(skip(self), fields(garage_id = %id))]
    pub async fn update_status(
        &self,
        id: Uuid,
        new_status: GarageStatus,
    ) -> Result<Garage, GarageServiceError> {
        let garage = garage_repo::get_by_id(&self.db, id).await?;

        // Validate transition
        GarageLifecycle::validate_transition(garage.status, new_status)?;

        debug!(
            garage_id = %id,
            from = %garage.status,
            to = %new_status,
            "updating garage status"
        );

        let updated = garage_repo::update_status(&self.db, id, new_status).await?;
        Ok(updated)
    }

    /// Gets the pod status for a garage.
    ///
    /// # Errors
    ///
    /// Returns an error if the K8s operation fails.
    #[instrument(skip(self), fields(garage_id = %id))]
    pub async fn get_pod_status(
        &self,
        id: &GarageId,
    ) -> Result<GaragePodStatus, GarageServiceError> {
        Ok(self.k8s.get_garage_pod_status(id).await?)
    }

    /// Validates TTL is within acceptable range.
    fn validate_ttl(ttl_seconds: i32) -> Result<(), GarageServiceError> {
        if ttl_seconds < MIN_TTL_SECONDS {
            return Err(GarageServiceError::InvalidTtl {
                message: format!("TTL must be at least {MIN_TTL_SECONDS}s"),
            });
        }
        if ttl_seconds > MAX_TTL_SECONDS {
            return Err(GarageServiceError::InvalidTtl {
                message: format!("TTL must be at most {MAX_TTL_SECONDS}s"),
            });
        }
        Ok(())
    }

    /// Generates a unique garage name.
    async fn generate_unique_name(&self) -> Result<String, GarageServiceError> {
        const MAX_ATTEMPTS: u32 = 10;

        for _ in 0..MAX_ATTEMPTS {
            let name = generate_name();
            if garage_repo::is_name_available(&self.db, &name).await? {
                return Ok(name);
            }
            debug!(name = %name, "name collision, trying another");
        }

        Err(GarageServiceError::NameGenerationFailed {
            attempts: MAX_ATTEMPTS,
        })
    }

    /// Creates K8s resources for a garage.
    ///
    /// # Flow (per spec lines 866-879)
    ///
    /// 4. Create K8s namespace: moto-garage-{id}
    /// 5. Apply labels: moto.dev/type=garage, moto.dev/garage-id={id}, moto.dev/owner={owner}
    /// 6. Apply `NetworkPolicy` (per garage-isolation.md spec)
    /// 7. Create `ServiceAccount` (for keybox auth) (deferred)
    /// 8. Deploy dev container pod
    async fn create_k8s_resources(
        &self,
        garage_id: &GarageId,
        name: &str,
        owner: &str,
        input: &CreateGarageInput,
        namespace: &str,
    ) -> Result<(), GarageServiceError> {
        let ttl_seconds = input.ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS);
        let expires_at = Utc::now() + chrono::Duration::seconds(i64::from(ttl_seconds));

        // Step 4-5: Create namespace (labels are applied by create_garage_namespace)
        let ns_input = GarageNamespaceInput {
            id: *garage_id,
            name: name.to_string(),
            owner: owner.to_string(),
            expires_at: Some(expires_at),
            engine: input.engine.clone(),
        };

        debug!(namespace = %namespace, "creating K8s namespace");
        self.k8s.create_garage_namespace(&ns_input).await?;

        // Step 6: Apply NetworkPolicy per garage-isolation.md spec
        debug!(namespace = %namespace, "creating NetworkPolicy");
        if let Err(e) = self.k8s.create_garage_network_policy(garage_id).await {
            // Cleanup namespace on NetworkPolicy creation failure
            warn!(namespace = %namespace, error = %e, "NetworkPolicy creation failed, cleaning up namespace");
            if let Err(ns_err) = self.k8s.delete_garage_namespace(garage_id).await {
                warn!(namespace = %namespace, error = %ns_err, "failed to cleanup namespace");
            }
            return Err(e.into());
        }

        // Step 6b: Apply ResourceQuota per garage-isolation.md spec
        debug!(namespace = %namespace, "creating ResourceQuota");
        if let Err(e) = self.k8s.create_garage_resource_quota(garage_id).await {
            // Cleanup namespace on ResourceQuota creation failure
            warn!(namespace = %namespace, error = %e, "ResourceQuota creation failed, cleaning up namespace");
            if let Err(ns_err) = self.k8s.delete_garage_namespace(garage_id).await {
                warn!(namespace = %namespace, error = %ns_err, "failed to cleanup namespace");
            }
            return Err(e.into());
        }

        // Step 6c: Apply LimitRange per garage-isolation.md spec
        debug!(namespace = %namespace, "creating LimitRange");
        if let Err(e) = self.k8s.create_garage_limit_range(garage_id).await {
            // Cleanup namespace on LimitRange creation failure
            warn!(namespace = %namespace, error = %e, "LimitRange creation failed, cleaning up namespace");
            if let Err(ns_err) = self.k8s.delete_garage_namespace(garage_id).await {
                warn!(namespace = %namespace, error = %ns_err, "failed to cleanup namespace");
            }
            return Err(e.into());
        }

        // Step 8: Deploy pod
        let pod_input = GaragePodInput {
            id: *garage_id,
            name: name.to_string(),
            owner: owner.to_string(),
            branch: input.branch.clone(),
            image: input.image.clone(),
            repo: input.repo.clone(),
        };

        debug!(namespace = %namespace, "deploying dev container pod");
        if let Err(e) = self.k8s.deploy_garage_pod(&pod_input).await {
            // Cleanup namespace on pod deployment failure
            warn!(namespace = %namespace, error = %e, "pod deployment failed, cleaning up namespace");
            if let Err(ns_err) = self.k8s.delete_garage_namespace(garage_id).await {
                warn!(namespace = %namespace, error = %ns_err, "failed to cleanup namespace");
            }
            return Err(e.into());
        }

        Ok(())
    }
}

/// Generates a random adjective-animal name.
fn generate_name() -> String {
    use rand::Rng;

    const ADJECTIVES: &[&str] = &[
        "bold", "calm", "dark", "eager", "fair", "glad", "hale", "idle", "jolly", "keen", "lank",
        "meek", "neat", "odd", "pale", "quick", "rare", "sly", "tall", "vast", "warm", "zany",
        "agile", "brave", "crisp", "deft", "epic", "fresh", "grand", "hardy",
    ];

    const ANIMALS: &[&str] = &[
        "ant", "bat", "cat", "dog", "elk", "fox", "gnu", "hog", "ibex", "jay", "kite", "lynx",
        "moth", "newt", "owl", "puma", "quail", "rat", "seal", "toad", "vole", "wolf", "yak",
        "zebra", "ape", "bear", "crow", "dove", "eel", "frog",
    ];

    let mut rng = rand::rng();
    let adj = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let animal = ANIMALS[rng.random_range(0..ANIMALS.len())];

    format!("{adj}-{animal}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_name_format() {
        let name = generate_name();
        assert!(name.contains('-'));
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }

    #[test]
    fn create_garage_input_defaults() {
        let input = CreateGarageInput {
            name: None,
            branch: "main".to_string(),
            ttl_seconds: None,
            image: None,
            engine: None,
            repo: None,
            with_postgres: false,
            with_redis: false,
        };

        assert!(input.name.is_none());
        assert_eq!(input.branch, "main");
        assert!(input.ttl_seconds.is_none());
        assert!(input.repo.is_none());
        assert!(!input.with_postgres);
        assert!(!input.with_redis);
    }

    #[test]
    fn extend_ttl_input_serde() {
        let input = ExtendTtlInput { seconds: 3600 };
        let json = serde_json::to_string(&input).unwrap();
        let parsed: ExtendTtlInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seconds, 3600);
    }

    #[test]
    fn error_display() {
        let err = GarageServiceError::NotFound("test".to_string());
        assert_eq!(err.to_string(), "garage not found: test");

        let err = GarageServiceError::NotOwned {
            name: "test".to_string(),
            owner: "alice".to_string(),
            requester: "bob".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "garage 'test' is owned by 'alice', not 'bob'"
        );

        let err = GarageServiceError::InvalidTtl {
            message: "too long".to_string(),
        };
        assert_eq!(err.to_string(), "invalid TTL: too long");
    }
}
