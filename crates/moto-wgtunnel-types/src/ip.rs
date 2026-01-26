//! IP allocation types for the `WireGuard` overlay network.
//!
//! The overlay network uses IPv6 ULA space (`fd00:moto::/48`) with two subnets:
//! - Garages: [`GARAGE_SUBNET`] (`fd00:moto:1::/64`)
//! - Clients: [`CLIENT_SUBNET`] (`fd00:moto:2::/64`)
//!
//! # Why IPv6 ULA
//! - No collision with public IPs
//! - Large address space (no exhaustion concerns)
//! - Standard prefix recognized by networking tools

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::Ipv6Addr;
use std::str::FromStr;

/// Garage subnet: `fd00:moto:1::/64`
///
/// IP addresses are derived deterministically from garage IDs.
pub const GARAGE_SUBNET: Subnet = Subnet {
    // fd00:6d6f:746f:0001::  (6d6f = "mo", 746f = "to")
    // Using simplified fd00:moto: representation conceptually
    prefix: Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0001, 0, 0, 0, 0),
    prefix_len: 64,
};

/// Client subnet: `fd00:moto:2::/64`
///
/// IP addresses are allocated per device and persisted.
pub const CLIENT_SUBNET: Subnet = Subnet {
    prefix: Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0002, 0, 0, 0, 0),
    prefix_len: 64,
};

/// Error type for IP operations.
#[derive(Debug, thiserror::Error)]
pub enum IpError {
    /// IP address is not in the expected subnet.
    #[error("IP {ip} is not in subnet {subnet}")]
    NotInSubnet {
        /// The IP address that was rejected.
        ip: Ipv6Addr,
        /// The expected subnet.
        subnet: Subnet,
    },

    /// Failed to parse IP address.
    #[error("invalid IP address: {0}")]
    ParseError(#[from] std::net::AddrParseError),
}

/// An IPv6 subnet with prefix and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subnet {
    /// The network prefix address.
    pub prefix: Ipv6Addr,
    /// The prefix length in bits (0-128).
    pub prefix_len: u8,
}

impl Subnet {
    /// Check if an IP address is within this subnet.
    #[must_use]
    pub fn contains(&self, ip: Ipv6Addr) -> bool {
        let prefix_bits = u128::from(self.prefix);
        let ip_bits = u128::from(ip);
        let mask = if self.prefix_len == 0 {
            0
        } else {
            !0u128 << (128 - self.prefix_len)
        };
        (prefix_bits & mask) == (ip_bits & mask)
    }

    /// Create an IP address in this subnet from a host identifier.
    ///
    /// The host identifier is used as the lower bits of the address.
    #[must_use]
    pub fn host(&self, host_id: u64) -> Ipv6Addr {
        let prefix_bits = u128::from(self.prefix);
        let host_bits = u128::from(host_id);
        Ipv6Addr::from(prefix_bits | host_bits)
    }
}

impl fmt::Display for Subnet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.prefix, self.prefix_len)
    }
}

/// An overlay IP address within the moto `WireGuard` network.
///
/// This type ensures IP addresses are always valid overlay addresses
/// (either in [`GARAGE_SUBNET`] or [`CLIENT_SUBNET`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OverlayIp {
    inner: Ipv6Addr,
}

impl OverlayIp {
    /// Create an overlay IP from a garage host identifier.
    ///
    /// The host ID is typically derived from the garage ID hash.
    #[must_use]
    pub fn garage(host_id: u64) -> Self {
        Self {
            inner: GARAGE_SUBNET.host(host_id),
        }
    }

    /// Create an overlay IP from a client host identifier.
    ///
    /// The host ID is typically allocated sequentially or from device ID.
    #[must_use]
    pub fn client(host_id: u64) -> Self {
        Self {
            inner: CLIENT_SUBNET.host(host_id),
        }
    }

    /// Create from an existing IPv6 address, validating it's in an overlay subnet.
    ///
    /// # Errors
    /// Returns error if the IP is not in [`GARAGE_SUBNET`] or [`CLIENT_SUBNET`].
    pub fn new(ip: Ipv6Addr) -> Result<Self, IpError> {
        if GARAGE_SUBNET.contains(ip) || CLIENT_SUBNET.contains(ip) {
            Ok(Self { inner: ip })
        } else {
            // Report the expected subnet based on which is more likely
            Err(IpError::NotInSubnet {
                ip,
                subnet: GARAGE_SUBNET,
            })
        }
    }

    /// Get the underlying IPv6 address.
    #[must_use]
    pub const fn as_ipv6(&self) -> Ipv6Addr {
        self.inner
    }

