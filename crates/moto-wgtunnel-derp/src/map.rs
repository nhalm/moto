//! DERP server map management.
//!
//! This module provides a manager for DERP server connections that handles:
//! - Tracking which DERP regions/nodes are available
//! - Selecting the best region to connect to
//! - Failover when connections fail
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_derp::map::{DerpMapManager, RegionStatus};
//! use moto_wgtunnel_types::derp::{DerpMap, DerpRegion, DerpNode};
//!
//! // Create a DERP map with multiple regions
//! let map = DerpMap::new()
//!     .with_region(
//!         DerpRegion::new(1, "primary")
//!             .with_node(DerpNode::with_defaults("derp1.example.com"))
//!     )
//!     .with_region(
//!         DerpRegion::new(2, "secondary")
//!             .with_node(DerpNode::with_defaults("derp2.example.com"))
//!     );
//!
//! // Create a manager with the map
//! let mut manager = DerpMapManager::new(map);
//!
//! // Select the next region to try
//! if let Some(region_id) = manager.select_region() {
//!     // Try connecting to this region...
//!     // On failure:
//!     manager.mark_failed(region_id);
//!     // On success:
//!     // manager.mark_connected(region_id);
//! }
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use moto_wgtunnel_types::derp::{DerpMap, DerpNode, DerpRegion};
use tracing::debug;

/// Default timeout before retrying a failed region.
pub const DEFAULT_RETRY_TIMEOUT: Duration = Duration::from_secs(30);

/// Status of a DERP region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionStatus {
    /// Region is available and can be tried.
    Available,

    /// Currently connected to this region.
    Connected,

    /// Region failed recently; will be retried after timeout.
    Failed {
        /// When the failure occurred.
        failed_at: Instant,
    },
}

impl RegionStatus {
    /// Check if the region is available for connection attempts.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    /// Check if the region is currently connected.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Check if the region has failed.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Per-region state tracking.
#[derive(Debug, Clone)]
struct RegionState {
    /// Current status.
    status: RegionStatus,

    /// Number of consecutive failures.
    failure_count: u32,

    /// Index of the current node being tried within the region.
    current_node_index: usize,
}

impl Default for RegionState {
    fn default() -> Self {
        Self {
            status: RegionStatus::Available,
            failure_count: 0,
            current_node_index: 0,
        }
    }
}

/// Manager for DERP server map with connection tracking and failover.
///
/// The manager tracks the status of each DERP region and provides methods
/// for selecting which region to connect to, handling failures, and
/// implementing retry logic.
#[derive(Debug, Clone)]
pub struct DerpMapManager {
    /// The underlying DERP map.
    map: DerpMap,

    /// Per-region state.
    regions: HashMap<u16, RegionState>,

    /// Timeout before retrying a failed region.
    retry_timeout: Duration,

    /// Preferred region ID (if set).
    preferred_region: Option<u16>,
}

impl DerpMapManager {
    /// Create a new manager from a DERP map.
    #[must_use]
    pub fn new(map: DerpMap) -> Self {
        let regions = map
            .regions()
            .keys()
            .map(|&id| (id, RegionState::default()))
            .collect();

        Self {
            map,
            regions,
            retry_timeout: DEFAULT_RETRY_TIMEOUT,
            preferred_region: None,
        }
    }

    /// Set the retry timeout for failed regions.
    #[must_use]
    pub const fn with_retry_timeout(mut self, timeout: Duration) -> Self {
        self.retry_timeout = timeout;
        self
    }

    /// Set the preferred region.
    ///
    /// The preferred region will be tried first when selecting a region.
    #[must_use]
    pub const fn with_preferred_region(mut self, region_id: u16) -> Self {
        self.preferred_region = Some(region_id);
        self
    }

    /// Get the underlying DERP map.
    #[must_use]
    pub const fn map(&self) -> &DerpMap {
        &self.map
    }

    /// Update the DERP map.
    ///
    /// This preserves connection state for regions that still exist.
    pub fn update_map(&mut self, new_map: DerpMap) {
        // Remove state for regions that no longer exist
        self.regions
            .retain(|id, _| new_map.get_region(*id).is_some());

        // Add state for new regions
        for &id in new_map.regions().keys() {
            self.regions.entry(id).or_default();
        }

        self.map = new_map;
    }

    /// Get the status of a region.
    #[must_use]
    pub fn region_status(&self, region_id: u16) -> Option<RegionStatus> {
        self.regions.get(&region_id).map(|s| s.status)
    }

    /// Check if a region is available for connection attempts.
    ///
    /// A region is available if:
    /// - It exists in the map
    /// - It has at least one node
    /// - It is not currently connected
    /// - It has not failed recently (or the retry timeout has elapsed)
    #[must_use]
    pub fn is_region_available(&self, region_id: u16) -> bool {
        let Some(state) = self.regions.get(&region_id) else {
            return false;
        };

        let Some(region) = self.map.get_region(region_id) else {
            return false;
        };

        if region.is_empty() {
            return false;
        }

        match state.status {
            RegionStatus::Available => true,
            RegionStatus::Connected => false,
            RegionStatus::Failed { failed_at } => failed_at.elapsed() >= self.retry_timeout,
        }
    }

