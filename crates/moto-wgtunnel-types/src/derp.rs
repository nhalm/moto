//! DERP (Designated Encrypted Relay for Packets) map types.
//!
//! DERP provides relay when direct P2P `WireGuard` connections fail due to NAT.
//! Traffic is already `WireGuard`-encrypted before reaching DERP, so DERP only
//! forwards opaque encrypted packets and never sees plaintext.
//!
//! # How DERP Works
//! 1. Both peers connect to a DERP server via HTTPS WebSocket
//! 2. Traffic is already `WireGuard`-encrypted before reaching DERP
//! 3. DERP forwards opaque encrypted packets
//! 4. DERP never sees plaintext
//!
//! # Failover
//! - If primary DERP region fails, try other regions
//! - If all DERP servers fail, connection fails
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_types::derp::{DerpMap, DerpRegion, DerpNode};
//!
//! // Create a DERP node
//! let node = DerpNode::new("derp.example.com", 443, 3478);
//!
//! // Create a DERP region with nodes
//! let region = DerpRegion::new(1, "primary")
//!     .with_node(node);
//!
//! // Create a DERP map with regions
//! let derp_map = DerpMap::new()
//!     .with_region(region);
//!
//! assert!(derp_map.get_region(1).is_some());
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default DERP port (HTTPS).
pub const DEFAULT_DERP_PORT: u16 = 443;

/// Default STUN port.
pub const DEFAULT_STUN_PORT: u16 = 3478;

/// A map of DERP regions for relay fallback.
///
/// The DERP map is provided by moto-club when creating tunnel sessions
/// and contains all available DERP relay servers organized by region.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DerpMap {
    /// Regions indexed by region ID.
    regions: HashMap<u16, DerpRegion>,
}

impl DerpMap {
    /// Create an empty DERP map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a region to the map.
    #[must_use]
    pub fn with_region(mut self, region: DerpRegion) -> Self {
        self.regions.insert(region.region_id, region);
        self
    }

    /// Add a region to the map (mutating).
    pub fn add_region(&mut self, region: DerpRegion) {
        self.regions.insert(region.region_id, region);
    }

    /// Get a region by ID.
    #[must_use]
    pub fn get_region(&self, region_id: u16) -> Option<&DerpRegion> {
        self.regions.get(&region_id)
    }

    /// Get all regions.
    #[must_use]
    pub const fn regions(&self) -> &HashMap<u16, DerpRegion> {
        &self.regions
    }

    /// Get region IDs in sorted order (for deterministic iteration).
    #[must_use]
    pub fn region_ids(&self) -> Vec<u16> {
        let mut ids: Vec<_> = self.regions.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Check if the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Get the number of regions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.regions.len()
    }
}

/// A DERP region containing one or more DERP nodes.
///
/// Regions typically represent geographic locations. Nodes within a region
/// are used for load balancing and redundancy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerpRegion {
    /// Unique region identifier.
    #[serde(rename = "region_id")]
    pub region_id: u16,

    /// Human-readable region name (e.g., "primary", "us-west", "eu-central").
    pub name: String,

    /// DERP nodes in this region.
    pub nodes: Vec<DerpNode>,
}

impl DerpRegion {
    /// Create a new DERP region.
    ///
    /// # Arguments
    /// - `region_id`: Unique identifier for this region
    /// - `name`: Human-readable name
    #[must_use]
    pub fn new(region_id: u16, name: impl Into<String>) -> Self {
        Self {
            region_id,
            name: name.into(),
            nodes: Vec::new(),
        }
    }

    /// Add a node to this region.
    #[must_use]
    pub fn with_node(mut self, node: DerpNode) -> Self {
        self.nodes.push(node);
        self
    }

    /// Add a node to this region (mutating).
    pub fn add_node(&mut self, node: DerpNode) {
        self.nodes.push(node);
    }

    /// Check if this region has any nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the number of nodes in this region.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

/// A single DERP relay node.
///
/// Each node has a hostname and two ports:
/// - DERP port: For relayed traffic (typically 443 for HTTPS)
/// - STUN port: For NAT discovery (typically 3478)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerpNode {
    /// Hostname or IP address of the DERP server.
    pub host: String,

    /// DERP port (typically 443 for HTTPS WebSocket).
    #[serde(default = "default_derp_port")]
    pub port: u16,

    /// STUN port for NAT discovery (typically 3478).
    #[serde(default = "default_stun_port")]
    pub stun_port: u16,
}

const fn default_derp_port() -> u16 {
    DEFAULT_DERP_PORT
}

const fn default_stun_port() -> u16 {
    DEFAULT_STUN_PORT
}