    /// Check if this is a garage IP.
    #[must_use]
    pub fn is_garage(&self) -> bool {
        GARAGE_SUBNET.contains(self.inner)
    }

    /// Check if this is a client IP.
    #[must_use]
    pub fn is_client(&self) -> bool {
        CLIENT_SUBNET.contains(self.inner)
    }
}

impl fmt::Display for OverlayIp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl From<OverlayIp> for Ipv6Addr {
    fn from(ip: OverlayIp) -> Self {
        ip.inner
    }
}

impl From<OverlayIp> for String {
    fn from(ip: OverlayIp) -> Self {
        ip.inner.to_string()
    }
}

impl TryFrom<Ipv6Addr> for OverlayIp {
    type Error = IpError;

    fn try_from(ip: Ipv6Addr) -> Result<Self, Self::Error> {
        Self::new(ip)
    }
}

impl TryFrom<String> for OverlayIp {
    type Error = IpError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let ip = Ipv6Addr::from_str(&s)?;
        Self::new(ip)
    }
}

impl FromStr for OverlayIp {
    type Err = IpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ip = Ipv6Addr::from_str(s)?;
        Self::new(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subnet_contains() {
        let garage_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0001, 0, 0, 0, 1);
        let client_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0002, 0, 0, 0, 1);
        let other_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0003, 0, 0, 0, 1);

        assert!(GARAGE_SUBNET.contains(garage_ip));
        assert!(!GARAGE_SUBNET.contains(client_ip));
        assert!(!GARAGE_SUBNET.contains(other_ip));

        assert!(!CLIENT_SUBNET.contains(garage_ip));
        assert!(CLIENT_SUBNET.contains(client_ip));
        assert!(!CLIENT_SUBNET.contains(other_ip));
    }

    #[test]
    fn subnet_host() {
        let ip = GARAGE_SUBNET.host(42);
        assert_eq!(
            ip,
            Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0001, 0, 0, 0, 42)
        );

        let ip = CLIENT_SUBNET.host(0xdead_beef);
        assert_eq!(
            ip,
            Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0002, 0, 0, 0xdead, 0xbeef)
        );
    }

    #[test]
    fn overlay_ip_garage() {
        let ip = OverlayIp::garage(1);
        assert!(ip.is_garage());
        assert!(!ip.is_client());
        assert_eq!(
            ip.as_ipv6(),
            Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0001, 0, 0, 0, 1)
        );
    }

    #[test]
    fn overlay_ip_client() {
        let ip = OverlayIp::client(1);
        assert!(!ip.is_garage());
        assert!(ip.is_client());
        assert_eq!(
            ip.as_ipv6(),
            Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0002, 0, 0, 0, 1)
        );
    }

    #[test]
    fn overlay_ip_validation() {
        // Valid garage IP
        let garage_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0001, 0, 0, 0, 123);
        assert!(OverlayIp::new(garage_ip).is_ok());

        // Valid client IP
        let client_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0002, 0, 0, 0, 456);
        assert!(OverlayIp::new(client_ip).is_ok());

        // Invalid - wrong subnet
        let other_ip = Ipv6Addr::new(0xfd00, 0x6d6f, 0x746f, 0x0003, 0, 0, 0, 1);
        assert!(OverlayIp::new(other_ip).is_err());

        // Invalid - public IP
        let public_ip = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1);
        assert!(OverlayIp::new(public_ip).is_err());
    }

    #[test]
    fn overlay_ip_from_str() {
        let ip: OverlayIp = "fd00:6d6f:746f:1::1".parse().unwrap();
        assert!(ip.is_garage());

        let ip: OverlayIp = "fd00:6d6f:746f:2::1".parse().unwrap();
        assert!(ip.is_client());

        // Invalid
        assert!("fd00:6d6f:746f:3::1".parse::<OverlayIp>().is_err());
    }

    #[test]
    fn overlay_ip_display() {
        let ip = OverlayIp::garage(1);
        let s = ip.to_string();
        // Should parse back to same IP
        let ip2: OverlayIp = s.parse().unwrap();
        assert_eq!(ip, ip2);
    }

    #[test]
    fn overlay_ip_serde() {
        let ip = OverlayIp::garage(42);
        let json = serde_json::to_string(&ip).unwrap();
        let ip2: OverlayIp = serde_json::from_str(&json).unwrap();
        assert_eq!(ip, ip2);
    }

    #[test]
    fn subnet_display() {
        assert_eq!(GARAGE_SUBNET.to_string(), "fd00:6d6f:746f:1::/64");
        assert_eq!(CLIENT_SUBNET.to_string(), "fd00:6d6f:746f:2::/64");
    }
}
