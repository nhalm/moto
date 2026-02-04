//! DERP (Designated Encrypted Relay for Packets) map management.
//!
//! This module manages the DERP relay server configuration that moto-club
//! provides to clients and garages for NAT traversal fallback.
//!
//! # Architecture
//!
//! moto-club maintains a DERP map that is returned to clients when creating
//! tunnel sessions. The DERP map contains regions with relay servers that
//! can be used when direct P2P connections fail.
//!
//! ```text
//! DERP Map Provider:
//!   - Store DERP regions and nodes
//!   - Provide map to clients in session responses
//!   - Support dynamic updates (add/remove regions)
//! ```
//!
//! # Configuration File
//!
//! DERP servers can be configured via a TOML file. Set the `MOTO_CLUB_DERP_CONFIG`
//! environment variable to the file path, or use the default `/etc/moto-club/derp.toml`.
//!
//! ```toml
//! [[regions]]
//! id = 1
//! name = "primary"
//!
//! [[regions.nodes]]
//! host = "derp.example.com"
//! port = 443
//! stun_port = 3478
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::derp::{DerpMapManager, InMemoryDerpStore};
//! use moto_wgtunnel_types::derp::{DerpMap, DerpRegion, DerpNode};
//!
//! // Create a manager with in-memory store
//! let store = InMemoryDerpStore::new();
//! let manager = DerpMapManager::new(store);
//!
//! // Add a DERP region
//! let region = DerpRegion::new(1, "primary")
//!     .with_node(DerpNode::with_defaults("derp.example.com"));
//!
//! manager.add_region(region).unwrap();
//!
//! // Get the current DERP map
//! let map = manager.get_map().unwrap();
//! assert_eq!(map.len(), 1);
//! ```

use moto_wgtunnel_types::derp::{
    DEFAULT_DERP_PORT, DEFAULT_STUN_PORT, DerpMap, DerpNode, DerpRegion,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Default path for DERP config file.
pub const DEFAULT_DERP_CONFIG_PATH: &str = "/etc/moto-club/derp.toml";

/// Environment variable for DERP config file path.
pub const DERP_CONFIG_ENV_VAR: &str = "MOTO_CLUB_DERP_CONFIG";

/// Error type for DERP map operations.
#[derive(Debug, thiserror::Error)]
pub enum DerpError {
    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Region not found.
    #[error("region not found: {0}")]
    RegionNotFound(u16),

    /// Invalid region configuration.
    #[error("invalid region: {0}")]
    InvalidRegion(String),

    /// Failed to read config file.
    #[error("failed to read config file: {0}")]
    ConfigReadError(String),

    /// Failed to parse config file.
    #[error("failed to parse config file: {0}")]
    ConfigParseError(String),
}

/// Result type for DERP map operations.
pub type Result<T> = std::result::Result<T, DerpError>;

// =============================================================================
// Config File Types
// =============================================================================

/// DERP configuration file format (TOML).
///
/// This matches the spec format:
/// ```toml
/// [[regions]]
/// id = 1
/// name = "primary"
///
/// [[regions.nodes]]
/// host = "derp.example.com"
/// port = 443
/// stun_port = 3478
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct DerpConfigFile {
    /// DERP regions.
    #[serde(default)]
    pub regions: Vec<DerpConfigRegion>,
}

/// A DERP region in the config file.
#[derive(Debug, Clone, Deserialize)]
pub struct DerpConfigRegion {
    /// Unique region identifier.
    pub id: u16,
    /// Human-readable region name.
    pub name: String,
    /// DERP nodes in this region.
    #[serde(default)]
    pub nodes: Vec<DerpConfigNode>,
}

/// A DERP node in the config file.
#[derive(Debug, Clone, Deserialize)]
pub struct DerpConfigNode {
    /// Hostname or IP address.
    pub host: String,
    /// DERP port (default 443).
    #[serde(default = "default_port")]
    pub port: u16,
    /// STUN port (default 3478).
    #[serde(default = "default_stun_port")]
    pub stun_port: u16,
}

const fn default_port() -> u16 {
    DEFAULT_DERP_PORT
}

const fn default_stun_port() -> u16 {
    DEFAULT_STUN_PORT
}

impl DerpConfigFile {
    /// Convert config file to a `DerpMap`.
    ///
    /// Regions without nodes are skipped (they would fail validation anyway).
    #[must_use]
    pub fn to_derp_map(&self) -> DerpMap {
        let mut map = DerpMap::new();
        for region in &self.regions {
            if region.nodes.is_empty() {
                continue;
            }
            let mut derp_region = DerpRegion::new(region.id, &region.name);
            for node in &region.nodes {
                derp_region.add_node(DerpNode::new(&node.host, node.port, node.stun_port));
            }
            map.add_region(derp_region);
        }
        map
    }
}

/// Load DERP configuration from the standard path.
///
/// Checks `MOTO_CLUB_DERP_CONFIG` environment variable first, then falls back
/// to `/etc/moto-club/derp.toml`.
///
/// # Returns
///
/// Returns `Ok(Some(config))` if the file exists and is valid.
/// Returns `Ok(None)` if the config file does not exist.
///
/// # Errors
///
/// Returns error if the file exists but cannot be read or parsed.
pub async fn load_derp_config() -> Result<Option<DerpConfigFile>> {
    let path =
        std::env::var(DERP_CONFIG_ENV_VAR).unwrap_or_else(|_| DEFAULT_DERP_CONFIG_PATH.to_string());
    load_derp_config_from_path(&path).await
}

/// Load DERP configuration from a specific path.
///
/// # Returns
///
/// Returns `Ok(Some(config))` if the file exists and is valid.
/// Returns `Ok(None)` if the file does not exist.
///
/// # Errors
///
/// Returns error if the file exists but cannot be read or parsed.
pub async fn load_derp_config_from_path(path: &str) -> Result<Option<DerpConfigFile>> {
    let path = Path::new(path);

    // Check if file exists
    if !path.exists() {
        return Ok(None);
    }

    // Read file contents
    let contents = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| DerpError::ConfigReadError(format!("{}: {}", path.display(), e)))?;

    // Parse TOML
    let config: DerpConfigFile = toml::from_str(&contents)
        .map_err(|e| DerpError::ConfigParseError(format!("{}: {}", path.display(), e)))?;

    Ok(Some(config))
}