impl DerpNode {
    /// Create a new DERP node.
    ///
    /// # Arguments
    /// - `host`: Hostname or IP address
    /// - `port`: DERP port (typically 443)
    /// - `stun_port`: STUN port (typically 3478)
    #[must_use]
    pub fn new(host: impl Into<String>, port: u16, stun_port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            stun_port,
        }
    }

    /// Create a new DERP node with default ports (443 and 3478).
    #[must_use]
    pub fn with_defaults(host: impl Into<String>) -> Self {
        Self::new(host, DEFAULT_DERP_PORT, DEFAULT_STUN_PORT)
    }

    /// Get the DERP URL (for WebSocket connection).
    ///
    /// Returns URL in format `https://{host}:{port}` or `https://{host}` if port is 443.
    #[must_use]
    pub fn derp_url(&self) -> String {
        if self.port == DEFAULT_DERP_PORT {
            format!("https://{}", self.host)
        } else {
            format!("https://{}:{}", self.host, self.port)
        }
    }

    /// Get the STUN address as `host:port`.
    #[must_use]
    pub fn stun_addr(&self) -> String {
        format!("{}:{}", self.host, self.stun_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derp_node_new() {
        let node = DerpNode::new("derp.example.com", 443, 3478);

        assert_eq!(node.host, "derp.example.com");
        assert_eq!(node.port, 443);
        assert_eq!(node.stun_port, 3478);
    }

    #[test]
    fn derp_node_with_defaults() {
        let node = DerpNode::with_defaults("derp.example.com");

        assert_eq!(node.host, "derp.example.com");
        assert_eq!(node.port, DEFAULT_DERP_PORT);
        assert_eq!(node.stun_port, DEFAULT_STUN_PORT);
    }

    #[test]
    fn derp_node_derp_url() {
        let node = DerpNode::new("derp.example.com", 443, 3478);
        assert_eq!(node.derp_url(), "https://derp.example.com");

        let node_custom_port = DerpNode::new("derp.example.com", 8443, 3478);
        assert_eq!(node_custom_port.derp_url(), "https://derp.example.com:8443");
    }

    #[test]
    fn derp_node_stun_addr() {
        let node = DerpNode::new("derp.example.com", 443, 3478);
        assert_eq!(node.stun_addr(), "derp.example.com:3478");
    }

    #[test]
    fn derp_node_serde() {
        let node = DerpNode::new("derp.example.com", 443, 3478);
        let json = serde_json::to_string(&node).unwrap();
        let node2: DerpNode = serde_json::from_str(&json).unwrap();

        assert_eq!(node, node2);
    }

    #[test]
    fn derp_node_serde_default_ports() {
        // Deserializing without ports should use defaults
        let json = r#"{"host":"derp.example.com"}"#;
        let node: DerpNode = serde_json::from_str(json).unwrap();

        assert_eq!(node.host, "derp.example.com");
        assert_eq!(node.port, DEFAULT_DERP_PORT);
        assert_eq!(node.stun_port, DEFAULT_STUN_PORT);
    }

    #[test]
    fn derp_region_new() {
        let region = DerpRegion::new(1, "primary");

        assert_eq!(region.region_id, 1);
        assert_eq!(region.name, "primary");
        assert!(region.is_empty());
        assert_eq!(region.len(), 0);
    }

    #[test]
    fn derp_region_with_nodes() {
        let region = DerpRegion::new(1, "primary")
            .with_node(DerpNode::with_defaults("derp1.example.com"))
            .with_node(DerpNode::with_defaults("derp2.example.com"));

        assert_eq!(region.len(), 2);
        assert!(!region.is_empty());
        assert_eq!(region.nodes[0].host, "derp1.example.com");
        assert_eq!(region.nodes[1].host, "derp2.example.com");
    }

    #[test]
    fn derp_region_add_node() {
        let mut region = DerpRegion::new(1, "primary");
        region.add_node(DerpNode::with_defaults("derp.example.com"));

        assert_eq!(region.len(), 1);
    }

    #[test]
    fn derp_region_serde() {
        let region = DerpRegion::new(1, "primary")
            .with_node(DerpNode::with_defaults("derp.example.com"));

        let json = serde_json::to_string(&region).unwrap();
        let region2: DerpRegion = serde_json::from_str(&json).unwrap();

        assert_eq!(region, region2);
    }

    #[test]
    fn derp_map_new() {
        let map = DerpMap::new();

        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn derp_map_with_regions() {
        let map = DerpMap::new()
            .with_region(
                DerpRegion::new(1, "primary")
                    .with_node(DerpNode::with_defaults("derp1.example.com")),
            )
            .with_region(
                DerpRegion::new(2, "secondary")
                    .with_node(DerpNode::with_defaults("derp2.example.com")),
            );

        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
        assert!(map.get_region(1).is_some());
        assert!(map.get_region(2).is_some());
        assert!(map.get_region(3).is_none());
    }

    #[test]
    fn derp_map_add_region() {
        let mut map = DerpMap::new();
        map.add_region(DerpRegion::new(1, "primary"));

        assert_eq!(map.len(), 1);
    }

    #[test]
    fn derp_map_region_ids() {
        let map = DerpMap::new()
            .with_region(DerpRegion::new(3, "region3"))
            .with_region(DerpRegion::new(1, "region1"))
            .with_region(DerpRegion::new(2, "region2"));

        let ids = map.region_ids();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn derp_map_serde() {
        let map = DerpMap::new()
            .with_region(
                DerpRegion::new(1, "primary")
                    .with_node(DerpNode::new("derp.example.com", 443, 3478)),
            );

        let json = serde_json::to_string(&map).unwrap();
        let map2: DerpMap = serde_json::from_str(&json).unwrap();

        assert_eq!(map, map2);
    }

    #[test]
    fn derp_map_serde_matches_api_format() {
        // Test that our format matches the API response format from the spec
        let json = r#"{
            "regions": {
                "1": {
                    "region_id": 1,
                    "name": "primary",
                    "nodes": [
                        { "host": "derp.example.com", "port": 443, "stun_port": 3478 }
                    ]
                }
            }
        }"#;

        let map: DerpMap = serde_json::from_str(json).unwrap();

        assert_eq!(map.len(), 1);
        let region = map.get_region(1).unwrap();
        assert_eq!(region.name, "primary");
        assert_eq!(region.nodes.len(), 1);
        assert_eq!(region.nodes[0].host, "derp.example.com");
    }

    #[test]
    fn derp_map_empty_serde() {
        let map = DerpMap::new();
        let json = serde_json::to_string(&map).unwrap();
        let map2: DerpMap = serde_json::from_str(&json).unwrap();

        assert_eq!(map, map2);
        assert!(map2.is_empty());
    }
}
