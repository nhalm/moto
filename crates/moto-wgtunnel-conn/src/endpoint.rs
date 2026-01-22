//! Endpoint selection logic for `WireGuard` connections.
//!
//! This module provides types and logic for selecting the best endpoint to use
//! when connecting to a `WireGuard` peer. The connection strategy is:
//!
//! 1. Try direct UDP connection (3 second timeout)
//! 2. If direct fails, use DERP relay
//! 3. No upgrade attempts once on DERP (simplicity for v1)
//!
//! # Example
//!
//! ```
//! use moto_wgtunnel_conn::endpoint::{Endpoint, EndpointSelector, EndpointConfig};
//! use std::net::SocketAddr;
//!
//! // Create endpoint selector with config
//! let config = EndpointConfig::default();
//! let mut selector = EndpointSelector::new(config);
//!
//! // Add a direct endpoint candidate
//! let direct_addr: SocketAddr = "203.0.113.5:51820".parse().unwrap();
//! selector.add_direct(direct_addr);
//!
//! // Get the next endpoint to try
//! if let Some(endpoint) = selector.next_endpoint() {
//!     println!("Trying endpoint: {:?}", endpoint);
//! }
//! ```

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Duration;

use moto_wgtunnel_types::DerpMap;

/// Default timeout for direct UDP connection attempts.
pub const DEFAULT_DIRECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Default timeout for DERP connection attempts.
pub const DEFAULT_DERP_TIMEOUT: Duration = Duration::from_secs(10);

/// An endpoint that can be used to reach a `WireGuard` peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// Direct UDP endpoint (IP:port).
    Direct(SocketAddr),

    /// DERP relay endpoint (region ID, region name).
    Derp {
        /// The DERP region ID.
        region_id: u16,
        /// The DERP region name (for display/logging).
        region_name: String,
    },
}

impl Endpoint {
    /// Create a direct UDP endpoint.
    #[must_use]
    pub const fn direct(addr: SocketAddr) -> Self {
        Self::Direct(addr)
    }

    /// Create a DERP relay endpoint.
    #[must_use]
    pub fn derp(region_id: u16, region_name: impl Into<String>) -> Self {
        Self::Derp {
            region_id,
            region_name: region_name.into(),
        }
    }

    /// Check if this is a direct endpoint.
    #[must_use]
    pub const fn is_direct(&self) -> bool {
        matches!(self, Self::Direct(_))
    }

    /// Check if this is a DERP relay endpoint.
    #[must_use]
    pub const fn is_derp(&self) -> bool {
        matches!(self, Self::Derp { .. })
    }

    /// Get the direct address, if this is a direct endpoint.
    #[must_use]
    pub const fn direct_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::Direct(addr) => Some(*addr),
            Self::Derp { .. } => None,
        }
    }

    /// Get the DERP region ID, if this is a DERP endpoint.
    #[must_use]
    pub const fn derp_region_id(&self) -> Option<u16> {
        match self {
            Self::Direct(_) => None,
            Self::Derp { region_id, .. } => Some(*region_id),
        }
    }

    /// Get the timeout for this endpoint type.
    #[must_use]
    pub const fn timeout(&self, config: &EndpointConfig) -> Duration {
        match self {
            Self::Direct(_) => config.direct_timeout,
            Self::Derp { .. } => config.derp_timeout,
        }
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct(addr) => write!(f, "direct:{addr}"),
            Self::Derp { region_name, .. } => write!(f, "derp:{region_name}"),
        }
    }
}

/// Configuration for endpoint selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointConfig {
    /// Timeout for direct UDP connection attempts.
    pub direct_timeout: Duration,

    /// Timeout for DERP connection attempts.
    pub derp_timeout: Duration,

    /// Whether to prefer direct connections over DERP.
    ///
    /// If true (default), direct endpoints are tried before DERP.
    /// If false, DERP is tried first (for testing or when NAT is known to block direct).
    pub prefer_direct: bool,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            direct_timeout: DEFAULT_DIRECT_TIMEOUT,
            derp_timeout: DEFAULT_DERP_TIMEOUT,
            prefer_direct: true,
        }
    }
}

impl EndpointConfig {
    /// Create a new configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the direct connection timeout.
    #[must_use]
    pub const fn with_direct_timeout(mut self, timeout: Duration) -> Self {
        self.direct_timeout = timeout;
        self
    }

    /// Set the DERP connection timeout.
    #[must_use]
    pub const fn with_derp_timeout(mut self, timeout: Duration) -> Self {
        self.derp_timeout = timeout;
        self
    }

    /// Set whether to prefer direct connections.
    #[must_use]
    pub const fn with_prefer_direct(mut self, prefer: bool) -> Self {
        self.prefer_direct = prefer;
        self
    }
}

