//! Garage reconciliation: K8s → Database.
//!
//! Synchronizes garage state between Kubernetes and the database.
//! K8s is the source of truth.

use std::collections::HashSet;
use std::time::Duration;

use thiserror::Error;
use tokio::time::interval;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use moto_club_db::{DbPool, GarageStatus, TerminationReason, garage_repo, wg_garage_repo};
use moto_club_k8s::{
    GarageK8s, GarageNamespaceOps, GaragePodOps, GaragePodStatus, GaragePostgresOps, GarageRedisOps,
};
use moto_club_types::GarageId;
use moto_k8s::Labels;

/// Configuration for the reconciliation loop.
#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    /// Interval between reconciliation cycles.
    pub interval: Duration,
    /// Whether to delete orphan namespaces (namespaces in K8s but not in DB).
    pub delete_orphans: bool,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            delete_orphans: false,
        }
    }
}

impl ReconcileConfig {
    /// Creates a new config with a custom interval.
    #[must_use]
    pub const fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Creates a new config that deletes orphan namespaces.
    #[must_use]
    pub const fn with_delete_orphans(mut self, delete_orphans: bool) -> Self {
        self.delete_orphans = delete_orphans;
        self
    }
}

/// Statistics from a reconciliation cycle.
#[derive(Debug, Clone, Default)]
pub struct ReconcileStats {
    /// Number of garages checked.
    pub checked: usize,
    /// Number of garages updated (status changed).
    pub updated: usize,
    /// Number of garages terminated (pod lost or namespace missing).
    pub terminated: usize,
    /// Number of orphan namespaces found.
    pub orphans: usize,
    /// Number of orphan namespaces deleted.
    pub orphans_deleted: usize,
    /// Number of errors encountered.
    pub errors: usize,
}

