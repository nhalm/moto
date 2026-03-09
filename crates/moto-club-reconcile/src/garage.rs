//! Garage reconciliation: K8s → Database.
//!
//! Synchronizes garage state between Kubernetes and the database.
//! K8s is the source of truth.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;
use tokio::time::interval;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use moto_club_db::{
    DbPool, GarageStatus, TerminationReason, audit_repo, garage_repo, wg_garage_repo,
};
use moto_club_k8s::{
    GarageK8s, GarageNamespaceOps, GaragePodOps, GaragePodStatus, GaragePostgresOps, GarageRedisOps,
};
use moto_club_types::GarageId;
use moto_club_ws::events::{EventBroadcaster, GarageEvent};
use moto_k8s::Labels;

/// TTL warning thresholds in minutes (15 and 5 minutes before expiry).
const TTL_WARNING_THRESHOLDS: &[u32] = &[15, 5];

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
    /// Number of garages terminated due to TTL expiry.
    pub ttl_expired: usize,
    /// Number of TTL warning events emitted.
    pub ttl_warnings: usize,
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

/// Restart count threshold for detecting crash loops.
const CRASH_LOOP_RESTART_THRESHOLD: i32 = 3;

/// Reconciles garage state between K8s and the database.
///
/// Runs periodically to ensure the database reflects the actual K8s state.
#[derive(Clone)]
pub struct GarageReconciler {
    db: DbPool,
    k8s: GarageK8s,
    config: ReconcileConfig,
    event_broadcaster: Option<Arc<EventBroadcaster>>,
    /// Tracks (`garage_id`, `threshold_minutes`) pairs for which warnings have been sent.
    ttl_warnings_sent: Arc<Mutex<HashSet<(Uuid, u32)>>>,
    /// Tracks garage IDs for which crash loop error events have been sent.
    crash_loop_errors_sent: Arc<Mutex<HashSet<Uuid>>>,
}

impl GarageReconciler {
    /// Creates a new reconciler.
    #[must_use]
    pub fn new(db: DbPool, k8s: GarageK8s, config: ReconcileConfig) -> Self {
        Self {
            db,
            k8s,
            config,
            event_broadcaster: None,
            ttl_warnings_sent: Arc::new(Mutex::new(HashSet::new())),
            crash_loop_errors_sent: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Sets the event broadcaster for emitting TTL warning and status change events.
    #[must_use]
    pub fn with_event_broadcaster(mut self, broadcaster: Arc<EventBroadcaster>) -> Self {
        self.event_broadcaster = Some(broadcaster);
        self
    }

    /// Broadcasts a `status_change` event if an event broadcaster is configured.
    fn emit_status_change(
        &self,
        owner: &str,
        garage_name: &str,
        from: GarageStatus,
        to: GarageStatus,
        reason: Option<&str>,
    ) {
        if let Some(ref broadcaster) = self.event_broadcaster {
            broadcaster.broadcast(
                owner,
                GarageEvent::StatusChange {
                    garage: garage_name.to_string(),
                    from: from.to_string(),
                    to: to.to_string(),
                    reason: reason.map(String::from),
                },
            );
        }
    }

    /// Broadcasts an `error` event if an event broadcaster is configured.
    fn emit_error(&self, owner: &str, garage_name: &str, message: &str) {
        if let Some(ref broadcaster) = self.event_broadcaster {
            broadcaster.broadcast(
                owner,
                GarageEvent::Error {
                    garage: garage_name.to_string(),
                    message: message.to_string(),
                },
            );
        }
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
                    if stats.updated > 0
                        || stats.terminated > 0
                        || stats.orphans > 0
                        || stats.ttl_expired > 0
                        || stats.ttl_warnings > 0
                    {
                        info!(
                            checked = stats.checked,
                            updated = stats.updated,
                            terminated = stats.terminated,
                            ttl_expired = stats.ttl_expired,
                            ttl_warnings = stats.ttl_warnings,
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

                if self.config.delete_orphans
                    && let Ok(garage_id) = id_str.parse::<Uuid>()
                {
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
                        self.emit_status_change(
                            &garage.owner,
                            &garage.name,
                            garage.status,
                            GarageStatus::Terminated,
                            Some("namespace_missing"),
                        );
                    }
                    Err(e) => {
                        warn!(garage_id = %id_str, error = %e, "failed to terminate garage");
                        stats.errors += 1;
                    }
                }
            }
        }

        // Step 5: Emit TTL warning events for garages approaching expiry
        self.emit_ttl_warnings(&mut stats).await;

        // Step 6: TTL enforcement — terminate expired garages and delete namespaces
        self.enforce_ttl(&mut stats).await;

        Ok(stats)
    }

    /// Reconciles a single garage's status from K8s pod state.
    #[allow(clippy::too_many_lines)]
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
                // Emit error event with failure details
                let message = self.get_pod_failure_message(&garage_id).await;
                self.emit_error(&garage.owner, &garage.name, &message);
                info!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    message = %message,
                    "emitted error event for pod failure"
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
                self.emit_status_change(
                    &garage.owner,
                    &garage.name,
                    garage.status,
                    GarageStatus::Terminated,
                    Some("pod_lost"),
                );
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
                self.emit_status_change(
                    &garage.owner,
                    &garage.name,
                    garage.status,
                    GarageStatus::Terminated,
                    Some("pod_lost"),
                );
                return Ok(());
            }
        };

        // Check for crash loops on non-ready, non-terminated pods
        if matches!(
            new_status,
            GarageStatus::Pending | GarageStatus::Initializing
        ) {
            self.check_crash_loop(&garage_id, uuid, &garage.owner, &garage.name)
                .await;
        } else {
            // Pod recovered or reached a terminal state — clear crash loop tracking
            self.crash_loop_errors_sent.lock().unwrap().remove(&uuid);
        }

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

            let reason = if new_status == GarageStatus::Failed {
                Some("error")
            } else {
                None
            };
            self.emit_status_change(
                &garage.owner,
                &garage.name,
                garage.status,
                new_status,
                reason,
            );

            // Audit log: garage_state_changed (best-effort)
            audit_repo::log_event(
                &self.db,
                audit_repo::InsertAuditEntry {
                    event_type: "garage_state_changed",
                    principal_type: "service",
                    principal_id: "moto-club-reconciler",
                    action: "update",
                    resource_type: "garage",
                    resource_id: id_str,
                    outcome: "success",
                    metadata: serde_json::json!({
                        "garage_name": garage.name,
                        "from": garage.status.to_string(),
                        "to": new_status.to_string(),
                    }),
                    client_ip: None,
                },
            )
            .await;
        }

        Ok(())
    }
}

