//! DERP (Designated Encrypted Relay for Packets) configuration.
//!
//! This module provides DERP relay server configuration for moto-club.
//! DERP servers enable NAT traversal fallback when direct P2P connections fail.
//!
//! # Configuration
//!
//! DERP servers are configured via the `MOTO_CLUB_DERP_SERVERS` environment variable
//! as a JSON array. This is static per deployment - all replicas read the same env var.
//!
//! ```json
//! [
//!   {"region_id": 1, "region_name": "primary", "host": "derp.example.com", "port": 443, "stun_port": 3478},
//!   {"region_id": 1, "region_name": "primary", "host": "derp2.example.com", "port": 443, "stun_port": 3478}
//! ]
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::derp::parse_derp_servers_json;
//!
//! // Parse JSON config
//! let json = r#"[{"region_id":1,"region_name":"primary","host":"derp.example.com","port":443,"stun_port":3478}]"#;
//! let config = parse_derp_servers_json(json).unwrap();
//! assert_eq!(config.servers.len(), 1);
//! assert_eq!(config.servers[0].host, "derp.example.com");
//!
//! // Get as DerpMap for clients
//! let map = config.to_derp_map();
//! assert_eq!(map.len(), 1);
//! ```

use moto_wgtunnel_types::derp::{DerpMap, DerpNode, DerpRegion};
use serde::Deserialize;
use std::collections::HashMap;

/// Environment variable for DERP servers JSON config.
pub const DERP_SERVERS_ENV_VAR: &str = "MOTO_CLUB_DERP_SERVERS";

/// Error type for DERP configuration.
#[derive(Debug, thiserror::Error)]
pub enum DerpError {
    /// Environment variable not set.
    #[error("MOTO_CLUB_DERP_SERVERS environment variable not set")]
    EnvNotSet,

    /// Failed to parse JSON.
    #[error("failed to parse MOTO_CLUB_DERP_SERVERS: {0}")]
    ParseError(String),

    /// Invalid configuration.
    #[error("invalid DERP configuration: {0}")]
    InvalidConfig(String),
}

/// Result type for DERP operations.
pub type Result<T> = std::result::Result<T, DerpError>;

/// A DERP server entry from the JSON config.
#[derive(Debug, Clone, Deserialize)]
pub struct DerpServerEntry {
    /// Region ID.
    pub region_id: u16,
    /// Region name.
    pub region_name: String,
    /// Server hostname.
    pub host: String,
    /// DERP port (required).
    pub port: u16,
    /// STUN port (required).
    pub stun_port: u16,
}

/// Parsed DERP configuration from environment variable.
#[derive(Debug, Clone)]
pub struct DerpConfig {
    /// All configured DERP servers.
    pub servers: Vec<DerpServerEntry>,
}

impl DerpConfig {
    /// Create an empty DERP configuration (no servers).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            servers: Vec::new(),
        }
    }

    /// Check if configuration is empty (no servers).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Get the number of configured regions.
    #[must_use]
    pub fn region_count(&self) -> usize {
        let mut regions = std::collections::HashSet::new();
        for server in &self.servers {
            regions.insert(server.region_id);
        }
        regions.len()
    }

    /// Convert to a `DerpMap` for use by clients and garages.
    #[must_use]
    pub fn to_derp_map(&self) -> DerpMap {
        // Group servers by region
        let mut regions: HashMap<u16, (String, Vec<DerpNode>)> = HashMap::new();

        for server in &self.servers {
            let entry = regions
                .entry(server.region_id)
                .or_insert_with(|| (server.region_name.clone(), Vec::new()));
            entry
                .1
                .push(DerpNode::new(&server.host, server.port, server.stun_port));
        }

        // Build DerpMap
        let mut map = DerpMap::new();
        for (region_id, (name, nodes)) in regions {
            let mut region = DerpRegion::new(region_id, &name);
            for node in nodes {
                region.add_node(node);
            }
            map.add_region(region);
        }

        map
    }
}

/// Parse DERP servers from the `MOTO_CLUB_DERP_SERVERS` environment variable.
///
/// Returns `Ok(DerpConfig::empty())` if the env var is not set.
/// Returns an error if the env var is set but malformed.
///
/// # Errors
///
/// Returns error if the JSON is malformed or fields are invalid.
///
/// # Example Error Output
///
/// ```text
/// ERROR: Failed to parse MOTO_CLUB_DERP_SERVERS
///   Expected JSON array: [{"region_id":1,"region_name":"primary","host":"derp.example.com","port":443,"stun_port":3478}]
///   Parse error: missing field `stun_port` at index 0
/// ```
pub fn parse_derp_servers_env() -> Result<DerpConfig> {
    let json = match std::env::var(DERP_SERVERS_ENV_VAR) {
        Ok(v) if v.is_empty() => return Ok(DerpConfig::empty()),
        Ok(v) => v,
        Err(_) => return Ok(DerpConfig::empty()),
    };

    parse_derp_servers_json(&json)
}