/// Selects the best endpoint for connecting to a `WireGuard` peer.
///
/// The selector maintains a queue of candidate endpoints and returns them
/// in priority order:
/// 1. Direct UDP endpoints (if `prefer_direct` is true)
/// 2. DERP relay endpoints (sorted by region ID)
///
/// Once an endpoint is retrieved via [`next_endpoint`], it won't be returned again.
///
/// [`next_endpoint`]: EndpointSelector::next_endpoint
#[derive(Debug, Clone)]
pub struct EndpointSelector {
    /// Configuration for endpoint selection.
    config: EndpointConfig,

    /// Queue of direct endpoint candidates.
    direct_endpoints: VecDeque<SocketAddr>,

    /// Queue of DERP region candidates (sorted by region ID).
    derp_regions: VecDeque<(u16, String)>,

    /// Whether we've exhausted direct endpoints and moved to DERP.
    in_derp_mode: bool,
}

impl EndpointSelector {
    /// Create a new endpoint selector with the given configuration.
    #[must_use]
    pub const fn new(config: EndpointConfig) -> Self {
        Self {
            config,
            direct_endpoints: VecDeque::new(),
            derp_regions: VecDeque::new(),
            in_derp_mode: false,
        }
    }

    /// Create a new endpoint selector with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(EndpointConfig::default())
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &EndpointConfig {
        &self.config
    }

    /// Add a direct UDP endpoint candidate.
    pub fn add_direct(&mut self, addr: SocketAddr) {
        self.direct_endpoints.push_back(addr);
    }

    /// Add multiple direct UDP endpoint candidates.
    pub fn add_direct_all(&mut self, addrs: impl IntoIterator<Item = SocketAddr>) {
        self.direct_endpoints.extend(addrs);
    }

    /// Add a DERP region as a fallback endpoint.
    pub fn add_derp(&mut self, region_id: u16, region_name: impl Into<String>) {
        self.derp_regions.push_back((region_id, region_name.into()));
    }

    /// Add all DERP regions from a DERP map.
    ///
    /// Regions are added in sorted order by region ID for deterministic behavior.
    pub fn add_derp_map(&mut self, derp_map: &DerpMap) {
        for region_id in derp_map.region_ids() {
            if let Some(region) = derp_map.get_region(region_id) {
                // Only add regions that have at least one node
                if !region.is_empty() {
                    self.add_derp(region_id, &region.name);
                }
            }
        }
    }

    /// Get the next endpoint to try.
    ///
    /// Returns endpoints in priority order:
    /// 1. Direct endpoints (if `prefer_direct` is true and not in DERP mode)
    /// 2. DERP endpoints
    ///
    /// Returns `None` when all endpoints have been exhausted.
    pub fn next_endpoint(&mut self) -> Option<Endpoint> {
        if self.config.prefer_direct && !self.in_derp_mode {
            // Try direct endpoints first
            if let Some(addr) = self.direct_endpoints.pop_front() {
                // Check if this was the last direct endpoint
                if self.direct_endpoints.is_empty() {
                    self.in_derp_mode = true;
                }
                return Some(Endpoint::Direct(addr));
            }
            // No more direct endpoints, switch to DERP mode
            self.in_derp_mode = true;
        }

        // Try DERP endpoints
        if let Some((region_id, region_name)) = self.derp_regions.pop_front() {
            return Some(Endpoint::Derp {
                region_id,
                region_name,
            });
        }

        // If prefer_direct is false, try direct endpoints after DERP
        if !self.config.prefer_direct {
            if let Some(addr) = self.direct_endpoints.pop_front() {
                return Some(Endpoint::Direct(addr));
            }
        }

        None
    }

    /// Peek at the next endpoint without consuming it.
    #[must_use]
    pub fn peek_endpoint(&self) -> Option<Endpoint> {
        if self.config.prefer_direct && !self.in_derp_mode {
            if let Some(&addr) = self.direct_endpoints.front() {
                return Some(Endpoint::Direct(addr));
            }
        }

        if let Some((region_id, region_name)) = self.derp_regions.front() {
            return Some(Endpoint::Derp {
                region_id: *region_id,
                region_name: region_name.clone(),
            });
        }

        if !self.config.prefer_direct {
            if let Some(&addr) = self.direct_endpoints.front() {
                return Some(Endpoint::Direct(addr));
            }
        }

        None
    }

    /// Check if there are any endpoints remaining.
    #[must_use]
    pub fn has_endpoints(&self) -> bool {
        !self.direct_endpoints.is_empty() || !self.derp_regions.is_empty()
    }

