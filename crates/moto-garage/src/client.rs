//! Garage client for managing dev environments.

use chrono::{DateTime, Duration, Utc};
use moto_club_types::{GarageId, GarageInfo, GarageState};
use moto_k8s::{K8sClient, Labels, LogStream, NamespaceOps, PodLogOptions, PodOps};
use tracing::{debug, instrument};

use crate::{Error, GarageMode, Result};

/// Client for garage operations.
///
/// Abstracts the difference between local (direct K8s) and remote (via club)
/// modes, providing the same interface for both.
pub struct GarageClient {
    mode: GarageMode,
    k8s: Option<K8sClient>,
}

impl GarageClient {
    /// Creates a new garage client in local mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the K8s client cannot be created.
    pub async fn local() -> Result<Self> {
        let k8s = K8sClient::new().await?;
        Ok(Self {
            mode: GarageMode::Local,
            k8s: Some(k8s),
        })
    }

    /// Creates a new garage client in local mode for a specific kubeconfig context.
    ///
    /// # Errors
    ///
    /// Returns an error if the K8s client cannot be created or the context doesn't exist.
    pub async fn local_with_context(context: &str) -> Result<Self> {
        let k8s = K8sClient::with_context(context).await?;
        Ok(Self {
            mode: GarageMode::Local,
            k8s: Some(k8s),
        })
    }

    /// Creates a new garage client in remote mode.
    ///
    /// Note: Remote mode is not yet implemented.
    #[must_use]
    pub fn remote(endpoint: impl Into<String>) -> Self {
        Self {
            mode: GarageMode::remote(endpoint),
            k8s: None,
        }
    }

    /// Creates a garage client from an existing K8s client (for testing).
    #[must_use]
    pub const fn from_k8s(k8s: K8sClient) -> Self {
        Self {
            mode: GarageMode::Local,
            k8s: Some(k8s),
        }
    }

    /// Returns the operating mode.
    #[must_use]
    pub const fn mode(&self) -> &GarageMode {
        &self.mode
    }