/// Storage backend for DERP map management.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait DerpStore: Send + Sync {
    /// Get the entire DERP map.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_map(&self) -> Result<DerpMap>;

    /// Get a specific region by ID.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_region(&self, region_id: u16) -> Result<Option<DerpRegion>>;

    /// Store or update a region.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_region(&self, region: DerpRegion) -> Result<()>;

    /// Remove a region.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_region(&self, region_id: u16) -> Result<Option<DerpRegion>>;

    /// List all region IDs.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn list_region_ids(&self) -> Result<Vec<u16>>;
}

/// DERP map manager for maintaining relay server configuration.
///
/// The manager provides the DERP map that is returned to clients and garages
/// when creating tunnel sessions.
pub struct DerpMapManager<S> {
    store: S,
}

impl<S: DerpStore> DerpMapManager<S> {
    /// Create a new DERP map manager.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Get the current DERP map.
    ///
    /// Returns the complete map of all configured DERP regions and nodes.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_map(&self) -> Result<DerpMap> {
        self.store.get_map()
    }

    /// Get a specific region by ID.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_region(&self, region_id: u16) -> Result<Option<DerpRegion>> {
        self.store.get_region(region_id)
    }

    /// Add or update a DERP region.
    ///
    /// If a region with the same ID already exists, it will be replaced.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails or region is invalid.
    pub fn add_region(&self, region: DerpRegion) -> Result<()> {
        // Validate region has at least one node
        if region.nodes.is_empty() {
            return Err(DerpError::InvalidRegion(
                "region must have at least one node".to_string(),
            ));
        }

        self.store.set_region(region)
    }

    /// Add a node to an existing region.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails or region not found.
    pub fn add_node_to_region(&self, region_id: u16, node: DerpNode) -> Result<()> {
        let mut region = self
            .store
            .get_region(region_id)?
            .ok_or(DerpError::RegionNotFound(region_id))?;

        region.add_node(node);
        self.store.set_region(region)
    }

    /// Remove a DERP region.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn remove_region(&self, region_id: u16) -> Result<Option<DerpRegion>> {
        self.store.remove_region(region_id)
    }

    /// List all configured region IDs.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn list_region_ids(&self) -> Result<Vec<u16>> {
        self.store.list_region_ids()
    }

    /// Check if the DERP map is empty (no regions configured).
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.store.list_region_ids()?.is_empty())
    }

    /// Get the number of configured regions.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn region_count(&self) -> Result<usize> {
        Ok(self.store.list_region_ids()?.len())
    }
}