    /// Select the next region to try connecting to.
    ///
    /// Selection priority:
    /// 1. Preferred region (if available)
    /// 2. First available region by ID
    ///
    /// Returns `None` if no regions are available.
    #[must_use]
    pub fn select_region(&self) -> Option<u16> {
        // First, check if the preferred region is available
        if let Some(preferred) = self.preferred_region
            && self.is_region_available(preferred)
        {
            debug!(region_id = preferred, "selecting preferred region");
            return Some(preferred);
        }

        // Otherwise, find the first available region
        let region_ids = self.map.region_ids();
        for region_id in region_ids {
            if self.is_region_available(region_id) {
                debug!(region_id, "selecting available region");
                return Some(region_id);
            }
        }

        debug!("no available regions");
        None
    }

    /// Get the currently connected region ID.
    #[must_use]
    pub fn connected_region(&self) -> Option<u16> {
        self.regions
            .iter()
            .find(|(_, state)| state.status.is_connected())
            .map(|(&id, _)| id)
    }

    /// Get the region for a given ID.
    #[must_use]
    pub fn get_region(&self, region_id: u16) -> Option<&DerpRegion> {
        self.map.get_region(region_id)
    }

    /// Get the current node to try for a region.
    ///
    /// Returns the node at the current index, or `None` if the region doesn't exist
    /// or has no nodes.
    #[must_use]
    pub fn current_node(&self, region_id: u16) -> Option<&DerpNode> {
        let state = self.regions.get(&region_id)?;
        let region = self.map.get_region(region_id)?;
        region.nodes.get(state.current_node_index)
    }

    /// Mark a region as connected.
    ///
    /// Disconnects any other connected region first.
    pub fn mark_connected(&mut self, region_id: u16) {
        // Disconnect any other connected region
        for (id, state) in &mut self.regions {
            if *id != region_id && state.status.is_connected() {
                state.status = RegionStatus::Available;
            }
        }

        // Mark this region as connected
        if let Some(state) = self.regions.get_mut(&region_id) {
            state.status = RegionStatus::Connected;
            state.failure_count = 0;
            debug!(region_id, "region connected");
        }
    }

    /// Mark a region as failed.
    ///
    /// The region will be unavailable until the retry timeout elapses.
    /// If the region has multiple nodes, advances to the next node.
    pub fn mark_failed(&mut self, region_id: u16) {
        let Some(state) = self.regions.get_mut(&region_id) else {
            return;
        };

        let Some(region) = self.map.get_region(region_id) else {
            return;
        };

        state.failure_count += 1;

        // Advance to the next node in the region
        let next_index = state.current_node_index + 1;
        if next_index < region.nodes.len() {
            // Try the next node in this region
            state.current_node_index = next_index;
            state.status = RegionStatus::Available;
            debug!(
                region_id,
                node_index = next_index,
                "advancing to next node in region"
            );
        } else {
            // All nodes in this region have been tried, mark region as failed
            state.current_node_index = 0; // Reset for next retry cycle
            state.status = RegionStatus::Failed {
                failed_at: Instant::now(),
            };
            debug!(
                region_id,
                failure_count = state.failure_count,
                "region marked as failed"
            );
        }
    }

    /// Mark a region as disconnected.
    ///
    /// The region becomes available for connection attempts again.
    pub fn mark_disconnected(&mut self, region_id: u16) {
        if let Some(state) = self.regions.get_mut(&region_id)
            && state.status.is_connected()
        {
            state.status = RegionStatus::Available;
            debug!(region_id, "region disconnected");
        }
    }

    /// Reset all region states to available.
    ///
    /// Useful when the user explicitly requests a reconnection.
    pub fn reset(&mut self) {
        for state in self.regions.values_mut() {
            *state = RegionState::default();
        }
        debug!("all regions reset to available");
    }

    /// Get the number of available regions.
    #[must_use]
    pub fn available_count(&self) -> usize {
        self.map
            .region_ids()
            .into_iter()
            .filter(|&id| self.is_region_available(id))
            .count()
    }

    /// Get the number of failed regions.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.regions
            .values()
            .filter(|s| s.status.is_failed())
            .count()
    }

