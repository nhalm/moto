//! Throttle configuration: tiers, per-endpoint overrides, and builder API.

use std::collections::HashMap;

/// Principal types that determine which rate limit tier applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrincipalType {
    /// Garage workspace (SVID-authenticated).
    Garage,
    /// Deployed service engine (SVID-authenticated).
    Bike,
    /// Internal service call (service token).
    Service,
    /// Unauthenticated or unrecognized principal.
    Unknown,
}

/// Rate limit settings for a single tier.
#[derive(Debug, Clone, Copy)]
pub struct TierConfig {
    /// Requests per minute.
    pub rpm: u32,
    /// Maximum burst size.
    pub burst: u32,
}

/// Key for per-endpoint override lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OverrideKey {
    path: String,
    principal_type: PrincipalType,
}

/// Configuration for the throttle middleware.
///
/// Built with a fluent API:
/// ```ignore
/// let config = ThrottleConfig::new()
///     .tier(PrincipalType::Garage, 120, 20)
///     .tier(PrincipalType::Service, 1000, 100)
///     .override_path("/health/", PrincipalType::Garage, 0, 0)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    tiers: HashMap<PrincipalType, TierConfig>,
    overrides: HashMap<OverrideKey, TierConfig>,
}

impl ThrottleConfig {
    /// Create a new config builder with default tier settings.
    #[must_use]
    pub fn new() -> Self {
        let mut tiers = HashMap::new();
        tiers.insert(
            PrincipalType::Garage,
            TierConfig {
                rpm: 120,
                burst: 20,
            },
        );
        tiers.insert(
            PrincipalType::Bike,
            TierConfig {
                rpm: 300,
                burst: 50,
            },
        );
        tiers.insert(
            PrincipalType::Service,
            TierConfig {
                rpm: 1000,
                burst: 100,
            },
        );
        tiers.insert(PrincipalType::Unknown, TierConfig { rpm: 30, burst: 5 });

        Self {
            tiers,
            overrides: HashMap::new(),
        }
    }

    /// Set the rate limit for a principal type.
    #[must_use]
    pub fn tier(mut self, principal_type: PrincipalType, rpm: u32, burst: u32) -> Self {
        self.tiers.insert(principal_type, TierConfig { rpm, burst });
        self
    }

    /// Set a per-endpoint override. A limit of 0 means no rate limiting.
    #[must_use]
    pub fn override_path(
        mut self,
        path: &str,
        principal_type: PrincipalType,
        rpm: u32,
        burst: u32,
    ) -> Self {
        self.overrides.insert(
            OverrideKey {
                path: path.to_string(),
                principal_type,
            },
            TierConfig { rpm, burst },
        );
        self
    }

    /// Build the config (consumes self). This is a no-op currently but
    /// exists for API consistency with the spec.
    #[must_use]
    pub const fn build(self) -> Self {
        self
    }

    /// Look up the effective tier config for a principal type and path.
    /// Returns `None` if the limit is 0 (no rate limiting).
    pub(crate) fn lookup(&self, principal_type: PrincipalType, path: &str) -> Option<TierConfig> {
        // Check per-endpoint overrides first (prefix match).
        for (key, tier) in &self.overrides {
            if key.principal_type == principal_type && path.starts_with(&key.path) {
                return if tier.rpm == 0 { None } else { Some(*tier) };
            }
        }

        // Fall back to tier default.
        self.tiers.get(&principal_type).copied()
    }
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self::new()
    }
}