impl GarageReconciler {
    /// Gets a descriptive failure message from a pod's container statuses.
    async fn get_pod_failure_message(&self, garage_id: &GarageId) -> String {
        let Ok(pod) = self.k8s.get_garage_pod(garage_id).await else {
            return "Pod failed".to_string();
        };

        // Check init container failures first
        if let Some(ref status) = pod.status
            && let Some(ref init_statuses) = status.init_container_statuses
        {
            for cs in init_statuses {
                if let Some(ref state) = cs.state
                    && let Some(ref terminated) = state.terminated
                    && terminated.exit_code != 0
                {
                    let reason = terminated.reason.as_deref().unwrap_or("unknown");
                    return format!(
                        "Init container '{}' failed: {} (exit code {})",
                        cs.name, reason, terminated.exit_code
                    );
                }
            }
        }

        // Check main container failures
        if let Some(ref status) = pod.status
            && let Some(ref container_statuses) = status.container_statuses
        {
            for cs in container_statuses {
                if let Some(ref state) = cs.state
                    && let Some(ref terminated) = state.terminated
                    && terminated.exit_code != 0
                {
                    let reason = terminated.reason.as_deref().unwrap_or("unknown");
                    return format!(
                        "Container '{}' failed: {} (exit code {})",
                        cs.name, reason, terminated.exit_code
                    );
                }
            }
        }

        "Pod failed".to_string()
    }

    /// Checks if a pod is in a crash loop and emits an error event if so.
    ///
    /// Detects crash loops by checking for `CrashLoopBackOff` waiting reason
    /// or restart count exceeding the threshold. Each garage is only warned once
    /// until the crash loop clears.
    async fn check_crash_loop(
        &self,
        garage_id: &GarageId,
        uuid: Uuid,
        owner: &str,
        garage_name: &str,
    ) {
        if self.event_broadcaster.is_none() {
            return;
        }

        // Skip if we already sent a crash loop error for this garage
        if self.crash_loop_errors_sent.lock().unwrap().contains(&uuid) {
            return;
        }

        let Ok(pod) = self.k8s.get_garage_pod(garage_id).await else {
            return;
        };

        let Some(ref status) = pod.status else {
            return;
        };

        let container_statuses = status.container_statuses.as_deref().unwrap_or_default();

        for cs in container_statuses {
            let is_crash_loop = cs
                .state
                .as_ref()
                .and_then(|s| s.waiting.as_ref())
                .is_some_and(|w| w.reason.as_deref() == Some("CrashLoopBackOff"));

            let high_restarts = cs.restart_count >= CRASH_LOOP_RESTART_THRESHOLD;

            if is_crash_loop || high_restarts {
                let message = format!(
                    "Pod crash loop detected (container '{}', {} restarts)",
                    cs.name, cs.restart_count
                );
                self.emit_error(owner, garage_name, &message);
                self.crash_loop_errors_sent.lock().unwrap().insert(uuid);

                info!(
                    garage_id = %garage_id,
                    garage_name = %garage_name,
                    container = %cs.name,
                    restart_count = cs.restart_count,
                    "emitted error event for crash loop"
                );
                return;
            }
        }
    }
}