/// In-memory DERP store for testing.
///
/// Data is lost when the store is dropped.
pub struct InMemoryDerpStore {
    inner: Mutex<InMemoryDerpStoreInner>,
}

struct InMemoryDerpStoreInner {
    regions: HashMap<u16, DerpRegion>,
}

impl InMemoryDerpStore {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemoryDerpStoreInner {
                regions: HashMap::new(),
            }),
        }
    }

    /// Create a new store pre-populated with a default DERP map.
    ///
    /// This is useful for testing or development environments.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn with_default_map() -> Self {
        let store = Self::new();

        // Add a default "primary" region
        let region =
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.moto.dev"));

        store
            .inner
            .lock()
            .unwrap()
            .regions
            .insert(region.region_id, region);

        store
    }
}

impl Default for InMemoryDerpStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DerpStore for InMemoryDerpStore {
    fn get_map(&self) -> Result<DerpMap> {
        let regions: Vec<DerpRegion> = self
            .inner
            .lock()
            .unwrap()
            .regions
            .values()
            .cloned()
            .collect();

        let mut map = DerpMap::new();
        for region in regions {
            map.add_region(region);
        }
        Ok(map)
    }

    fn get_region(&self, region_id: u16) -> Result<Option<DerpRegion>> {
        Ok(self.inner.lock().unwrap().regions.get(&region_id).cloned())
    }