/// Errors from reconciliation.
#[derive(Debug, Error)]
pub enum ReconcileError {
    /// Database error.
    #[error("database error: {0}")]
    Database(#[from] moto_club_db::DbError),

    /// Kubernetes error.
    #[error("kubernetes error: {0}")]
    Kubernetes(#[from] moto_k8s::Error),
}

/// Reconciles garage state between K8s and the database.
///
/// Runs periodically to ensure the database reflects the actual K8s state.
#[derive(Clone)]
pub struct GarageReconciler {
    db: DbPool,
    k8s: GarageK8s,
    config: ReconcileConfig,
}

impl GarageReconciler {
    /// Creates a new reconciler.
    #[must_use]
    pub const fn new(db: DbPool, k8s: GarageK8s, config: ReconcileConfig) -> Self {
        Self { db, k8s, config }
    }

    /// Runs the reconciliation loop continuously.
    ///
    /// This runs forever, reconciling on each interval tick.
    pub async fn run(&self) {
        let mut ticker = interval(self.config.interval);

        loop {
            ticker.tick().await;

            match self.reconcile_once().await {
                Ok(stats) => {
                    if stats.updated > 0 || stats.terminated > 0 || stats.orphans > 0 {
                        info!(
                            checked = stats.checked,
                            updated = stats.updated,
                            terminated = stats.terminated,
                            orphans = stats.orphans,
                            orphans_deleted = stats.orphans_deleted,
                            "reconciliation complete"
                        );
                    } else {
                        debug!(
                            checked = stats.checked,
                            "reconciliation complete, no changes"
                        );
                    }
                }
                Err(e) => {
                    error!(error = %e, "reconciliation failed");
                }
            }
        }
    }

    /// Runs a single reconciliation cycle.
    ///
    /// # Errors
    ///
    /// Returns an error if K8s or DB operations fail critically.
    #[instrument(skip(self), name = "reconcile")]
    pub async fn reconcile_once(&self) -> Result<ReconcileStats, ReconcileError> {
        let mut stats = ReconcileStats::default();

        // Step 1: Get all garage namespaces from K8s
        let k8s_namespaces = self.k8s.list_garage_namespaces().await?;
        let k8s_ids: HashSet<String> = k8s_namespaces
            .iter()
            .filter_map(|ns| {
                ns.metadata
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.get(Labels::GARAGE_ID).cloned())
            })
            .collect();

        debug!(
            k8s_count = k8s_namespaces.len(),
            "found garage namespaces in K8s"
        );

        // Step 2: Get all non-terminated garages from DB
        let db_garages = garage_repo::list_all(&self.db, false).await?;
        let db_ids: HashSet<String> = db_garages.iter().map(|g| g.id.to_string()).collect();

        debug!(
            db_count = db_garages.len(),
            "found non-terminated garages in DB"
        );

        // Step 3: Reconcile K8s → DB (update status, mark lost pods)
        for ns in &k8s_namespaces {
            let Some(labels) = &ns.metadata.labels else {
                continue;
            };
            let Some(id_str) = labels.get(Labels::GARAGE_ID) else {
                continue;
            };

            stats.checked += 1;

            // Check if this garage exists in DB
            if db_ids.contains(id_str) {
                // Garage exists in DB - update status from pod
                if let Err(e) = self.reconcile_garage_status(id_str, &mut stats).await {
                    warn!(garage_id = %id_str, error = %e, "failed to reconcile garage status");
                    stats.errors += 1;
                }
            } else {
                // Orphan namespace (in K8s but not in DB)
                stats.orphans += 1;
                warn!(
                    garage_id = %id_str,
                    namespace = ?ns.metadata.name,
                    "orphan namespace found (in K8s but not in DB)"
                );

                if self.config.delete_orphans {
                    if let Ok(garage_id) = id_str.parse::<Uuid>() {
                        let garage_id = GarageId::from_uuid(garage_id);
                        match self.k8s.delete_garage_namespace(&garage_id).await {
                            Ok(()) => {
                                info!(garage_id = %id_str, "deleted orphan namespace");
                                stats.orphans_deleted += 1;
                            }
                            Err(e) => {
                                warn!(garage_id = %id_str, error = %e, "failed to delete orphan namespace");
                                stats.errors += 1;
                            }
                        }
                    }
                }
            }
        }

        // Step 4: Reconcile DB → K8s (mark missing namespaces as terminated)
        for garage in &db_garages {
            let id_str = garage.id.to_string();

            if !k8s_ids.contains(&id_str) {
                // Namespace missing in K8s - mark as terminated
                debug!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    "namespace missing in K8s, terminating"
                );

                match garage_repo::terminate(
                    &self.db,
                    garage.id,
                    TerminationReason::NamespaceMissing,
                )
                .await
                {
                    Ok(_) => {
                        info!(
                            garage_id = %id_str,
                            garage_name = %garage.name,
                            "garage terminated: namespace_missing"
                        );
                        stats.terminated += 1;
                    }
                    Err(e) => {
                        warn!(garage_id = %id_str, error = %e, "failed to terminate garage");
                        stats.errors += 1;
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Reconciles a single garage's status from K8s pod state.
    async fn reconcile_garage_status(
        &self,
        id_str: &str,
        stats: &mut ReconcileStats,
    ) -> Result<(), ReconcileError> {
        let garage_id = id_str
            .parse::<Uuid>()
            .map(GarageId::from_uuid)
            .map_err(|_| moto_club_db::DbError::NotFound {
                entity: "garage",
                id: id_str.to_string(),
            })?;

        let uuid = garage_id.as_uuid();

        // Get current DB state
        let garage = garage_repo::get_by_id(&self.db, uuid).await?;

        // Skip if already terminated
        if garage.status == GarageStatus::Terminated {
            return Ok(());
        }

        // Get pod status from K8s
        let pod_status = self.k8s.get_garage_pod_status(&garage_id).await?;

        // Map pod status to garage status
        // Ready criteria check: see check_ready_criteria() for full Ready criteria per spec
        // See garage-lifecycle.md spec for full Ready criteria:
        //   1. Pod running (containers ready)
        //   2. Terminal daemon up (ttyd on port 7681) - checked via container readiness probe
        //   3. WireGuard registered - checked in check_ready_criteria()
        //   4. Repo cloned - checked via init container completion in check_ready_criteria()
        let new_status = match pod_status {
            GaragePodStatus::Pending => GarageStatus::Pending,
            GaragePodStatus::Running => GarageStatus::Initializing,
            GaragePodStatus::Ready => {
                // Pod containers are ready, but we need to check additional ready criteria
                // before transitioning to Ready status per garage-lifecycle.md and supporting-services.md
                if !self.check_ready_criteria(&garage_id, uuid, id_str).await {
                    return Ok(());
                }
                GarageStatus::Ready
            }
            GaragePodStatus::Failed => {
                // Pod failed (init container or main container failure)
                // Per spec: Failed state for startup failures
                debug!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    "pod failed, transitioning to Failed state"
                );
                GarageStatus::Failed
            }
            GaragePodStatus::Succeeded => {
                // Pod completed successfully (shouldn't happen for long-running containers)
                // Treat as terminated
                garage_repo::terminate(&self.db, uuid, TerminationReason::PodLost).await?;
                info!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    pod_status = %pod_status,
                    "garage terminated: pod_lost (pod succeeded)"
                );
                stats.terminated += 1;
                return Ok(());
            }
            GaragePodStatus::Unknown => {
                // Pod might be missing - check if it actually exists
                if self.k8s.get_garage_pod(&garage_id).await.is_ok() {
                    // Pod exists but status unknown - keep current status
                    return Ok(());
                }
                // Pod doesn't exist - terminate
                garage_repo::terminate(&self.db, uuid, TerminationReason::PodLost).await?;
                info!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    "garage terminated: pod_lost (pod not found)"
                );
                stats.terminated += 1;
                return Ok(());
            }
        };

        // Update status if changed (but don't downgrade from Ready to Running)
        if new_status != garage.status && should_update_status(garage.status, new_status) {
            debug!(
                garage_id = %id_str,
                old_status = %garage.status,
                new_status = %new_status,
                "updating garage status"
            );
            garage_repo::update_status(&self.db, uuid, new_status).await?;
            stats.updated += 1;
        }

        Ok(())
    }
}

impl GarageReconciler {
    /// Checks all ready criteria for a garage pod.
    ///
    /// Per garage-lifecycle.md and supporting-services.md specs, a garage is Ready when:
    /// 1. Pod running (containers ready) - already checked
    /// 2. Terminal daemon up (ttyd) - checked via container readiness probe
    /// 3. WireGuard registered
    /// 4. Repo cloned (init container completed successfully)
    /// 5. Supporting services available (if requested)
    async fn check_ready_criteria(&self, garage_id: &GarageId, uuid: Uuid, id_str: &str) -> bool {
        // Check WireGuard registration
        let wg_registered = wg_garage_repo::exists(&self.db, uuid)
            .await
            .unwrap_or(false);
        if !wg_registered {
            debug!(
                garage_id = %id_str,
                "pod ready but WireGuard not registered, staying in Initializing"
            );
            return false;
        }

        // Check repo cloned (init container succeeded)
        // Per garage-lifecycle.md spec: Repo cloned | Repository cloned to `/workspace/<repo-name>/`
        // Returns Some(true) if succeeded, Some(false) if failed/pending, None if no init container
        match self.k8s.init_container_succeeded(garage_id).await {
            Ok(Some(true)) => {
                // Init container succeeded - repo is cloned
                debug!(garage_id = %id_str, "init container (repo clone) succeeded");
            }
            Ok(Some(false)) => {
                // Init container not yet completed - stay in Initializing
                debug!(
                    garage_id = %id_str,
                    "pod ready but init container (repo clone) not complete, staying in Initializing"
                );
                return false;
            }
            Ok(None) => {
                // No init container configured - no repo to clone, skip this check
                debug!(garage_id = %id_str, "no init container configured, skipping repo clone check");
            }
            Err(e) => {
                // Error checking init container - log and skip this check to avoid blocking
                warn!(
                    garage_id = %id_str,
                    error = %e,
                    "failed to check init container status, skipping check"
                );
            }
        }

        // Check supporting services (if any were requested)
        // Per supporting-services.md: moto-club waits for deployments to be available
        if self.k8s.postgres_exists(garage_id).await.unwrap_or(false)
            && !self
                .k8s
                .postgres_available(garage_id)
                .await
                .unwrap_or(false)
        {
            debug!(
                garage_id = %id_str,
                "pod ready but Postgres not available, staying in Initializing"
            );
            return false;
        }

        if self.k8s.redis_exists(garage_id).await.unwrap_or(false)
            && !self.k8s.redis_available(garage_id).await.unwrap_or(false)
        {
            debug!(
                garage_id = %id_str,
                "pod ready but Redis not available, staying in Initializing"
            );
            return false;
        }

        true
    }
}

/// Determines if a status update should be applied.
///
/// We avoid some transitions that don't make sense:
/// - Ready → Initializing (would be a downgrade)
/// - Failed is a terminal state (only moves to Terminated)
#[allow(clippy::match_same_arms)] // Explicit arms are clearer for state machine transitions
const fn should_update_status(current: GarageStatus, new: GarageStatus) -> bool {
    match (current, new) {
        // Don't downgrade from Ready to Initializing
        (GarageStatus::Ready, GarageStatus::Initializing) => false,
        // Failed can only transition to Terminated (not back to other states)
        (
            GarageStatus::Failed,
            GarageStatus::Pending | GarageStatus::Initializing | GarageStatus::Ready,
        ) => false,
        // All other transitions are allowed
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_config_default() {
        let config = ReconcileConfig::default();
        assert_eq!(config.interval, Duration::from_secs(30));
        assert!(!config.delete_orphans);
    }

    #[test]
    fn reconcile_config_with_interval() {
        let config = ReconcileConfig::default().with_interval(Duration::from_secs(60));
        assert_eq!(config.interval, Duration::from_secs(60));
    }

    #[test]
    fn reconcile_config_with_delete_orphans() {
        let config = ReconcileConfig::default().with_delete_orphans(true);
        assert!(config.delete_orphans);
    }

    #[test]
    fn reconcile_stats_default() {
        let stats = ReconcileStats::default();
        assert_eq!(stats.checked, 0);
        assert_eq!(stats.updated, 0);
        assert_eq!(stats.terminated, 0);
        assert_eq!(stats.orphans, 0);
        assert_eq!(stats.orphans_deleted, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn should_update_status_allows_forward_progress() {
        // Normal forward progress
        assert!(should_update_status(
            GarageStatus::Pending,
            GarageStatus::Initializing
        ));
        assert!(should_update_status(
            GarageStatus::Initializing,
            GarageStatus::Ready
        ));
        // Failure transitions
        assert!(should_update_status(
            GarageStatus::Pending,
            GarageStatus::Failed
        ));
        assert!(should_update_status(
            GarageStatus::Initializing,
            GarageStatus::Failed
        ));
    }

    #[test]
    fn should_update_status_prevents_downgrades() {
        // Don't downgrade from Ready
        assert!(!should_update_status(
            GarageStatus::Ready,
            GarageStatus::Initializing
        ));
        // Failed is terminal - can't go back to other states
        assert!(!should_update_status(
            GarageStatus::Failed,
            GarageStatus::Pending
        ));
        assert!(!should_update_status(
            GarageStatus::Failed,
            GarageStatus::Initializing
        ));
        assert!(!should_update_status(
            GarageStatus::Failed,
            GarageStatus::Ready
        ));
    }

    #[test]
    fn should_update_status_allows_pending_restart() {
        // Allow going back to Pending (pod restart)
        assert!(should_update_status(
            GarageStatus::Initializing,
            GarageStatus::Pending
        ));
        assert!(should_update_status(
            GarageStatus::Ready,
            GarageStatus::Pending
        ));
    }

    #[test]
    fn should_update_status_allows_failed_to_terminated() {
        // Failed can transition to Terminated (cleanup)
        assert!(should_update_status(
            GarageStatus::Failed,
            GarageStatus::Terminated
        ));
    }
}