    /// Check if we're currently in DERP mode (direct endpoints exhausted).
    #[must_use]
    pub const fn in_derp_mode(&self) -> bool {
        self.in_derp_mode
    }

    /// Get the number of remaining direct endpoints.
    #[must_use]
    pub fn direct_count(&self) -> usize {
        self.direct_endpoints.len()
    }

    /// Get the number of remaining DERP regions.
    #[must_use]
    pub fn derp_count(&self) -> usize {
        self.derp_regions.len()
    }

    /// Reset the selector to try endpoints again.
    ///
    /// This clears the current queue and resets DERP mode, but you'll need
    /// to add endpoints again.
    pub fn reset(&mut self) {
        self.direct_endpoints.clear();
        self.derp_regions.clear();
        self.in_derp_mode = false;
    }

    /// Mark direct connection as failed and switch to DERP mode.
    ///
    /// This is useful when a direct connection attempt times out and you
    /// want to skip remaining direct endpoints.
    pub fn switch_to_derp(&mut self) {
        self.direct_endpoints.clear();
        self.in_derp_mode = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_wgtunnel_types::{DerpNode, DerpRegion};

    #[test]
    fn endpoint_direct() {
        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        let endpoint = Endpoint::direct(addr);

        assert!(endpoint.is_direct());
        assert!(!endpoint.is_derp());
        assert_eq!(endpoint.direct_addr(), Some(addr));
        assert_eq!(endpoint.derp_region_id(), None);
        assert_eq!(endpoint.to_string(), "direct:192.0.2.1:51820");
    }

    #[test]
    fn endpoint_derp() {
        let endpoint = Endpoint::derp(1, "primary");

        assert!(!endpoint.is_direct());
        assert!(endpoint.is_derp());
        assert_eq!(endpoint.direct_addr(), None);
        assert_eq!(endpoint.derp_region_id(), Some(1));
        assert_eq!(endpoint.to_string(), "derp:primary");
    }

    #[test]
    fn endpoint_timeout() {
        let config = EndpointConfig::default();
        let direct = Endpoint::direct("192.0.2.1:51820".parse().unwrap());
        let derp = Endpoint::derp(1, "primary");

        assert_eq!(direct.timeout(&config), DEFAULT_DIRECT_TIMEOUT);
        assert_eq!(derp.timeout(&config), DEFAULT_DERP_TIMEOUT);
    }

    #[test]
    fn endpoint_config_default() {
        let config = EndpointConfig::default();

        assert_eq!(config.direct_timeout, Duration::from_secs(3));
        assert_eq!(config.derp_timeout, Duration::from_secs(10));
        assert!(config.prefer_direct);
    }

    #[test]
    fn endpoint_config_builder() {
        let config = EndpointConfig::new()
            .with_direct_timeout(Duration::from_secs(5))
            .with_derp_timeout(Duration::from_secs(15))
            .with_prefer_direct(false);

        assert_eq!(config.direct_timeout, Duration::from_secs(5));
        assert_eq!(config.derp_timeout, Duration::from_secs(15));
        assert!(!config.prefer_direct);
    }

    #[test]
    fn selector_direct_endpoints() {
        let mut selector = EndpointSelector::with_defaults();

        let addr1: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        let addr2: SocketAddr = "192.0.2.2:51820".parse().unwrap();

        selector.add_direct(addr1);
        selector.add_direct(addr2);

        assert_eq!(selector.direct_count(), 2);
        assert!(selector.has_endpoints());
        assert!(!selector.in_derp_mode());

        // First direct endpoint
        let e1 = selector.next_endpoint().unwrap();
        assert_eq!(e1, Endpoint::Direct(addr1));

        // Second direct endpoint
        let e2 = selector.next_endpoint().unwrap();
        assert_eq!(e2, Endpoint::Direct(addr2));

        // No more endpoints
        assert!(selector.in_derp_mode());
        assert!(selector.next_endpoint().is_none());
    }

    #[test]
    fn selector_derp_endpoints() {
        let mut selector = EndpointSelector::with_defaults();

        selector.add_derp(1, "primary");
        selector.add_derp(2, "secondary");

        assert_eq!(selector.derp_count(), 2);

        // No direct endpoints, but DERP mode not triggered yet due to prefer_direct
        let e1 = selector.next_endpoint().unwrap();
        assert!(selector.in_derp_mode());
        assert_eq!(e1, Endpoint::derp(1, "primary"));

        let e2 = selector.next_endpoint().unwrap();
        assert_eq!(e2, Endpoint::derp(2, "secondary"));

        assert!(selector.next_endpoint().is_none());
    }

    #[test]
    fn selector_mixed_endpoints() {
        let mut selector = EndpointSelector::with_defaults();

        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        selector.add_direct(addr);
        selector.add_derp(1, "primary");
        selector.add_derp(2, "secondary");

        // Direct first (prefer_direct = true)
        let e1 = selector.next_endpoint().unwrap();
        assert!(e1.is_direct());

        // Then DERP
        let e2 = selector.next_endpoint().unwrap();
        assert!(e2.is_derp());
        assert_eq!(e2.derp_region_id(), Some(1));

        let e3 = selector.next_endpoint().unwrap();
        assert_eq!(e3.derp_region_id(), Some(2));

        assert!(selector.next_endpoint().is_none());
    }

    #[test]
    fn selector_prefer_derp() {
        let config = EndpointConfig::new().with_prefer_direct(false);
        let mut selector = EndpointSelector::new(config);

        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        selector.add_direct(addr);
        selector.add_derp(1, "primary");

        // DERP first when prefer_direct = false
        let e1 = selector.next_endpoint().unwrap();
        assert!(e1.is_derp());

        // Then direct
        let e2 = selector.next_endpoint().unwrap();
        assert!(e2.is_direct());
    }

    #[test]
    fn selector_add_derp_map() {
        let mut selector = EndpointSelector::with_defaults();

        let derp_map = DerpMap::new()
            .with_region(
                DerpRegion::new(2, "secondary")
                    .with_node(DerpNode::with_defaults("derp2.example.com")),
            )
            .with_region(
                DerpRegion::new(1, "primary")
                    .with_node(DerpNode::with_defaults("derp1.example.com")),
            );

        selector.add_derp_map(&derp_map);

        assert_eq!(selector.derp_count(), 2);

        // Regions should be sorted by ID
        let e1 = selector.next_endpoint().unwrap();
        assert_eq!(e1.derp_region_id(), Some(1));

        let e2 = selector.next_endpoint().unwrap();
        assert_eq!(e2.derp_region_id(), Some(2));
    }

    #[test]
    fn selector_empty_derp_regions_skipped() {
        let mut selector = EndpointSelector::with_defaults();

        let derp_map = DerpMap::new()
            .with_region(DerpRegion::new(1, "empty")) // No nodes
            .with_region(
                DerpRegion::new(2, "has-nodes")
                    .with_node(DerpNode::with_defaults("derp.example.com")),
            );

        selector.add_derp_map(&derp_map);

        // Only region 2 should be added (region 1 has no nodes)
        assert_eq!(selector.derp_count(), 1);

        let e = selector.next_endpoint().unwrap();
        assert_eq!(e.derp_region_id(), Some(2));
    }

    #[test]
    fn selector_peek_endpoint() {
        let mut selector = EndpointSelector::with_defaults();

        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        selector.add_direct(addr);

        // Peek doesn't consume
        let peeked = selector.peek_endpoint().unwrap();
        assert_eq!(peeked, Endpoint::Direct(addr));
        assert_eq!(selector.direct_count(), 1);

        // Next consumes
        let next = selector.next_endpoint().unwrap();
        assert_eq!(next, Endpoint::Direct(addr));
        assert_eq!(selector.direct_count(), 0);
    }

    #[test]
    fn selector_add_direct_all() {
        let mut selector = EndpointSelector::with_defaults();

        let addrs: Vec<SocketAddr> = vec![
            "192.0.2.1:51820".parse().unwrap(),
            "192.0.2.2:51820".parse().unwrap(),
        ];

        selector.add_direct_all(addrs);
        assert_eq!(selector.direct_count(), 2);
    }

    #[test]
    fn selector_switch_to_derp() {
        let mut selector = EndpointSelector::with_defaults();

        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        selector.add_direct(addr);
        selector.add_derp(1, "primary");

        assert_eq!(selector.direct_count(), 1);
        assert!(!selector.in_derp_mode());

        // Force switch to DERP
        selector.switch_to_derp();

        assert_eq!(selector.direct_count(), 0);
        assert!(selector.in_derp_mode());

        // Next endpoint should be DERP
        let e = selector.next_endpoint().unwrap();
        assert!(e.is_derp());
    }

    #[test]
    fn selector_reset() {
        let mut selector = EndpointSelector::with_defaults();

        let addr: SocketAddr = "192.0.2.1:51820".parse().unwrap();
        selector.add_direct(addr);
        selector.next_endpoint();

        assert!(selector.in_derp_mode());
        assert_eq!(selector.direct_count(), 0);

        selector.reset();

        assert!(!selector.in_derp_mode());
        assert!(!selector.has_endpoints());
    }

    #[test]
    fn selector_no_endpoints() {
        let mut selector = EndpointSelector::with_defaults();

        assert!(!selector.has_endpoints());
        assert!(selector.next_endpoint().is_none());
        assert!(selector.peek_endpoint().is_none());
    }
}