    fn set_region(&self, region: DerpRegion) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .regions
            .insert(region.region_id, region);
        Ok(())
    }

    fn remove_region(&self, region_id: u16) -> Result<Option<DerpRegion>> {
        Ok(self.inner.lock().unwrap().regions.remove(&region_id))
    }

    fn list_region_ids(&self) -> Result<Vec<u16>> {
        let mut ids: Vec<_> = self.inner.lock().unwrap().regions.keys().copied().collect();
        ids.sort_unstable();
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_manager() -> DerpMapManager<InMemoryDerpStore> {
        DerpMapManager::new(InMemoryDerpStore::new())
    }

    fn create_test_region(id: u16, name: &str) -> DerpRegion {
        DerpRegion::new(id, name).with_node(DerpNode::with_defaults("derp.example.com"))
    }

    #[test]
    fn empty_map() {
        let manager = create_manager();

        let map = manager.get_map().unwrap();
        assert!(map.is_empty());
        assert!(manager.is_empty().unwrap());
        assert_eq!(manager.region_count().unwrap(), 0);
    }

    #[test]
    fn add_region() {
        let manager = create_manager();

        let region = create_test_region(1, "primary");
        manager.add_region(region).unwrap();

        let map = manager.get_map().unwrap();
        assert_eq!(map.len(), 1);
        assert!(!manager.is_empty().unwrap());

        let retrieved = map.get_region(1).unwrap();
        assert_eq!(retrieved.name, "primary");
        assert_eq!(retrieved.nodes.len(), 1);
    }

    #[test]
    fn add_region_empty_nodes_fails() {
        let manager = create_manager();

        let region = DerpRegion::new(1, "primary"); // No nodes
        let err = manager.add_region(region).unwrap_err();

        assert!(matches!(err, DerpError::InvalidRegion(_)));
    }

    #[test]
    fn add_multiple_regions() {
        let manager = create_manager();

        manager
            .add_region(create_test_region(1, "us-west"))
            .unwrap();
        manager
            .add_region(create_test_region(2, "us-east"))
            .unwrap();
        manager
            .add_region(create_test_region(3, "eu-central"))
            .unwrap();

        assert_eq!(manager.region_count().unwrap(), 3);

        let ids = manager.list_region_ids().unwrap();
        assert_eq!(ids, vec![1, 2, 3]);

        let map = manager.get_map().unwrap();
        assert_eq!(map.get_region(1).unwrap().name, "us-west");
        assert_eq!(map.get_region(2).unwrap().name, "us-east");
        assert_eq!(map.get_region(3).unwrap().name, "eu-central");
    }

    #[test]
    fn update_region() {
        let manager = create_manager();

        // Add initial region
        manager
            .add_region(create_test_region(1, "primary"))
            .unwrap();

        // Update with new name and nodes
        let updated = DerpRegion::new(1, "primary-updated").with_node(DerpNode::new(
            "new.derp.com",
            443,
            3478,
        ));

        manager.add_region(updated).unwrap();

        // Should still be one region
        assert_eq!(manager.region_count().unwrap(), 1);

        let retrieved = manager.get_region(1).unwrap().unwrap();
        assert_eq!(retrieved.name, "primary-updated");
        assert_eq!(retrieved.nodes[0].host, "new.derp.com");
    }

    #[test]
    fn get_region() {
        let manager = create_manager();

        // Not found initially
        assert!(manager.get_region(1).unwrap().is_none());

        // Add and retrieve
        manager
            .add_region(create_test_region(1, "primary"))
            .unwrap();

        let region = manager.get_region(1).unwrap().unwrap();
        assert_eq!(region.name, "primary");

        // Non-existent still returns None
        assert!(manager.get_region(99).unwrap().is_none());
    }

    #[test]
    fn remove_region() {
        let manager = create_manager();

        manager
            .add_region(create_test_region(1, "primary"))
            .unwrap();
        manager
            .add_region(create_test_region(2, "secondary"))
            .unwrap();

        assert_eq!(manager.region_count().unwrap(), 2);

        // Remove one
        let removed = manager.remove_region(1).unwrap().unwrap();
        assert_eq!(removed.name, "primary");

        assert_eq!(manager.region_count().unwrap(), 1);
        assert!(manager.get_region(1).unwrap().is_none());
        assert!(manager.get_region(2).unwrap().is_some());

        // Remove non-existent returns None
        assert!(manager.remove_region(99).unwrap().is_none());
    }

    #[test]
    fn add_node_to_region() {
        let manager = create_manager();

        manager
            .add_region(create_test_region(1, "primary"))
            .unwrap();

        // Add another node
        manager
            .add_node_to_region(1, DerpNode::new("derp2.example.com", 443, 3478))
            .unwrap();

        let region = manager.get_region(1).unwrap().unwrap();
        assert_eq!(region.nodes.len(), 2);
        assert_eq!(region.nodes[1].host, "derp2.example.com");
    }

    #[test]
    fn add_node_to_missing_region_fails() {
        let manager = create_manager();

        let err = manager
            .add_node_to_region(99, DerpNode::with_defaults("derp.example.com"))
            .unwrap_err();

        assert!(matches!(err, DerpError::RegionNotFound(99)));
    }

    #[test]
    fn in_memory_store_with_default_map() {
        let store = InMemoryDerpStore::with_default_map();
        let manager = DerpMapManager::new(store);

        assert_eq!(manager.region_count().unwrap(), 1);

        let region = manager.get_region(1).unwrap().unwrap();
        assert_eq!(region.name, "primary");
        assert_eq!(region.nodes[0].host, "derp.moto.dev");
    }

    #[test]
    fn list_region_ids_sorted() {
        let manager = create_manager();

        // Add in non-sorted order
        manager
            .add_region(create_test_region(3, "region3"))
            .unwrap();
        manager
            .add_region(create_test_region(1, "region1"))
            .unwrap();
        manager
            .add_region(create_test_region(2, "region2"))
            .unwrap();

        let ids = manager.list_region_ids().unwrap();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn derp_map_returned_matches_stored() {
        let manager = create_manager();

        // Add regions with multiple nodes
        let region1 = DerpRegion::new(1, "us-west")
            .with_node(DerpNode::new("derp1.us-west.example.com", 443, 3478))
            .with_node(DerpNode::new("derp2.us-west.example.com", 443, 3478));

        let region2 = DerpRegion::new(2, "eu-central").with_node(DerpNode::new(
            "derp1.eu.example.com",
            8443,
            3479,
        ));

        manager.add_region(region1).unwrap();
        manager.add_region(region2).unwrap();

        let map = manager.get_map().unwrap();

        // Check region 1
        let r1 = map.get_region(1).unwrap();
        assert_eq!(r1.name, "us-west");
        assert_eq!(r1.nodes.len(), 2);
        assert_eq!(r1.nodes[0].host, "derp1.us-west.example.com");
        assert_eq!(r1.nodes[1].host, "derp2.us-west.example.com");

        // Check region 2
        let r2 = map.get_region(2).unwrap();
        assert_eq!(r2.name, "eu-central");
        assert_eq!(r2.nodes.len(), 1);
        assert_eq!(r2.nodes[0].host, "derp1.eu.example.com");
        assert_eq!(r2.nodes[0].port, 8443);
        assert_eq!(r2.nodes[0].stun_port, 3479);
    }

    #[test]
    fn concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(DerpMapManager::new(InMemoryDerpStore::new()));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let m = Arc::clone(&manager);
                thread::spawn(move || {
                    // i % 3 is always 0, 1, or 2; fits in u16
                    let region_id = u16::try_from(i % 3).unwrap() + 1;
                    let region = DerpRegion::new(region_id, format!("region-{region_id}"))
                        .with_node(DerpNode::with_defaults(format!("derp{i}.example.com")));
                    m.add_region(region).unwrap();

                    // Read back
                    let _ = m.get_map().unwrap();
                    let _ = m.list_region_ids().unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have at most 3 regions (IDs 1, 2, 3)
        let count = manager.region_count().unwrap();
        assert!(count <= 3);
        assert!(count > 0);
    }

    // Config file tests
    mod config {
        use super::*;

        #[test]
        fn parse_toml_config() {
            let toml = r#"
[[regions]]
id = 1
name = "primary"

[[regions.nodes]]
host = "derp.example.com"
port = 443
stun_port = 3478
"#;
            let config: DerpConfigFile = toml::from_str(toml).unwrap();

            assert_eq!(config.regions.len(), 1);
            assert_eq!(config.regions[0].id, 1);
            assert_eq!(config.regions[0].name, "primary");
            assert_eq!(config.regions[0].nodes.len(), 1);
            assert_eq!(config.regions[0].nodes[0].host, "derp.example.com");
            assert_eq!(config.regions[0].nodes[0].port, 443);
            assert_eq!(config.regions[0].nodes[0].stun_port, 3478);
        }

        #[test]
        fn parse_toml_config_default_ports() {
            let toml = r#"
[[regions]]
id = 1
name = "primary"

[[regions.nodes]]
host = "derp.example.com"
"#;
            let config: DerpConfigFile = toml::from_str(toml).unwrap();

            assert_eq!(config.regions[0].nodes[0].port, 443);
            assert_eq!(config.regions[0].nodes[0].stun_port, 3478);
        }

        #[test]
        fn parse_toml_config_multiple_regions() {
            let toml = r#"
[[regions]]
id = 1
name = "us-west"

[[regions.nodes]]
host = "derp1.us-west.example.com"

[[regions.nodes]]
host = "derp2.us-west.example.com"

[[regions]]
id = 2
name = "eu-central"

[[regions.nodes]]
host = "derp1.eu.example.com"
port = 8443
stun_port = 3479
"#;
            let config: DerpConfigFile = toml::from_str(toml).unwrap();

            assert_eq!(config.regions.len(), 2);

            assert_eq!(config.regions[0].id, 1);
            assert_eq!(config.regions[0].name, "us-west");
            assert_eq!(config.regions[0].nodes.len(), 2);

            assert_eq!(config.regions[1].id, 2);
            assert_eq!(config.regions[1].name, "eu-central");
            assert_eq!(config.regions[1].nodes.len(), 1);
            assert_eq!(config.regions[1].nodes[0].port, 8443);
        }

        #[test]
        fn config_to_derp_map() {
            let toml = r#"
[[regions]]
id = 1
name = "primary"

[[regions.nodes]]
host = "derp.example.com"
port = 443
stun_port = 3478
"#;
            let config: DerpConfigFile = toml::from_str(toml).unwrap();
            let map = config.to_derp_map();

            assert_eq!(map.len(), 1);
            let region = map.get_region(1).unwrap();
            assert_eq!(region.name, "primary");
            assert_eq!(region.nodes.len(), 1);
            assert_eq!(region.nodes[0].host, "derp.example.com");
        }

        #[test]
        fn config_to_derp_map_skips_empty_regions() {
            let toml = r#"
[[regions]]
id = 1
name = "empty"

[[regions]]
id = 2
name = "non-empty"

[[regions.nodes]]
host = "derp.example.com"
"#;
            let config: DerpConfigFile = toml::from_str(toml).unwrap();
            let map = config.to_derp_map();

            // Only one region should be in the map (empty ones skipped)
            assert_eq!(map.len(), 1);
            assert!(map.get_region(1).is_none());
            assert!(map.get_region(2).is_some());
        }

        #[test]
        fn parse_empty_config() {
            let toml = "";
            let config: DerpConfigFile = toml::from_str(toml).unwrap();

            assert!(config.regions.is_empty());
            let map = config.to_derp_map();
            assert!(map.is_empty());
        }

        #[tokio::test]
        async fn load_nonexistent_config_returns_none() {
            let result = load_derp_config_from_path("/nonexistent/path/derp.toml").await;
            assert!(result.is_ok());
            assert!(result.unwrap().is_none());
        }
    }
}