impl GarageReconciler {
    /// Checks all ready criteria for a garage pod.
    ///
    /// Per garage-lifecycle.md and supporting-services.md specs, a garage is Ready when:
    /// 1. Pod running (containers ready) - already checked
    /// 2. Terminal daemon up (ttyd) - checked via container readiness probe
    /// 3. `WireGuard` registered
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

impl GarageReconciler {
    /// Enforces TTL on expired garages.
    ///
    /// Lists expired garages (oldest-first), terminates each in the DB,
    /// then deletes the K8s namespace. Processes at most 10 per cycle.
    async fn enforce_ttl(&self, stats: &mut ReconcileStats) {
        let expired = match garage_repo::list_expired(&self.db).await {
            Ok(garages) => garages,
            Err(e) => {
                warn!(error = %e, "failed to list expired garages for TTL enforcement");
                stats.errors += 1;
                return;
            }
        };

        // Rate limit: at most 10 per cycle
        for garage in expired.into_iter().take(10) {
            let id_str = garage.id.to_string();
            let garage_id = GarageId::from_uuid(garage.id);

            // Terminate in DB
            match garage_repo::terminate(&self.db, garage.id, TerminationReason::TtlExpired).await {
                Ok(_) => {
                    info!(
                        garage_id = %id_str,
                        garage_name = %garage.name,
                        reason = "ttl_expired",
                        "garage expired, terminated"
                    );
                    stats.ttl_expired += 1;
                    self.emit_status_change(
                        &garage.owner,
                        &garage.name,
                        garage.status,
                        GarageStatus::Terminated,
                        Some("ttl_expired"),
                    );

                    // Audit log: ttl_enforced (best-effort)
                    audit_repo::log_event(
                        &self.db,
                        audit_repo::InsertAuditEntry {
                            event_type: "ttl_enforced",
                            principal_type: "service",
                            principal_id: "moto-club-reconciler",
                            action: "delete",
                            resource_type: "garage",
                            resource_id: &id_str,
                            outcome: "success",
                            metadata: serde_json::json!({
                                "garage_name": garage.name,
                                "owner": garage.owner,
                                "previous_status": garage.status.to_string(),
                            }),
                            client_ip: None,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    warn!(
                        garage_id = %id_str,
                        garage_name = %garage.name,
                        error = %e,
                        "failed to terminate expired garage"
                    );
                    stats.errors += 1;
                    continue;
                }
            }

            // Delete K8s namespace (best-effort after DB termination)
            if let Err(e) = self.k8s.delete_garage_namespace(&garage_id).await {
                warn!(
                    garage_id = %id_str,
                    garage_name = %garage.name,
                    error = %e,
                    "failed to delete namespace for expired garage, orphan cleanup will retry"
                );
            }
        }
    }
}

impl GarageReconciler {
    /// Emits TTL warning events for garages approaching expiry.
    ///
    /// Checks for garages expiring within 15 and 5 minutes. Each (garage, threshold)
    /// pair is only warned once — tracked via `ttl_warnings_sent`.
    async fn emit_ttl_warnings(&self, stats: &mut ReconcileStats) {
        let Some(ref broadcaster) = self.event_broadcaster else {
            return;
        };

        // Query garages expiring within the largest threshold (15 minutes)
        let max_threshold = TTL_WARNING_THRESHOLDS.iter().copied().max().unwrap_or(15);
        let garages =
            match garage_repo::list_expiring_within(&self.db, max_threshold.cast_signed()).await {
                Ok(g) => g,
                Err(e) => {
                    warn!(error = %e, "failed to list garages for TTL warnings");
                    stats.errors += 1;
                    return;
                }
            };

        let now = Utc::now();
        let mut sent = self.ttl_warnings_sent.lock().unwrap();

        for garage in &garages {
            let remaining = garage.expires_at.signed_duration_since(now);
            let remaining_minutes = u32::try_from(remaining.num_minutes().max(0)).unwrap_or(0);

            for &threshold in TTL_WARNING_THRESHOLDS {
                if remaining_minutes < threshold && !sent.contains(&(garage.id, threshold)) {
                    let event = GarageEvent::TtlWarning {
                        garage: garage.name.clone(),
                        minutes_remaining: remaining_minutes,
                        expires_at: garage.expires_at.to_rfc3339(),
                    };

                    broadcaster.broadcast(&garage.owner, event);
                    sent.insert((garage.id, threshold));
                    stats.ttl_warnings += 1;

                    info!(
                        garage_name = %garage.name,
                        minutes_remaining = remaining_minutes,
                        threshold = threshold,
                        "emitted TTL warning event"
                    );
                }
            }
        }

        // Clean up entries for garages that have been terminated (no longer in the expiring list)
        let active_ids: HashSet<Uuid> = garages.iter().map(|g| g.id).collect();
        sent.retain(|(id, _)| active_ids.contains(id));
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
        assert_eq!(stats.ttl_expired, 0);
        assert_eq!(stats.ttl_warnings, 0);
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
