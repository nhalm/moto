//! Garage operating modes.

use serde::{Deserialize, Serialize};

/// The operating mode for garage operations.
///
/// Determines whether garage operations are performed directly against K8s
/// (local mode) or through the moto-club server (remote mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GarageMode {
    /// Direct K8s access via kubeconfig (for solo dev).
    ///
    /// In this mode, the garage client talks directly to the K8s cluster
    /// using the configured kubeconfig. No moto-club server is needed.
    Local,

    /// Access through moto-club server (for team/managed).
    ///
    /// In this mode, all garage operations go through the moto-club server,
    /// which handles K8s operations, authentication, and audit logging.
    Remote {
        /// The moto-club server endpoint (e.g., "https://club.example.com").
        endpoint: String,
    },
}

impl Default for GarageMode {
    fn default() -> Self {
        Self::Local
    }
}

impl GarageMode {
    /// Creates a new remote mode with the given endpoint.
    #[must_use]
    pub fn remote(endpoint: impl Into<String>) -> Self {
        Self::Remote {
            endpoint: endpoint.into(),
        }
    }

    /// Returns `true` if this is local mode.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    /// Returns `true` if this is remote mode.
    #[must_use]
    pub const fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

    /// Returns the remote endpoint, if in remote mode.
    #[must_use]
    pub fn endpoint(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Remote { endpoint } => Some(endpoint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_local() {
        assert_eq!(GarageMode::default(), GarageMode::Local);
    }

    #[test]
    fn remote_constructor() {
        let mode = GarageMode::remote("https://club.example.com");
        assert!(mode.is_remote());
        assert!(!mode.is_local());
        assert_eq!(mode.endpoint(), Some("https://club.example.com"));
    }

    #[test]
    fn local_mode() {
        let mode = GarageMode::Local;
        assert!(mode.is_local());
        assert!(!mode.is_remote());
        assert_eq!(mode.endpoint(), None);
    }

    #[test]
    fn serde_roundtrip_local() {
        let mode = GarageMode::Local;
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: GarageMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }

    #[test]
    fn serde_roundtrip_remote() {
        let mode = GarageMode::remote("https://club.example.com");
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: GarageMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, parsed);
    }
}