    /// Check if all regions have failed.
    #[must_use]
    pub fn all_failed(&self) -> bool {
        !self.map.is_empty() && self.available_count() == 0 && self.connected_region().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_map() -> DerpMap {
        DerpMap::new()
            .with_region(
                DerpRegion::new(1, "primary")
                    .with_node(DerpNode::with_defaults("derp1a.example.com"))
                    .with_node(DerpNode::with_defaults("derp1b.example.com")),
            )
            .with_region(
                DerpRegion::new(2, "secondary")
                    .with_node(DerpNode::with_defaults("derp2.example.com")),
            )
    }

    #[test]
    fn new_manager() {
        let map = test_map();
        let manager = DerpMapManager::new(map);

        assert_eq!(manager.available_count(), 2);
        assert_eq!(manager.failed_count(), 0);
        assert!(manager.connected_region().is_none());
    }

    #[test]
    fn select_region_prefers_preferred() {
        let map = test_map();
        let manager = DerpMapManager::new(map).with_preferred_region(2);

        assert_eq!(manager.select_region(), Some(2));
    }

    #[test]
    fn select_region_falls_back_to_first() {
        let map = test_map();
        let manager = DerpMapManager::new(map);

        // Without preferred region, selects first by ID
        assert_eq!(manager.select_region(), Some(1));
    }

    #[test]
    fn mark_connected() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        manager.mark_connected(1);

        assert_eq!(manager.region_status(1), Some(RegionStatus::Connected));
        assert_eq!(manager.connected_region(), Some(1));
        assert_eq!(manager.available_count(), 1); // Only region 2 is available
    }

    #[test]
    fn mark_connected_disconnects_other() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        manager.mark_connected(1);
        manager.mark_connected(2);

        assert_eq!(manager.region_status(1), Some(RegionStatus::Available));
        assert_eq!(manager.region_status(2), Some(RegionStatus::Connected));
        assert_eq!(manager.connected_region(), Some(2));
    }

    #[test]
    fn mark_failed_advances_node() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        // Region 1 has 2 nodes
        assert_eq!(
            manager.current_node(1).map(|n| n.host.as_str()),
            Some("derp1a.example.com")
        );

        manager.mark_failed(1);

        // Should advance to second node, still available
        assert!(manager.is_region_available(1));
        assert_eq!(
            manager.current_node(1).map(|n| n.host.as_str()),
            Some("derp1b.example.com")
        );
    }

    #[test]
    fn mark_failed_marks_region_failed_after_all_nodes() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        // Fail both nodes in region 1
        manager.mark_failed(1);
        manager.mark_failed(1);

        // Region should now be failed
        assert!(!manager.is_region_available(1));
        assert!(manager.region_status(1).unwrap().is_failed());
    }

    #[test]
    fn mark_disconnected() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        manager.mark_connected(1);
        manager.mark_disconnected(1);

        assert_eq!(manager.region_status(1), Some(RegionStatus::Available));
        assert!(manager.connected_region().is_none());
    }

    #[test]
    fn failed_region_retries_after_timeout() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map).with_retry_timeout(Duration::from_millis(1));

        // Fail region 2 (single node, so it becomes failed)
        manager.mark_failed(2);
        assert!(!manager.is_region_available(2));

        // Wait for retry timeout
        std::thread::sleep(Duration::from_millis(5));

        // Should be available again
        assert!(manager.is_region_available(2));
    }

    #[test]
    fn all_failed() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        assert!(!manager.all_failed());

        // Fail all nodes in region 1
        manager.mark_failed(1);
        manager.mark_failed(1);
        assert!(!manager.all_failed()); // Region 2 still available

        // Fail region 2
        manager.mark_failed(2);
        assert!(manager.all_failed());
    }

    #[test]
    fn reset() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        manager.mark_connected(1);
        manager.mark_failed(2);
        manager.reset();

        assert_eq!(manager.available_count(), 2);
        assert!(manager.connected_region().is_none());
    }

    #[test]
    fn update_map_preserves_connected() {
        let map = test_map();
        let mut manager = DerpMapManager::new(map);

        manager.mark_connected(1);

        // Update map (keeping region 1)
        let new_map = DerpMap::new().with_region(
            DerpRegion::new(1, "primary")
                .with_node(DerpNode::with_defaults("derp1-new.example.com")),
        );
        manager.update_map(new_map);

        // Region 1 should still be connected
        assert_eq!(manager.connected_region(), Some(1));
        // Region 2 should be gone
        assert!(manager.region_status(2).is_none());
    }

    #[test]
    fn update_map_adds_new_regions() {
        let map = DerpMap::new()
            .with_region(DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("a.com")));
        let mut manager = DerpMapManager::new(map);

        let new_map = DerpMap::new()
            .with_region(DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("a.com")))
            .with_region(DerpRegion::new(2, "new").with_node(DerpNode::with_defaults("b.com")));
        manager.update_map(new_map);

        assert!(manager.is_region_available(2));
    }

    #[test]
    fn empty_region_not_available() {
        let map = DerpMap::new().with_region(DerpRegion::new(1, "empty")); // No nodes
        let manager = DerpMapManager::new(map);

        assert!(!manager.is_region_available(1));
        assert!(manager.select_region().is_none());
    }

    #[test]
    fn current_node_returns_correct_node() {
        let map = test_map();
        let manager = DerpMapManager::new(map);

        let node = manager.current_node(1).unwrap();
        assert_eq!(node.host, "derp1a.example.com");

        let node = manager.current_node(2).unwrap();
        assert_eq!(node.host, "derp2.example.com");

        assert!(manager.current_node(99).is_none());
    }
}