/// Parse DERP servers from a JSON string.
///
/// # Errors
///
/// Returns error if the JSON is malformed or fields are invalid.
pub fn parse_derp_servers_json(json: &str) -> Result<DerpConfig> {
    let servers: Vec<DerpServerEntry> = serde_json::from_str(json).map_err(|e| {
        DerpError::ParseError(format!(
            "{e}\n  Expected JSON array: [{{\"region_id\":1,\"region_name\":\"primary\",\"host\":\"derp.example.com\",\"port\":443,\"stun_port\":3478}}]"
        ))
    })?;

    // Validate entries
    for (i, server) in servers.iter().enumerate() {
        if server.host.is_empty() {
            return Err(DerpError::InvalidConfig(format!(
                "server at index {i} has empty host"
            )));
        }
        if server.region_name.is_empty() {
            return Err(DerpError::InvalidConfig(format!(
                "server at index {i} has empty region_name"
            )));
        }
    }

    Ok(DerpConfig { servers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_server() {
        let json = r#"[{"region_id":1,"region_name":"primary","host":"derp.example.com","port":443,"stun_port":3478}]"#;

        let config = parse_derp_servers_json(json).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.region_count(), 1);

        let server = &config.servers[0];
        assert_eq!(server.region_id, 1);
        assert_eq!(server.region_name, "primary");
        assert_eq!(server.host, "derp.example.com");
        assert_eq!(server.port, 443);
        assert_eq!(server.stun_port, 3478);

        let map = config.to_derp_map();
        assert_eq!(map.len(), 1);
        let region = map.get_region(1).unwrap();
        assert_eq!(region.name, "primary");
        assert_eq!(region.nodes.len(), 1);
    }

    #[test]
    fn parse_multiple_servers_same_region() {
        let json = r#"[
            {"region_id":1,"region_name":"primary","host":"derp1.example.com","port":443,"stun_port":3478},
            {"region_id":1,"region_name":"primary","host":"derp2.example.com","port":443,"stun_port":3478}
        ]"#;

        let config = parse_derp_servers_json(json).unwrap();
        assert_eq!(config.servers.len(), 2);
        assert_eq!(config.region_count(), 1);

        let map = config.to_derp_map();
        assert_eq!(map.len(), 1);
        let region = map.get_region(1).unwrap();
        assert_eq!(region.nodes.len(), 2);
    }

    #[test]
    fn parse_multiple_regions() {
        let json = r#"[
            {"region_id":1,"region_name":"us-west","host":"derp.us-west.example.com","port":443,"stun_port":3478},
            {"region_id":2,"region_name":"eu-central","host":"derp.eu.example.com","port":8443,"stun_port":3479}
        ]"#;

        let config = parse_derp_servers_json(json).unwrap();
        assert_eq!(config.servers.len(), 2);
        assert_eq!(config.region_count(), 2);

        let map = config.to_derp_map();
        assert_eq!(map.len(), 2);

        let us_west = map.get_region(1).unwrap();
        assert_eq!(us_west.name, "us-west");
        assert_eq!(us_west.nodes.len(), 1);
        assert_eq!(us_west.nodes[0].host, "derp.us-west.example.com");

        let eu = map.get_region(2).unwrap();
        assert_eq!(eu.name, "eu-central");
        assert_eq!(eu.nodes.len(), 1);
        assert_eq!(eu.nodes[0].host, "derp.eu.example.com");
        assert_eq!(eu.nodes[0].port, 8443);
        assert_eq!(eu.nodes[0].stun_port, 3479);
    }

    #[test]
    fn parse_empty_array() {
        let json = "[]";

        let config = parse_derp_servers_json(json).unwrap();
        assert!(config.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        let json = "not valid json";

        let result = parse_derp_servers_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DerpError::ParseError(_)));
    }

    #[test]
    fn parse_missing_field() {
        // Missing stun_port
        let json =
            r#"[{"region_id":1,"region_name":"primary","host":"derp.example.com","port":443}]"#;

        let result = parse_derp_servers_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DerpError::ParseError(_)));
    }

    #[test]
    fn parse_empty_host() {
        let json =
            r#"[{"region_id":1,"region_name":"primary","host":"","port":443,"stun_port":3478}]"#;

        let result = parse_derp_servers_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DerpError::InvalidConfig(_)));
    }

    #[test]
    fn parse_empty_region_name() {
        let json = r#"[{"region_id":1,"region_name":"","host":"derp.example.com","port":443,"stun_port":3478}]"#;

        let result = parse_derp_servers_json(json);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DerpError::InvalidConfig(_)));
    }

    #[test]
    fn derp_config_empty() {
        let config = DerpConfig::empty();
        assert!(config.is_empty());
        assert_eq!(config.region_count(), 0);
        assert!(config.to_derp_map().is_empty());
    }
}