    /// Lists all garages.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    #[instrument(skip(self))]
    pub async fn list(&self) -> Result<Vec<GarageInfo>> {
        match &self.mode {
            GarageMode::Local => self.list_local().await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Opens (creates) a new garage with the given name.
    ///
    /// # Arguments
    ///
    /// * `name` - Human-friendly name for the garage.
    /// * `owner` - Optional owner identifier.
    /// * `ttl_seconds` - Optional time-to-live in seconds.
    /// * `engine` - Optional engine name (what the garage is working on).
    ///
    /// # Errors
    ///
    /// Returns an error if a garage with this name already exists or creation fails.
    #[instrument(skip(self, name), fields(garage.name = %name))]
    pub async fn open(
        &self,
        name: &str,
        owner: Option<&str>,
        ttl_seconds: Option<i64>,
        engine: Option<&str>,
    ) -> Result<GarageInfo> {
        match &self.mode {
            GarageMode::Local => self.open_local(name, owner, ttl_seconds, engine).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Closes (deletes) a garage by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist or deletion fails.
    #[instrument(skip(self), fields(garage.id = %id))]
    pub async fn close(&self, id: &GarageId) -> Result<()> {
        match &self.mode {
            GarageMode::Local => self.close_local(id).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Closes (deletes) a garage by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist or deletion fails.
    #[instrument(skip(self), fields(garage.name = %name))]
    pub async fn close_by_name(&self, name: &str) -> Result<()> {
        match &self.mode {
            GarageMode::Local => self.close_by_name_local(name).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Gets logs from a garage.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the garage to get logs from.
    /// * `tail_lines` - Number of lines to show from the end (None = all).
    /// * `since_seconds` - Only return logs from last N seconds (None = all).
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist or logs cannot be fetched.
    #[instrument(skip(self), fields(garage.name = %name))]
    pub async fn logs(
        &self,
        name: &str,
        tail_lines: Option<i64>,
        since_seconds: Option<i64>,
    ) -> Result<String> {
        match &self.mode {
            GarageMode::Local => self.logs_local(name, tail_lines, since_seconds).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Streams logs from a garage continuously.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the garage to stream logs from.
    /// * `tail_lines` - Number of lines to show from the end before streaming.
    /// * `since_seconds` - Only return logs from last N seconds initially.
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist or stream cannot be started.
    #[instrument(skip(self), fields(garage.name = %name))]
    pub async fn logs_stream(
        &self,
        name: &str,
        tail_lines: Option<i64>,
        since_seconds: Option<i64>,
    ) -> Result<LogStream> {
        match &self.mode {
            GarageMode::Local => {
                self.logs_stream_local(name, tail_lines, since_seconds)
                    .await
            }
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Extends a garage's TTL by the specified number of seconds.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the garage to extend.
    /// * `seconds` - Number of seconds to add to the current expiry.
    ///
    /// # Returns
    ///
    /// Returns the updated `GarageInfo` with the new expiration time.
    ///
    /// # Errors
    ///
    /// Returns an error if the garage doesn't exist, has expired, or the extension
    /// would exceed the maximum TTL (48 hours).
    #[instrument(skip(self), fields(garage.name = %name))]
    pub async fn extend(&self, name: &str, seconds: i64) -> Result<GarageInfo> {
        match &self.mode {
            GarageMode::Local => self.extend_local(name, seconds).await,
            GarageMode::Remote { .. } => Err(Error::RemoteNotImplemented),
        }
    }

    /// Lists garages in local mode.
    async fn list_local(&self) -> Result<Vec<GarageInfo>> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        debug!("listing garages from K8s");
        let namespaces = k8s
            .list_namespaces(Some(&Labels::garage_selector()))
            .await?;

        let mut garages = Vec::new();
        for ns in namespaces {
            if let Some(info) = namespace_to_garage_info(&ns) {
                garages.push(info);
            }
        }

        // Sort by creation time (newest first)
        garages.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(garages)
    }

    /// Opens a garage in local mode.
    async fn open_local(
        &self,
        name: &str,
        owner: Option<&str>,
        ttl_seconds: Option<i64>,
        engine: Option<&str>,
    ) -> Result<GarageInfo> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Check if garage with this name already exists
        let existing = self.list_local().await?;
        if existing.iter().any(|g| g.name == name) {
            return Err(Error::GarageExists(name.to_string()));
        }

        // Create new garage info
        let mut info = GarageInfo::new(name);
        if let Some(owner) = owner {
            info = info.with_owner(owner);
        }
        if let Some(engine) = engine {
            info = info.with_engine(engine);
        }

        // Calculate expiration time if TTL provided
        let expires_at =
            ttl_seconds.and_then(|secs| Duration::try_seconds(secs).map(|d| Utc::now() + d));
        if let Some(expires) = expires_at {
            info = info.with_expires_at(expires);
        }

        // Format expires_at as RFC 3339 for label
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        // Create namespace with labels
        let labels = Labels::for_garage(
            &info.id.to_string(),
            &info.name,
            info.owner.as_deref(),
            expires_at_str.as_deref(),
            info.engine.as_deref(),
        );

        debug!(namespace = %info.namespace, "creating garage namespace");
        k8s.create_namespace(&info.namespace, labels).await?;

        Ok(info)
    }

    /// Closes a garage in local mode.
    async fn close_local(&self, id: &GarageId) -> Result<()> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Find the garage by ID
        let garages = self.list_local().await?;
        let garage = garages
            .iter()
            .find(|g| &g.id == id)
            .ok_or_else(|| Error::GarageNotFound(id.to_string()))?;

        debug!(namespace = %garage.namespace, "deleting garage namespace");
        k8s.delete_namespace(&garage.namespace).await?;

        Ok(())
    }

    /// Closes a garage by name in local mode.
    async fn close_by_name_local(&self, name: &str) -> Result<()> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Find the garage by name
        let garages = self.list_local().await?;
        let garage = garages
            .iter()
            .find(|g| g.name == name)
            .ok_or_else(|| Error::GarageNotFound(name.to_string()))?;

        debug!(namespace = %garage.namespace, "deleting garage namespace");
        k8s.delete_namespace(&garage.namespace).await?;

        Ok(())
    }

    /// Gets logs from a garage in local mode.
    async fn logs_local(
        &self,
        name: &str,
        tail_lines: Option<i64>,
        since_seconds: Option<i64>,
    ) -> Result<String> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Find the garage by name
        let garages = self.list_local().await?;
        let garage = garages
            .iter()
            .find(|g| g.name == name)
            .ok_or_else(|| Error::GarageNotFound(name.to_string()))?;

        let options = PodLogOptions {
            tail_lines,
            since_seconds,
            follow: false,
        };

        debug!(namespace = %garage.namespace, "fetching garage logs");
        let logs = k8s.get_pod_logs(&garage.namespace, None, &options).await?;

        Ok(logs)
    }

    /// Streams logs from a garage in local mode.
    async fn logs_stream_local(
        &self,
        name: &str,
        tail_lines: Option<i64>,
        since_seconds: Option<i64>,
    ) -> Result<LogStream> {
        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Find the garage by name
        let garages = self.list_local().await?;
        let garage = garages
            .iter()
            .find(|g| g.name == name)
            .ok_or_else(|| Error::GarageNotFound(name.to_string()))?;

        let options = PodLogOptions {
            tail_lines,
            since_seconds,
            follow: true,
        };

        debug!(namespace = %garage.namespace, "starting garage log stream");
        let stream = k8s
            .stream_pod_logs(&garage.namespace, None, &options)
            .await?;

        Ok(stream)
    }

    /// Extends a garage's TTL in local mode.
    async fn extend_local(&self, name: &str, seconds: i64) -> Result<GarageInfo> {
        const MAX_TTL_SECONDS: i64 = 48 * 3600; // 48 hours

        let k8s = self
            .k8s
            .as_ref()
            .expect("k8s client required for local mode");

        // Validate extension amount
        if seconds <= 0 {
            return Err(Error::InvalidTtl("extension must be positive".to_string()));
        }

        // Find the garage by name
        let garages = self.list_local().await?;
        let garage = garages
            .into_iter()
            .find(|g| g.name == name)
            .ok_or_else(|| Error::GarageNotFound(name.to_string()))?;

        // Check if garage has expired
        let now = Utc::now();
        if let Some(expires_at) = garage.expires_at {
            if expires_at < now {
                return Err(Error::GarageExpired(name.to_string()));
            }
        }

        // Calculate new expiration time
        let current_expires = garage.expires_at.unwrap_or(now);
        let extension = Duration::try_seconds(seconds)
            .ok_or_else(|| Error::InvalidTtl("invalid extension duration".to_string()))?;
        let new_expires = current_expires + extension;

        // Check if new TTL exceeds maximum
        let new_ttl_seconds = (new_expires - garage.created_at).num_seconds();
        if new_ttl_seconds > MAX_TTL_SECONDS {
            return Err(Error::InvalidTtl(format!(
                "total TTL would be {new_ttl_seconds}s, which exceeds maximum of {MAX_TTL_SECONDS}s"
            )));
        }

        // Build labels with updated expires_at
        let labels = Labels::for_garage(
            &garage.id.to_string(),
            &garage.name,
            garage.owner.as_deref(),
            Some(&new_expires.to_rfc3339()),
            garage.engine.as_deref(),
        );

        debug!(namespace = %garage.namespace, new_expires = %new_expires, "extending garage TTL");
        k8s.patch_namespace_labels(&garage.namespace, labels)
            .await?;

        // Return updated garage info
        Ok(GarageInfo {
            expires_at: Some(new_expires),
            ..garage
        })
    }
}

/// Converts a K8s namespace to `GarageInfo`.
///
/// Returns `None` if the namespace doesn't have the required moto labels.
fn namespace_to_garage_info(ns: &k8s_openapi::api::core::v1::Namespace) -> Option<GarageInfo> {
    let metadata = &ns.metadata;
    let labels = metadata.labels.as_ref()?;

    // Required labels
    let id_str = labels.get(Labels::GARAGE_ID)?;
    let name = labels.get(Labels::GARAGE_NAME)?;

    let id: GarageId = id_str.parse().ok()?;
    let namespace = metadata.name.clone()?;

    // Optional owner
    let owner = labels.get(Labels::OWNER).cloned();

    // Optional engine
    let engine = labels.get(Labels::ENGINE).cloned();

    // Optional expiration time (RFC 3339 format)
    let expires_at = labels
        .get(Labels::EXPIRES_AT)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Creation time from K8s metadata
    let created_at = metadata
        .creation_timestamp
        .as_ref()
        .map_or_else(Utc::now, |ts| ts.0);

    // Determine state based on namespace status
    // Note: K8s namespace phase only tells us if namespace is active or terminating.
    // For full garage state (Pending/Initializing/Ready/Failed), we need DB info.
    // This is a simplified mapping for local mode without DB access.
    let state = match &ns.status {
        Some(status) => match status.phase.as_deref() {
            Some("Active") => GarageState::Ready,
            Some("Terminating") => GarageState::Terminated,
            _ => GarageState::Pending,
        },
        None => GarageState::Pending,
    };

    Some(GarageInfo {
        id,
        name: name.clone(),
        namespace,
        state,
        created_at,
        expires_at,
        owner,
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_client_mode() {
        let client = GarageClient::remote("https://club.example.com");
        assert!(client.mode().is_remote());
        assert_eq!(client.mode().endpoint(), Some("https://club.example.com"));
    }

    // Integration tests require a running K8s cluster
    // They are marked #[ignore] and can be run with:
    // cargo test --package moto-garage -- --ignored

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn local_client_creation() {
        let client = GarageClient::local().await;
        assert!(
            client.is_ok(),
            "Failed to create client: {:?}",
            client.err()
        );
        assert!(client.unwrap().mode().is_local());
    }

    #[tokio::test]
    #[ignore = "requires running K8s cluster"]
    async fn list_empty() {
        let client = GarageClient::local().await.expect("client creation failed");
        let garages = client.list().await;
        assert!(
            garages.is_ok(),
            "Failed to list garages: {:?}",
            garages.err()
        );
    }
}
