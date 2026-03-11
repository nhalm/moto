//! Leader election for the reconciler using K8s Lease API.
//!
//! Ensures only one replica of moto-club runs reconciliation at a time.
//! Uses the coordination.k8s.io/v1 Lease API for distributed locking.

use std::time::Duration;

use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::Client;
use kube::api::{Api, Patch, PatchParams, PostParams};
use thiserror::Error;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

/// Errors from leader election operations.
#[derive(Debug, Error)]
pub enum LeaderElectionError {
    /// Kubernetes API error.
    #[error("kubernetes API error: {0}")]
    KubernetesApi(#[from] kube::Error),
}

/// Leader election configuration.
#[derive(Debug, Clone)]
pub struct LeaderElectionConfig {
    /// Lease duration - how long the lease is valid for.
    pub lease_duration: Duration,
    /// Renew deadline - how often to renew the lease.
    pub renew_deadline: Duration,
    /// Retry period - how often to retry acquiring the lease if not leader.
    pub retry_period: Duration,
    /// Namespace where the lease lives.
    pub namespace: String,
    /// Name of the lease resource.
    pub lease_name: String,
    /// Identity of this instance (pod name or unique ID).
    pub identity: String,
}

impl Default for LeaderElectionConfig {
    fn default() -> Self {
        Self {
            lease_duration: Duration::from_secs(15),
            renew_deadline: Duration::from_secs(10),
            retry_period: Duration::from_secs(2),
            namespace: "moto-system".to_string(),
            lease_name: "moto-club-reconciler".to_string(),
            identity: Self::default_identity(),
        }
    }
}

impl LeaderElectionConfig {
    /// Gets the default identity (pod name from env, or hostname).
    fn default_identity() -> String {
        std::env::var("POD_NAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "moto-club-unknown".to_string())
    }
}

/// Leader elector that uses K8s Lease API for distributed locking.
pub struct LeaderElector {
    client: Client,
    config: LeaderElectionConfig,
    is_leader: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl LeaderElector {
    /// Creates a new leader elector.
    #[must_use]
    pub fn new(client: Client, config: LeaderElectionConfig) -> Self {
        Self {
            client,
            config,
            is_leader: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Returns whether this instance is currently the leader.
    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.is_leader.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Runs the leader election loop.
    ///
    /// This runs forever, attempting to acquire and renew the lease.
    /// Updates the internal `is_leader` state based on lease ownership.
    pub async fn run(&self) {
        info!(
            identity = %self.config.identity,
            namespace = %self.config.namespace,
            lease_name = %self.config.lease_name,
            "starting leader election"
        );

        loop {
            if self.is_leader() {
                // We're the leader - try to renew the lease
                match self.try_renew_lease().await {
                    Ok(true) => {
                        debug!("lease renewed successfully");
                        sleep(self.config.renew_deadline).await;
                    }
                    Ok(false) => {
                        // Lost leadership
                        warn!("lost leadership, lease was acquired by another instance");
                        self.is_leader
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        sleep(self.config.retry_period).await;
                    }
                    Err(e) => {
                        error!(error = %e, "failed to renew lease, assuming leadership lost");
                        self.is_leader
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        sleep(self.config.retry_period).await;
                    }
                }
            } else {
                // We're not the leader - try to acquire the lease
                match self.try_acquire_lease().await {
                    Ok(true) => {
                        info!("acquired leadership");
                        self.is_leader
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        sleep(self.config.renew_deadline).await;
                    }
                    Ok(false) => {
                        debug!("lease held by another instance, retrying");
                        sleep(self.config.retry_period).await;
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to acquire lease, retrying");
                        sleep(self.config.retry_period).await;
                    }
                }
            }
        }
    }

    /// Attempts to acquire the lease.
    ///
    /// Returns:
    /// - `Ok(true)` if we acquired the lease
    /// - `Ok(false)` if the lease is held by another instance
    /// - `Err(_)` if there was an error communicating with K8s
    #[instrument(skip(self), fields(identity = %self.config.identity))]
    async fn try_acquire_lease(&self) -> Result<bool, LeaderElectionError> {
        let api: Api<Lease> = Api::namespaced(self.client.clone(), &self.config.namespace);

        // Try to get the existing lease
        match api.get(&self.config.lease_name).await {
            Ok(lease) => {
                // Lease exists - check if it's expired or held by us
                if self.can_acquire_lease(&lease) {
                    // Lease is expired or held by us - take it
                    self.update_lease(&api).await?;
                    Ok(true)
                } else {
                    // Lease is held by another instance
                    Ok(false)
                }
            }
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                // Lease doesn't exist - create it
                self.create_lease(&api).await?;
                Ok(true)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Attempts to renew the lease.
    ///
    /// Returns:
    /// - `Ok(true)` if we renewed the lease
    /// - `Ok(false)` if the lease is held by another instance
    /// - `Err(_)` if there was an error communicating with K8s
    #[instrument(skip(self), fields(identity = %self.config.identity))]
    async fn try_renew_lease(&self) -> Result<bool, LeaderElectionError> {
        let api: Api<Lease> = Api::namespaced(self.client.clone(), &self.config.namespace);

        // Get the current lease
        match api.get(&self.config.lease_name).await {
            Ok(lease) => {
                // Check if we still hold the lease
                if self.holds_lease(&lease) {
                    // We hold the lease - renew it
                    self.update_lease(&api).await?;
                    Ok(true)
                } else {
                    // Lease is held by another instance
                    Ok(false)
                }
            }
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => {
                // Lease was deleted - we lost leadership
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Checks if we can acquire the lease (it's expired or held by us).
    fn can_acquire_lease(&self, lease: &Lease) -> bool {
        if self.holds_lease(lease) {
            return true;
        }

        // Check if the lease is expired
        if let Some(spec) = &lease.spec
            && let Some(renew_time) = &spec.renew_time
        {
            let now = chrono::Utc::now();
            let elapsed = now.signed_duration_since(renew_time.0);
            let lease_duration_secs =
                i64::try_from(self.config.lease_duration.as_secs()).unwrap_or(i64::MAX);
            return elapsed.num_seconds() > lease_duration_secs;
        }

        false
    }

    /// Checks if we hold the lease.
    fn holds_lease(&self, lease: &Lease) -> bool {
        lease
            .spec
            .as_ref()
            .and_then(|spec| spec.holder_identity.as_ref())
            .is_some_and(|holder| holder == &self.config.identity)
    }

    /// Creates the lease.
    async fn create_lease(&self, api: &Api<Lease>) -> Result<(), LeaderElectionError> {
        let now = chrono::Utc::now();
        let micro_time = MicroTime(now);

        let lease = Lease {
            metadata: ObjectMeta {
                name: Some(self.config.lease_name.clone()),
                namespace: Some(self.config.namespace.clone()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::coordination::v1::LeaseSpec {
                holder_identity: Some(self.config.identity.clone()),
                lease_duration_seconds: Some(
                    i32::try_from(self.config.lease_duration.as_secs()).unwrap_or(15),
                ),
                acquire_time: Some(micro_time.clone()),
                renew_time: Some(micro_time),
                ..Default::default()
            }),
        };

        api.create(&PostParams::default(), &lease).await?;

        debug!("created lease");
        Ok(())
    }

    /// Updates the lease (renews it).
    async fn update_lease(&self, api: &Api<Lease>) -> Result<(), LeaderElectionError> {
        let now = chrono::Utc::now();
        let renew_time_str = now.to_rfc3339();

        let patch = serde_json::json!({
            "spec": {
                "holderIdentity": self.config.identity,
                "leaseDurationSeconds": i32::try_from(self.config.lease_duration.as_secs()).unwrap_or(15),
                "renewTime": renew_time_str,
            }
        });

        api.patch(
            &self.config.lease_name,
            &PatchParams::default(),
            &Patch::Merge(&patch),
        )
        .await?;

        Ok(())
    }
}
